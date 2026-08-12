pub(crate) mod cloud_config;
pub(crate) mod conf;
pub(crate) mod middleware;
pub(crate) mod provision;
pub(crate) mod reaper;
pub(crate) mod vm_metadata;
pub(crate) mod webhook;

use crate::{
    conf::Conf,
    middleware::github_webhook::ValidateGitHubWebhookLayer,
    reaper::Reaper,
    webhook::{Event, WebhookHandler},
};

use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::post,
    Router,
};
use secrecy::ExposeSecret;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::task::JoinSet;
use tracing_kickstart::otel_sdk::propagation::TraceContextPropagator;
use tracing_kickstart::otel::global;

// !- API execution

fn default_env_filter() -> [&'static str; 12] {
    [
        "h2=info",
        "hyper_util=info",
        "hyper=info",
        "opentelemetry_sdk=info",
        "opentelemetry-http=info",
        "opentelemetry-otlp=info",
        "reqwest::connect=info",
        "rustls=info",
        "sqlx::query=info",
        "tendermint_rpc=info",
        "gh-pve-webhook=debug",
        "debug",
    ]
}

#[derive(Debug, Clone)]
pub struct AppState {
    conf: Arc<conf::SharedConf>,
}

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    // And the error handler.
    color_eyre::install()?;

    // init tracing attrs
    let tracing_svc_attrs = tracing_kickstart::build_attrs!();
    tracing_svc_attrs.dump();

    // load config
    let conf = Conf::load()?;
    println!("Current config:\n{conf:#?}");
    let conf = Arc::new(conf.into_shared());
    // configure tracing
    let default_filter = default_env_filter().join(",");
    let tracing_providers = tracing_kickstart::init(tracing_svc_attrs.clone(), &conf.tracing, Some(&default_filter), None)?;
    global::set_text_map_propagator(TraceContextPropagator::new());
    tracing_providers.register_globally();

    let state = AppState { conf: conf.clone() };

    let webhook_layer = ValidateGitHubWebhookLayer::new(conf.github.webhook_secret.expose_secret().to_string());
    let app = Router::new()
        .route("/webhook", post(webhook).layer(webhook_layer))
        .with_state(state);

    // init task set
    let mut join_set = JoinSet::new();

    // init api task
    let listen_addr = (conf.api.listen_host.clone(), conf.api.listen_port);
    let listener = TcpListener::bind(listen_addr).await.unwrap();
    tracing::info!("listening on {}", listener.local_addr().unwrap());
    join_set.spawn(async move { axum::serve(listener, app).await });

    // init reaper task
    let reaper = Reaper::new(conf.pve.clone())?;
    join_set.spawn(async move { let res = reaper.monitor().await; Ok(res) });

    while let Some(res) = join_set.join_next().await {
        tracing::error!(?res, "Task ended prematurely?");
    }

    Ok(())
}

async fn webhook(
    State(state): State<AppState>,
    event: Event,
) -> impl IntoResponse {
    tracing::debug!("Got event: {event:?}");

    let handler = WebhookHandler::new(state.conf);
    match Box::pin(handler.handle(event)).await {
        Err(error) => {
            tracing::error!("Handler failed: {error:#?}");
            StatusCode::INTERNAL_SERVER_ERROR
        },
        Ok(state) => {
            tracing::info!("Webhook decision: {state}");
            StatusCode::OK
        }
    }
}
