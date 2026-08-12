use config::{Config, Environment};
use dotenvy::dotenv;
use secrecy::SecretString;
use serde_with::{DurationSeconds, StringWithSeparator, serde_as};
use serde_with::formats::CommaSeparator;
use serde::Deserialize;
use std::net::Ipv4Addr;
use std::sync::Arc;
use std::time::Duration;
use tracing_kickstart::TracingConfig;
use url::Url;

const CONFIG_ENV_PREFIX: &str = "PVE_SCALESET";
const CONFIG_ENV_DELIM: &str = "__";

#[derive(Debug, Clone, Deserialize)]
pub struct Conf {
    #[serde(default)]
    pub api: ApiConfig,

    pub pve: PveConfig,

    pub github: GithubConfig,

    #[serde(default)]
    pub tracing: TracingConfig,
}
impl Conf {
    pub fn into_shared(self) -> SharedConf {
        self.into()
    }
}
#[derive(Debug, Clone)]
pub struct SharedConf {
    pub api: Arc<ApiConfig>,
    pub pve: Arc<PveConfig>,
    pub github: Arc<GithubConfig>,
    pub tracing: Arc<TracingConfig>,
}
impl From<Conf> for SharedConf {
    fn from(value: Conf) -> Self {
        Self {
            api: value.api.into(),
            pve: value.pve.into(),
            github: value.github.into(),
            tracing: value.tracing.into(),
        }
    }
}

impl Conf {
    pub fn load() -> color_eyre::Result<Self> {
        // load from .env file
        dotenv().ok();

        // init config settings
        let settings = Config::builder()
            .add_source(
                Environment::with_prefix(CONFIG_ENV_PREFIX)
                    .separator(CONFIG_ENV_DELIM)
                    .try_parsing(true),
            )
            .build()?;

        // load config
        let config = settings.try_deserialize::<Self>()?;

        Ok(config)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ApiConfig {
    /// IP address to listen on
    ///
    /// Set to `0.0.0.0` to listen on all addresses
    ///
    /// Default: `127.0.0.1`
    #[serde(default = "ApiConfig::listen_host_default")]
    pub listen_host: String,

    /// Port to listen on
    ///
    /// Default: `6175`
    #[serde(default = "ApiConfig::listen_port_default")]
    pub listen_port: u16,
}
impl ApiConfig {
    #[must_use]
    pub fn listen_host_default() -> String {
        Ipv4Addr::LOCALHOST.to_string()
    }
    #[must_use]
    pub fn listen_port_default() -> u16 {
        6175
    }
}
impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            listen_host: Self::listen_host_default(),
            listen_port: Self::listen_port_default(),
        }
    }
}

#[serde_as]
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct GithubConfig {
    /// GitHub token used to register runners to the triggering repo
    pub token: SecretString,

    /// Webhook secret
    pub webhook_secret: SecretString,

    /// Optional comma-separated list of repository owners (name of org/user)
    ///
    /// Used to restrict where provisioning can be triggered from.
    /// E.g. guard provisioning from forks
    ///
    /// If unset, there is no restriction
    ///
    /// WARNING: if set, but empty, no provisions will occur
    #[serde_as(as = "Option<StringWithSeparator::<CommaSeparator, String>>")]
    pub repo_owners: Option<Vec<String>>,

    /// Label that must be set on a job to consider provisioning a runner for it
    ///
    /// This must be set to a label that GitHub-hosted runners don't use in order to prevent
    /// provisioning of runners for jobs that GitHub will run
    ///
    /// Default: `self-hosted`
    #[serde(default = "GithubConfig::required_label_default")]
    pub required_label: String,

    /// Comma-separated list of labels to assign to the provisioned runners
    ///
    /// MUST contain the above 'required label'
    ///
    /// Default: `self-hosted,proxmox,ephemeral,linux,x64`
    #[serde_as(as = "StringWithSeparator::<CommaSeparator, String>")]
    #[serde(default = "GithubConfig::runner_labels_default")]
    pub runner_labels: Vec<String>,
}
impl GithubConfig {
    #[must_use]
    pub fn required_label_default() -> String {
        "self-hosted".into()
    }
    #[must_use]
    pub fn runner_labels_default() -> Vec<String> {
        vec![
            "self-hosted".into(),
            "proxmox".into(),
            "ephemeral".into(),
            "linux".into(),
            "x64".into(),
        ]
    }
}

#[serde_as]
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct PveConfig {
    /// PVE cluster/node API endpoint
    pub host_url: Url,

    /// PVE API token ID
    pub token_id: String,

    /// PVE API token secret
    pub token_secret: SecretString,

    /// Name of PVE node which hosts the template VM
    ///
    /// Runners will also be deployed here
    pub node: String,

    /// Name of PVE storage containing snippets
    ///
    /// Default: `local`
    #[serde(default = "PveConfig::snippets_storage_default")]
    pub snippets_storage: String,

    /// Local directory containing the PVE storage snippets
    ///
    /// This must be writable + write through to the actual snippets storage in use by the node
    ///
    /// Supported configurations:
    /// - running on bare metal: the PVE snippets path, no mapping required
    /// - running in LXC on same node (or shared storage): the path of the LXC mount point
    /// - elsewhere (snippets dir shared via NFS or similar): local path of (mounted) snippets dir
    pub snippets_local_dir: String,

    /// Filename of template for generated cloud init config
    pub snippets_template_name: String,

    /// VMID of runner template VM
    pub template_vmid: u32,

    /// VMID range to use for deployed runners - min
    ///
    /// Default: `5000`
    #[serde(default = "PveConfig::runner_vmid_min_default")]
    pub runner_vmid_min: u32,

    /// VMID range to use for deployed runners - max
    ///
    /// Default: `5999`
    #[serde(default = "PveConfig::runner_vmid_max_default")]
    pub runner_vmid_max: u32,

    /// Prefix used in name of runner vm
    ///
    /// Full vm name is of the form: `{prefix}{vmid}`
    ///
    /// Default: `github-runner-`
    #[serde(default = "PveConfig::runner_name_prefix_default")]
    pub runner_name_prefix: String,

    /// Optional pool to assign deployed runners to
    pub runner_pool: Option<String>,

    /// VM reaper frequency
    ///
    /// Default: `1 min`
    #[serde_as(as = "DurationSeconds<u64>")]
    #[serde(default = "PveConfig::reap_interval_default")]
    pub reap_interval: Duration,

    /// Enables dry run for VM reaper. logs decisions but doesn't destroy VMs
    ///
    /// Default: `false`
    #[serde(default = "PveConfig::reap_dryrun_default")]
    pub reap_dryrun: bool,
}
impl PveConfig {
    #[must_use]
    pub fn runner_vmid_min_default() -> u32 {
        5000
    }
    #[must_use]
    pub fn runner_vmid_max_default() -> u32 {
        5999
    }
    #[must_use]
    pub fn runner_name_prefix_default() -> String {
        "github-runner-".to_string()
    }
    #[must_use]
    pub fn reap_interval_default() -> Duration {
        Duration::from_mins(1)
    }
    #[must_use]
    pub fn reap_dryrun_default() -> bool {
        false
    }
    #[must_use]
    pub fn snippets_storage_default() -> String {
        "local".to_string()
    }
    #[must_use]
    pub fn snippets_template_path(&self) -> String {
        format!("{}/{}", self.snippets_local_dir, self.snippets_template_name)
    }
}

