use crate::conf::PveConfig;

use proxmox_client::{ProxmoxClient, nodes::qemu::VmCloneParams};

use secrecy::{ExposeSecret, SecretString};
use std::sync::Arc;

pub(crate) struct Provisioner {
    conf: Arc<PveConfig>,
    client: ProxmoxClient,
}
impl Provisioner {
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
    pub async fn provision(&self, _runner_token: SecretString) -> color_eyre::Result<()> {
        let vm_id = self.get_new_vm_id().await?;
        self.create_vm(vm_id).await?;

        // TODO: cloudinit

        Ok(())
    }

    fn get_random_vm_id(&self) -> u16 {
        rand::random_range(self.conf.runner_vmid_min..=self.conf.runner_vmid_max)
    }
    async fn get_new_vm_id(&self) -> color_eyre::Result<u16> {
        let vm_ids: Vec<_> = self.client.list_cluster_resources()
            .await?
            .into_iter()
            .filter_map(|r| r.vmid)
            .map(|id| id as u16)
            .collect();
        let mut new_id = self.get_random_vm_id();
        while vm_ids.contains(&new_id) {
            new_id = self.get_random_vm_id();
        }
        Ok(new_id)
    }

    async fn create_vm(&self, vmid: u16) -> color_eyre::Result<()> {
        let params = VmCloneParams {
            newid: vmid as u32,
            name: Some(format!("github-runner-{vmid}")),
            full: Some(false),
            // description: "".to_string(), // TODO: add job URL,
            ..Default::default()
        };
        self.client.clone_vm(self.conf.node.as_str(), self.conf.template_vmid as u32, &params).await?;
        Ok(())
    }
}
