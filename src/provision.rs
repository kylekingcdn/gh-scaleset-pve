use crate::{
    cloud_config::{CloudConfigGenerator, CloudConfigParams},
    conf::PveConfig,
    vm_metadata::VmMetadata,
};

use proxmox_client::{
    ProxmoxClient,
    nodes::qemu::{VmConfigUpdateParams, VmCloneParams},
};
use secrecy::{ExposeSecret, SecretString};
use std::{
    collections::HashMap,
    sync::Arc,
};
use tracing::instrument;

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

    #[instrument(skip_all)]
    pub async fn provision(&self, org: String, repo: String, metadata: VmMetadata, labels: Vec<String>, runner_token: SecretString) -> color_eyre::Result<()> {
        let vmid = self.get_new_vm_id().await?;
        let template_path = self.conf.snippets_template_path();
        let output_filename = format!("{vmid}.yaml");
        let output_path = format!("{}/{output_filename}", self.conf.snippets_local_dir);

        let repo = format!("{org}/{repo}");
        let params = CloudConfigParams {
            vmid,
            repo,
            labels,
            runner_token,
        };

        tracing::info!(output_path, "Generating cloud-init config");
        let cloud_gen = CloudConfigGenerator::new(template_path, output_path);
        cloud_gen.generate(params)?;

        self.create_vm(vmid).await?;

        let mut extra = HashMap::new();
        extra.insert("cicustom".to_string(), format!("user=local-isos:snippets/{output_filename}").into());

        tracing::info!("Updating VM config with cloud-init config");
        let config = VmConfigUpdateParams {
            description: Some(metadata.to_yaml()?),
            ipconfig0: Some("ip=dhcp".to_string()),
            extra,
            ..Default::default()
        };
        self.client.update_vm_config(self.conf.node.as_str(), vmid as u32, &config).await?;

        tracing::info!(vmid, "Starting VM");
        self.client.start_vm(self.conf.node.as_str(), vmid as u32, None).await?;

        Ok(())
    }

    fn get_random_vm_id(&self) -> u16 {
        rand::random_range(self.conf.runner_vmid_min..=self.conf.runner_vmid_max)
    }

    #[instrument(skip(self))]
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

    #[instrument(skip(self))]
    async fn create_vm(&self, vmid: u16) -> color_eyre::Result<()> {
        let params = VmCloneParams {
            newid: vmid as u32,
            name: Some(format!("github-runner-{vmid}")),
            full: Some(false),
            // description: "".to_string(), // TODO: add job URL,
            ..Default::default()
        };
        tracing::info!(vmid, "Cloning VM into runner");
        self.client.clone_vm(self.conf.node.as_str(), self.conf.template_vmid as u32, &params).await?;
        Ok(())
    }
}
