use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use bobaclaw_agent::{build_delivery_registry, AgentDispatcher};
use bobaclaw_channel_telegram::{run_telegram_polling, TelegramApi, TelegramChannelDelivery};
use bobaclaw_core::{BobaConfig, BobaPaths, IngressKind, NormalizedRequest};
use bobaclaw_scheduler::spawn_in_process_scheduler;
use bobaclaw_state::SpawnJobRecord;
use serde::{Deserialize, Serialize};

pub struct GatewayState {
    pub dispatcher: Arc<AgentDispatcher>,
    pub config: BobaConfig,
}

#[derive(Debug, Deserialize)]
pub struct ChatCompletionsRequest {
    pub model: Option<String>,
    pub messages: Vec<OpenAiMessage>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct OpenAiMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Serialize)]
pub struct ChatCompletionsResponse {
    pub id: String,
    pub object: &'static str,
    pub choices: Vec<Choice>,
}

#[derive(Debug, Serialize)]
pub struct Choice {
    pub index: u32,
    pub message: OpenAiMessage,
    pub finish_reason: &'static str,
}

#[derive(Debug, Deserialize)]
struct SpawnJobsQuery {
    session_id: String,
}

pub async fn serve(paths: BobaPaths, config: BobaConfig) -> anyhow::Result<()> {
    let dispatcher = Arc::new(AgentDispatcher::new(paths.clone(), config.clone()).await?);
    wire_spawn_feedback(&dispatcher, &paths, &config).await?;

    let state = Arc::new(GatewayState {
        dispatcher: dispatcher.clone(),
        config: config.clone(),
    });

    let app = Router::new()
        .route("/health", get(health))
        .route("/v1/chat/completions", post(chat_completions))
        .route("/api/agent", post(api_agent))
        .route("/api/agent/interrupt", post(api_agent_interrupt))
        .route("/api/spawn/jobs", get(api_spawn_jobs_list))
        .route("/api/spawn/jobs/{id}", get(api_spawn_job_get))
        .with_state(state);

    spawn_in_process_scheduler(paths.clone(), config.clone(), Some(dispatcher.clone()));

    if config.channels.telegram.enabled && config.channels.telegram.polling {
        let tg_paths = paths.clone();
        let tg_config = config.clone();
        let tg_dispatcher = dispatcher.clone();
        tokio::spawn(async move {
            if let Err(e) = run_telegram_polling(tg_paths, tg_config, Some(tg_dispatcher)).await {
                tracing::error!("telegram channel stopped: {e}");
            }
        });
        tracing::info!("telegram long-poll started");
    }

    let addr = format!("{}:{}", config.gateway.bind, config.gateway.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!(
        "gateway listening on http://{addr} (max_parallel_turns={})",
        config.gateway.max_parallel_turns
    );
    axum::serve(listener, app).await?;
    Ok(())
}

pub async fn wire_spawn_feedback(
    dispatcher: &Arc<AgentDispatcher>,
    paths: &BobaPaths,
    config: &BobaConfig,
) -> anyhow::Result<()> {
    let telegram = if config.channels.telegram.enabled {
        let api = Arc::new(TelegramApi::from_config(&config.channels.telegram)?);
        Some(Arc::new(TelegramChannelDelivery::new(config.clone(), api))
            as Arc<dyn bobaclaw_agent::ChannelDelivery>)
    } else {
        None
    };
    let deliveries = build_delivery_registry(paths.home.clone(), telegram);
    dispatcher
        .wire_spawn_feedback(config.clone(), deliveries)
        .await;
    Ok(())
}

async fn health() -> &'static str {
    "ok"
}

async fn api_spawn_jobs_list(
    State(state): State<Arc<GatewayState>>,
    Query(query): Query<SpawnJobsQuery>,
) -> Json<Vec<SpawnJobRecord>> {
    let jobs = state.dispatcher.list_spawn_jobs(&query.session_id).await;
    Json(jobs)
}

async fn api_spawn_job_get(
    State(state): State<Arc<GatewayState>>,
    Path(id): Path<String>,
) -> Json<Option<SpawnJobRecord>> {
    let job = state.dispatcher.get_spawn_job(&id).await;
    Json(job)
}

async fn chat_completions(
    State(state): State<Arc<GatewayState>>,
    Json(body): Json<ChatCompletionsRequest>,
) -> Json<ChatCompletionsResponse> {
    let user_text = body
        .messages
        .iter()
        .rev()
        .find(|m| m.role == "user")
        .map(|m| m.content.clone())
        .unwrap_or_default();

    let req = NormalizedRequest {
        request_id: uuid::Uuid::new_v4(),
        ingress: IngressKind::OpenAiCompat,
        agent_group: state.config.default_agent_group.clone(),
        session_id: None,
        channel_peer: None,
        user_text,
        attachments: Vec::new(),
        model_override: body.model,
    };

    let reply = run_agent(state, req).await;
    Json(ChatCompletionsResponse {
        id: format!("chatcmpl-{}", uuid::Uuid::new_v4()),
        object: "chat.completion",
        choices: vec![Choice {
            index: 0,
            message: OpenAiMessage {
                role: "assistant".into(),
                content: reply,
            },
            finish_reason: "stop",
        }],
    })
}

#[derive(Debug, Deserialize)]
struct ApiAgentRequest {
    message: String,
    #[serde(default)]
    agent_group: Option<String>,
}

#[derive(Debug, Serialize)]
struct ApiAgentResponse {
    reply: String,
    session_id: String,
    run_id: Option<String>,
}

async fn api_agent(
    State(state): State<Arc<GatewayState>>,
    Json(body): Json<ApiAgentRequest>,
) -> Json<ApiAgentResponse> {
    let req = NormalizedRequest {
        request_id: uuid::Uuid::new_v4(),
        ingress: IngressKind::Rest,
        agent_group: body
            .agent_group
            .unwrap_or_else(|| state.config.default_agent_group.clone()),
        session_id: None,
        channel_peer: None,
        user_text: body.message,
        attachments: Vec::new(),
        model_override: None,
    };
    match state.dispatcher.handle(req).await {
        Ok(resp) => Json(ApiAgentResponse {
            reply: resp.text,
            session_id: resp.session_id,
            run_id: resp.run_id,
        }),
        Err(e) => Json(ApiAgentResponse {
            reply: format!("error: {e}"),
            session_id: String::new(),
            run_id: None,
        }),
    }
}

#[derive(Debug, Deserialize)]
struct ApiInterruptRequest {
    #[serde(default)]
    agent_group: Option<String>,
    #[serde(default)]
    scope: Option<String>,
}

#[derive(Debug, Serialize)]
struct ApiInterruptResponse {
    interrupted: bool,
    scope: String,
}

async fn api_agent_interrupt(
    State(state): State<Arc<GatewayState>>,
    Json(body): Json<ApiInterruptRequest>,
) -> Json<ApiInterruptResponse> {
    let scope = body.scope.unwrap_or_else(|| {
        format!(
            "api:{}",
            body.agent_group
                .unwrap_or_else(|| state.config.default_agent_group.clone())
        )
    });
    let interrupted = state.dispatcher.interrupt_scope(&scope).await;
    Json(ApiInterruptResponse { interrupted, scope })
}

async fn run_agent(state: Arc<GatewayState>, req: NormalizedRequest) -> String {
    match state.dispatcher.handle(req).await {
        Ok(r) => r.text,
        Err(e) => format!("error: {e}"),
    }
}
