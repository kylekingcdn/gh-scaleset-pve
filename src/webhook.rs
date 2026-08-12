use crate::{
    conf::SharedConf,
    provision::Provisioner,
};

use axum::{
    body::Bytes,
    extract::{FromRequest, Request},
    response::{IntoResponse, Response},
};
use derive_more::Display;
use octocrab::models::{
    Author, Repository,
    actions::SelfHostedRunnerToken,
    webhook_events::{
        payload::WorkflowJobWebhookEventAction,
        WebhookEvent, WebhookEventPayload, WebhookEventType,
    },
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::instrument;

pub(crate) struct WebhookHandler {
    conf: Arc<SharedConf>,
}
impl WebhookHandler {
    pub fn new(conf: Arc<SharedConf>) -> Self {
        Self {
            conf
        }
    }

    #[instrument(skip_all, err(Display))]
    pub async fn handle(&self, event: Event) -> HandleResult {
        // check event kind
        if event.kind != WebhookEventType::WorkflowJob {
            return IgnoreReason::EventKindMismatch { kind: json_display(event.kind) }.into();
        }
        tracing::debug!("Event kind passed: {:#?}", event.kind);

        // check owner
        let Some(owner) = event.repository_owner_name() else {
            return IgnoreReason::OwnerMissing.into();
        };
        let repo = event.repository_name().expect("repository should already be validated");

        if let Some(allowed_owners) = &self.conf.github.repo_owners && !allowed_owners.contains(&owner)  {
            return IgnoreReason::OwnerNotPermitted { owner: owner.clone() }.into();
        }
        tracing::debug!("Repository owner passed: {owner}");

        // extract payload
        let WebhookEventPayload::WorkflowJob(payload) = event.payload else {
            return Err(color_eyre::eyre::eyre!("event payload doesn't match kind").into());
        };

        // check job action
        if payload.action != WorkflowJobWebhookEventAction::Queued {
            return IgnoreReason::WorkflowJobEventActionMismatch { action: json_display(payload.action) }.into();
        }
        tracing::debug!("Workflow action passed: {:#?}", payload.action);

        // check job labels for any matching runner labels
        let Some(labels) = payload.workflow_job.get("labels").and_then(|l| l.as_array()) else {
            return Err(color_eyre::eyre::eyre!("event labels type isn't an array").into());
        };
        let label_strs: Vec<_> = labels.iter().map(|l| l.as_str().unwrap().to_string()).collect(); // FIXME: unwrap
        if !label_strs.contains(&self.conf.github.required_label) {
            return IgnoreReason::RunnerRequiredLabel { job_labels: label_strs, label: self.conf.github.required_label.clone() }.into()
        }
        tracing::debug!("Required runner label passed: {}", &self.conf.github.required_label);

        let mut missing_labels = Vec::new();
        for label in &label_strs {
            if !self.conf.github.runner_labels.contains(label) {
                missing_labels.push(label.clone());
            }
        }
        if !missing_labels.is_empty() {
            return IgnoreReason::RunnerLabelMismatch { missing_labels }.into();
        }
        tracing::debug!("Job runner labels passed: [{}]", label_strs.join(", "));

        // TODO: dedicated filter fn, returning org name

        // add new runner
        let token = self.init_runner(&owner, &repo).await?;

        // provision vm
        match Provisioner::new(self.conf.pve.clone())
        {
            Ok(provisioner) => {
                provisioner.provision(owner, repo, self.conf.github.runner_labels.clone(), token.token.into()).await?;
                HandleState::Handled.into()
            },
            Err(error) => {
                tracing::error!("failed to build provisioner: {error}");
                Err(HandleError::Eyre(error.into ()))
            }
        }
    }

    #[instrument(skip_all, err(Display))]
    async fn init_runner(&self, org: &str, repo: &str) -> color_eyre::Result<SelfHostedRunnerToken> {
        let client = octocrab::OctocrabBuilder::default()
            .personal_token(self.conf.github.token.clone())
            .build()?;

        let token = client.actions().create_repo_runner_registration_token(org, repo).await?;
        Ok(token)
    }
}

fn json_display<T: Serialize>(t: T) -> String {
    let value = serde_json::to_value(&t);
    if let Ok(value) = value && value.is_string() {
        value.as_str().unwrap().to_string()
    } else {
        serde_json::to_string_pretty(&t).expect("invalid json")
    }
}

pub type HandleResult = Result<HandleState, HandleError>;
impl From<HandleState> for HandleResult {
    fn from(state: HandleState) -> Self {
        Self::Ok(state)
    }
}
impl From<IgnoreReason> for HandleResult {
    fn from(reason: IgnoreReason) -> Self {
        Self::Ok(HandleState::Ignored { reason })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum HandleError {
    #[error("An unexpected error occured")]
    Eyre(#[from] color_eyre::Report),
}

#[derive(Debug, Clone, PartialEq, Eq, Display)]
pub enum HandleState {
    #[display("Handled")]
    Handled,

    #[display("Ignored: {reason}")]
    Ignored { reason: IgnoreReason },
}
impl From<IgnoreReason> for HandleState {
    fn from(reason: IgnoreReason) -> Self {
        Self::Ignored { reason }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Display)]
pub enum IgnoreReason {
    #[display("Event kind not handled: {kind}")]
    EventKindMismatch { kind: String },

    #[display("Workflow job event action not handled: {action}")]
    WorkflowJobEventActionMismatch { action: String },

    #[display("Required runner label '{label}' not included in job labels: {}", job_labels.join(", "))]
    RunnerRequiredLabel { label: String, job_labels: Vec<String> },

    #[display("Job has unsupported runner labels: {}", missing_labels.join(", "))]
    RunnerLabelMismatch { missing_labels: Vec<String> },

    #[display("Repository owner isn't included in the allow list: {owner}")]
    OwnerNotPermitted { owner: String },

    #[display("Repository owner missing from webhook payload")]
    OwnerMissing,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub kind: WebhookEventType,
    pub sender: Option<Author>,
    pub repository: Option<Repository>,
    pub payload: WebhookEventPayload,
}
impl Event {
    pub fn repository_owner_name(&self) -> Option<String> {
        self.repository.as_ref().and_then(|r| r.owner.as_ref()).map(|o| o.login.clone())
    }
    pub fn repository_name(&self) -> Option<String> {
        self.repository.as_ref().map(|r| r.name.clone())
    }
}

impl From<WebhookEvent> for Event {
    fn from(e: WebhookEvent) -> Self {
        Self {
            kind: e.kind,
            sender: e.sender,
            repository: e.repository,
            payload: e.specific,
        }
    }
}

impl<S> FromRequest<S> for Event
where
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let headers = req.headers().clone();
        let header = headers
            .get("x-github-event")
            .map(|x| x.to_str())
            .unwrap()
            .map_err(|_| {
                "Failed to convert header to string"
                    .to_string()
                    .into_response()
            })?;
        let bytes = Bytes::from_request(req, state)
            .await
            .map_err(IntoResponse::into_response)?;
        let webhook_event = WebhookEvent::try_from_header_and_body(header, &bytes).unwrap();
        Ok(Self::from(webhook_event))
    }
}
