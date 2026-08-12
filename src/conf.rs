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

const CONFIG_ENV_PREFIX: &str = "GH_PVE_HOOK";
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
    /// Defaults to localhost, set to `0.0.0.0` for any
    #[serde(default = "ApiConfig::listen_host_default")]
    pub listen_host: String,
    #[serde(default = "ApiConfig::listen_port_default")]
    /// Defaults to 8080
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
    pub token: SecretString,

    pub webhook_secret: SecretString,

    /// restrict webhook processing to orgs/users
    #[serde_as(as = "Option<StringWithSeparator::<CommaSeparator, String>>")]
    pub repo_owners: Option<Vec<String>>,

    #[serde(default = "GithubConfig::required_label_default")]
    pub required_label: String,

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
    pub host_url: Url,
    pub token_id: String,
    pub token_secret: SecretString,

    // pub cluster: String,
    pub node: String,

    pub template_vmid: u32,

    pub snippets_local_dir: String,
    pub snippets_template_name: String,

    #[serde(default = "PveConfig::runner_vmid_min_default")]
    pub runner_vmid_min: u32,
    #[serde(default = "PveConfig::runner_vmid_max_default")]
    pub runner_vmid_max: u32,

    #[serde_as(as = "DurationSeconds<u64>")]
    #[serde(default = "PveConfig::reap_interval_default")]
    pub reap_interval: Duration,

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
    pub fn reap_interval_default() -> Duration {
        Duration::from_mins(1)
    }
    #[must_use]
    pub fn reap_dryrun_default() -> bool {
        false
    }
    #[must_use]
    pub fn snippets_template_path(&self) -> String {
        format!("{}/{}", self.snippets_local_dir, self.snippets_template_name)
    }

}

