use chrono::{DateTime, Utc};
use octocrab::models::{
    Repository,
    Author,
    webhook_events::{
        payload::WorkflowJobWebhookEventPayload,
}};
use serde::{Deserialize, Serialize};
use serde_with::{serde_as, TimestampSeconds};
use url::Url;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WorkflowJobPayloadData {
    id: u64,
    run_id: u64,
    workflow_name: String,

    created_at: DateTime<Utc>,

    name: String,
}

#[serde_as]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct VmMetadata {
    workflow_name: String,

    repo: String,
    repo_url: Url,

    job_id: u64,
    job_url: Url,
    job_name: String,
    job_created_at: DateTime<Utc>,

    run_id: u64,
    run_url: Url,

    #[serde_as(as = "TimestampSeconds<i64>")]
    vm_created_at: DateTime<Utc>,
}
impl VmMetadata {
    pub fn try_from_event(
        workflow_job: &WorkflowJobWebhookEventPayload,
        repo: &Repository,
        owner: &Author,
    ) -> Result<Self, VmMetadataError> {
        let parsed_data: WorkflowJobPayloadData = serde_json::from_value(workflow_job.workflow_job.clone())
            .map_err(|e| VmMetadataError::PayloadDeserialize(e.to_string()))?;

        let repo_name = format!("{}/{}", owner.login, repo.name);
        let repo_url = format!("https://github.com/{repo_name}").as_str().try_into().unwrap();
        let run_url = Self::run_url(&repo_name, parsed_data.run_id);
        let job_url = Self::job_url(&repo_name, parsed_data.run_id, parsed_data.id);

        Ok(Self {
            workflow_name: parsed_data.workflow_name,

            repo: repo_name,
            repo_url,

            job_id: parsed_data.id,
            job_url,
            job_name: parsed_data.name,
            job_created_at: parsed_data.created_at,

            run_id: parsed_data.run_id,
            run_url,

            vm_created_at: Utc::now(),
        })
    }
    fn run_url(repo: &str, run_id: u64) -> Url {
        let url_str = format!("https://github.com/{repo}/actions/runs/{run_id}");
        Url::try_from(url_str.as_str()).unwrap()
    }
    fn job_url(repo: &str, run_id: u64, job_id: u64) -> Url {
        let run_url = Self::run_url(repo, run_id);
        let url_str = format!("{run_url}/job/{job_id}", );
        Url::try_from(url_str.as_str()).unwrap()
    }
    pub fn to_yaml(&self) -> serde_yaml::Result<String> {
        serde_yaml::to_string(self).map(|yaml| yaml.replace("\n", "\n\n"))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum VmMetadataError {
    #[error("Failed to deserialize webhook payload data: {0}")]
    PayloadDeserialize(String),
}
