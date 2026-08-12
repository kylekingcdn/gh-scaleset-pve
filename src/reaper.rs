use crate::{
    conf::PveConfig,
    vm_metadata::VmMetadata,
};

use chrono::TimeDelta;
use proxmox_client::{
    ProxmoxClient,
    cluster::ClusterResource,
    nodes::qemu::{VmDeleteParams, VmPowerParams},
};
use secrecy::ExposeSecret;
use tracing::instrument;
use std::sync::Arc;
use tokio::time::{interval, MissedTickBehavior};

#[derive(Debug, Copy, Clone)]
pub struct ReapDecision {
    reap: bool,
    vm_is_running: bool,
}
impl ReapDecision {
    pub fn new_from_status(reap: bool, status: &str) -> Self {
        Self {
            reap,
            vm_is_running: status == "running",
        }
    }
    pub fn reap(status: &str) -> Self {
        Self::new_from_status(true, status)
    }
    pub fn skip(status: &str) -> Self {
        Self::new_from_status(false, status)
    }
}
pub(crate) struct Reaper {
    conf: Arc<PveConfig>,
    client: ProxmoxClient,
}
impl Reaper {
    /// Min age to consider vm for reaping
    ///
    /// Useful to prevent deletion of vms
    /// that were just created and not yet started)
    const MIN_VM_AGE: TimeDelta = TimeDelta::minutes(1);

    /// Max time a job/vm may run/live for before being terminated
    const MAX_VM_AGE: TimeDelta = TimeDelta::hours(1);

    pub fn new(conf: Arc<PveConfig>) -> Result<Self, proxmox_client::Error> {
        Ok(Self {
            client: ProxmoxClient::with_api_token(
                conf.host_url.as_str(),
                &conf.token_id,
                conf.token_secret.expose_secret(),
            )?,
            conf,
        })
    }

    pub async fn monitor(&self) {
        let reap_interval = self.conf.reap_interval;
        tracing::info!(?reap_interval, "Starting vm reaper");

        let mut interval = interval(reap_interval);
        interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
        interval.tick().await;

        loop {
            interval.tick().await;
            let reap_vms_res = self.reap_vms().await;
            if let Err(error) = reap_vms_res {
                tracing::error!("Error reaping vms: {error}");
            }
        }
    }

    #[instrument(skip_all)]
    async fn reap_vms(&self) -> color_eyre::Result<()> {
        tracing::info!("Checking for VMs to prune");
        let resources = self.client.list_cluster_resources().await?;

        for resource in resources {
            if let Some(id) = resource.vmid {
                let id = id as u32;
                // base case - vmid range
                if id >= self.conf.runner_vmid_min && id <= self.conf.runner_vmid_max {
                    // determine if vm should be purged
                    match self.check_reap(id, &resource).await {
                        Ok(decision) => {
                            if decision.reap {
                                // execute deletion
                                let reap_res = self.reap_vm(id, decision.vm_is_running).await;
                                if let Err(error) = reap_res {
                                    tracing::error!(id, "Error pruning vm: {error}");
                                }
                            }
                        },
                        Err(error) => {
                            tracing::error!("Skipping prune on vm due to check error: {error}");
                        },
                    }
                }
            }
        }
        Ok(())
    }

    #[instrument(skip_all, fields(vmid, name=?resource.name))]
    async fn check_reap(&self, vmid: u32, resource: &ClusterResource) -> color_eyre::Result<ReapDecision> {
        let vm_name = resource.name.clone().unwrap_or("[no name]".to_string());
        tracing::info!(vmid, vm_name, "Checking vm for prune");

        // fetch config, parse status + description
        let config = self.client.get_vm_config(&self.conf.node, vmid).await?;
        let Some(description_url) = config.description else {
            color_eyre::eyre::bail!("Failed to get vm description");
        };
        // fixme: store as base64 json in comment
        let Ok(description) = urlencoding::decode(&description_url) else {
            color_eyre::eyre::bail!("Failed to decode vm description");
        };
        // tracing::debug!("decoded description: {description}");

        let Some(status) = resource.status.as_deref() else {
            color_eyre::eyre::bail!("Failed to get vm status");
        };
        let metadata: VmMetadata = serde_yaml::from_str(&description)?;
        let vm_age = metadata.vm_age();
        let vm_age_sec = vm_age.num_seconds();

        // check min age
        if vm_age < Self::MIN_VM_AGE {
            tracing::info!(vmid, vm_name, vm_age_sec, threshold_sec=Self::MIN_VM_AGE.num_seconds(), "Not pruning VM - newer than min age threshold");
            return Ok(ReapDecision::skip(status));
        }

        // runner pool manually configured, add extra check for matching pool
        #[allow(clippy::collapsible_if)]
        if self.conf.runner_pool.is_some() {
            if self.conf.runner_pool != resource.pool {
                tracing::warn!(vmid, vm_name, configured_pool=self.conf.runner_pool, vm_pool=resource.pool, "Not pruning VM - vm is not a member of the configured pool");
            }
            // else {
            //     tracing::debug!(vmid, vm_name, configured_pool=self.conf.runner_pool, "VM is a member of the configured runners pool");
            // }
        }

        // check status
        match status {
            "running" => {
                // check max age
                if vm_age > Self::MAX_VM_AGE {
                    tracing::info!(vmid, vm_name, vm_age_sec, threshold_min=Self::MIN_VM_AGE.num_minutes(), "Pruning running VM - older than max age threshold");
                    Ok(ReapDecision::reap(status))
                } else {
                    tracing::info!(vmid, vm_name, vm_age_sec, threshold_sec=Self::MAX_VM_AGE.num_seconds(), "Not pruning running VM - newer than min age threshold");
                    Ok(ReapDecision::skip(status))
                }
            },
            "stopped" => {
                tracing::info!(vmid, vm_name, vm_age_sec, "Issuing prune on non-running VM");
                Ok(ReapDecision::reap(status))
            },
            status_name => unreachable!("unexpected status: {status_name}"),
        }
    }

    #[instrument(skip(self))]
    async fn reap_vm(&self, vmid: u32, is_running: bool) -> color_eyre::Result<()> {
        tracing::warn!(vmid, is_running, "Pruning VM");

        // stop vm if running
        if is_running {
            tracing::info!(vmid, "Stopping VM");
            let params = VmPowerParams {
                force_stop: true.into(),
                ..Default::default()
            };
            self.client.stop_vm(&self.conf.node, vmid, (&params).into()).await?;
            tracing::info!(vmid, "VM stopped");
        }

        let params = VmDeleteParams {
            destroy_unreferenced_disks: Some(true),
            purge: Some(true),
            ..Default::default()
        };
        if self.conf.reap_dryrun {
            tracing::warn!(vmid, "Destroying VM ---- dry-run ----");
        } else {
            tracing::warn!(vmid, "Destroying VM");
            self.client.delete_vm(&self.conf.node, vmid, Some(&params)).await?;
        }
        tracing::warn!(vmid, "VM destroyed");

        Ok(())
    }
}
