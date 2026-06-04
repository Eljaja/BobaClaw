use std::sync::Arc;

use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use bobaclaw_agent::AgentLoop;
use bobaclaw_channel_telegram::run_telegram_polling;
use bobaclaw_scheduler::spawn_embedded_scheduler;
use bobaclaw_core::{BobaConfig, BobaPaths, IngressKind, NormalizedRequest};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

pub struct GatewayState {
    pub agent: Mutex<AgentLoop>,
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

pub async fn serve(paths: BobaPaths, config: BobaConfig) -> anyhow::Result<()> {
    let agent = AgentLoop::new(paths.clone(), config.clone()).await?;
    let state = Arc::new(GatewayState {
        agent: Mutex::new(agent),
        config: config.clone(),
    });

    let app = Router::new()
        .route("/health", get(health))
        .route("/v1/chat/completions", post(chat_completions))
        .route("/api/agent", post(api_agent))
        .with_state(state);

    spawn_embedded_scheduler(paths.clone(), config.clone());

    if config.channels.telegram.enabled && config.channels.telegram.polling {
        let tg_paths = paths.clone();
        let tg_config = config.clone();
        tokio::spawn(async move {
            if let Err(e) = run_telegram_polling(tg_paths, tg_config).await {
                tracing::error!("telegram channel stopped: {e}");
            }
        });
        tracing::info!("telegram long-poll started");
    }

    let addr = format!("{}:{}", config.gateway.bind, config.gateway.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("gateway listening on http://{addr}");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn health() -> &'static str {
    "ok"
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
        model_override: None,
    };
    let agent = state.agent.lock().await;
    match agent.handle(req).await {
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

async fn run_agent(state: Arc<GatewayState>, req: NormalizedRequest) -> String {
    let agent = state.agent.lock().await;
    match agent.handle(req).await {
        Ok(r) => r.text,
        Err(e) => format!("error: {e}"),
    }
}
