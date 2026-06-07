use std::sync::Arc;

use bobaclaw_agent::AgentDispatcher;
use bobaclaw_core::{
    evaluate_telegram_trust, resolve_agent_group, BobaConfig, BobaPaths, DmPolicy, IngressKind,
    NormalizedRequest, TelegramFormat, TrustDecision, TrustInput, WorkspaceAttachment,
};
use bobaclaw_state::{PairingStore, SessionStore, StateDb};
use tracing::{error, info, warn};

use crate::api::TelegramApi;
use crate::commands::{parse_slash_command, telegram_help_text};
use crate::ingress::{message_has_attachments, parse_message, InboundMessage};
use crate::media::download_message_media;
use crate::status::{initial_activity, stream_message};
use crate::stream::TelegramStream;

pub async fn run_telegram_polling(
    paths: BobaPaths,
    config: BobaConfig,
    shared_dispatcher: Option<Arc<AgentDispatcher>>,
) -> anyhow::Result<()> {
    let tg = &config.channels.telegram;
    match tg.resolve_proxy() {
        Some(p) => info!("telegram Bot API proxy: {p}"),
        None => info!("telegram Bot API: direct (no proxy)"),
    }
    let api = TelegramApi::from_config(tg)?;
    let me = api.get_me().await?;
    info!(
        "telegram bot connected: id={} username={:?}",
        me.id, me.username
    );
    if let Err(e) = api.delete_webhook().await {
        warn!("telegram deleteWebhook: {e}");
    } else {
        info!("telegram webhook cleared (long-poll mode)");
    }
    if let Err(e) = api.set_my_commands().await {
        warn!("telegram setMyCommands: {e}");
    }

    let dispatcher = match shared_dispatcher {
        Some(d) => d,
        None => Arc::new(AgentDispatcher::new(paths.clone(), config.clone()).await?),
    };
    let state = StateDb::open(&paths.state_db).await?;
    let bot_id = me.id;
    let bot_username = me.username.clone();
    let stream_ms = tg.stream_edit_interval_ms;
    let msg_format = tg.format;

    let mut offset: i64 = 0;
    loop {
        let updates = match api.get_updates(offset, 50).await {
            Ok(u) => u,
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("Conflict") || msg.contains("409") {
                    error!(
                        "telegram getUpdates conflict: another bobaclaw/gateway/channel process is \
                         already polling this bot token. Stop all duplicates (host binary, second \
                         container, old compose stack). Only one getUpdates client is allowed. \
                         detail: {e}"
                    );
                    tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                } else {
                    warn!("telegram getUpdates: {e}");
                    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                }
                continue;
            }
        };

        for update in updates {
            offset = update.update_id + 1;
            let msg = update.message.or(update.edited_message);
            let Some(msg) = msg else { continue };

            let Some(inbound) = parse_message(&msg, bot_id, bot_username.as_deref()) else {
                continue;
            };

            if inbound.text.starts_with("/pair") || inbound.text == "/start" {
                let _ = handle_pairing_command(&api, &config, &state, &inbound).await;
                continue;
            }

            if !check_trust(&config, &state, &inbound).await {
                continue;
            }

            let agent_group = resolve_agent_group(
                &config.default_agent_group,
                &config.routing.rules,
                &inbound.peer,
            );

            if let Some((reply, stop_only)) =
                handle_slash_command(&state, &inbound, &agent_group, bot_username.as_deref())
                    .await?
            {
                if stop_only {
                    let scope = NormalizedRequest::telegram(
                        &inbound.text,
                        &agent_group,
                        inbound.peer.clone(),
                        Vec::new(),
                    )
                    .dispatch_scope();
                    let _ = dispatcher.interrupt_scope(&scope).await;
                }
                let chat_id: i64 = inbound.peer.peer.parse().unwrap_or(0);
                let thread_id = inbound.peer.thread_id.as_ref().and_then(|t| t.parse().ok());
                api.send_message(
                    chat_id,
                    &reply,
                    Some(inbound.message_id),
                    thread_id,
                    msg_format,
                )
                .await?;
                continue;
            }

            let attachments: Vec<WorkspaceAttachment> = if message_has_attachments(&msg) {
                download_message_media(&api, &paths, &agent_group, &msg)
                    .await
                    .into_iter()
                    .map(Into::into)
                    .collect()
            } else {
                Vec::new()
            };

            let dispatcher = dispatcher.clone();
            let api = api.clone();
            let inbound = inbound.clone();
            tokio::spawn(async move {
                if let Err(e) = run_agent_turn(
                    &dispatcher,
                    &api,
                    &inbound,
                    &agent_group,
                    attachments,
                    stream_ms,
                    msg_format,
                )
                .await
                {
                    warn!("telegram agent turn: {e}");
                }
            });
        }
    }
}

async fn run_agent_turn(
    dispatcher: &AgentDispatcher,
    api: &TelegramApi,
    inbound: &InboundMessage,
    agent_group: &str,
    attachments: Vec<WorkspaceAttachment>,
    stream_ms: u64,
    msg_format: TelegramFormat,
) -> anyhow::Result<()> {
    let chat_id: i64 = inbound.peer.peer.parse().unwrap_or(0);
    let thread_id = inbound.peer.thread_id.as_ref().and_then(|t| t.parse().ok());

    let placeholder = api
        .send_message(
            chat_id,
            &stream_message(initial_activity()),
            Some(inbound.message_id),
            thread_id,
            TelegramFormat::Plain,
        )
        .await?;

    let _ = api.send_chat_action(chat_id, "typing").await;

    let stream = TelegramStream::new(
        api.clone(),
        placeholder.chat.id,
        placeholder.message_id,
        stream_ms,
        msg_format,
    );

    let req = NormalizedRequest::telegram(
        &inbound.text,
        agent_group,
        inbound.peer.clone(),
        attachments,
    );

    match dispatcher.handle_with_progress(req, Some(&stream)).await {
        Ok(resp) => {
            if let Err(e) = stream.finalize_with_fallback(&resp.text).await {
                warn!("telegram finalize (all retries failed): {e}");
            }
        }
        Err(e) => {
            let err = if e.to_string().contains("Validation of the message failed") {
                format!(
                    "Error: LLM-провайдер (ASI Cloud) отклонил формат сообщений: {e}\n\
                     Попробуйте /new для сброса сессии и отправьте запрос снова."
                )
            } else {
                format!("Error: {e}")
            };
            let _ = stream.finalize_with_fallback(&err).await;
        }
    }
    Ok(())
}

async fn check_trust(config: &BobaConfig, state: &StateDb, inbound: &InboundMessage) -> bool {
    let tg = &config.channels.telegram;
    let pairing = PairingStore::new(state.pool());

    let peer_key = inbound.user_id.to_string();
    let approved = pairing
        .is_approved("telegram", &peer_key)
        .await
        .unwrap_or(false);

    let mut allow_from = tg.allow_from.clone();
    if approved {
        allow_from.push(inbound.user_id);
    }

    let mut cfg = tg.clone();
    cfg.allow_from = allow_from;

    let pairing_code = if matches!(tg.dm_policy, DmPolicy::Pairing)
        && inbound.chat_kind == bobaclaw_core::ChatKind::Private
    {
        if approved {
            None
        } else {
            let code = pairing
                .create_or_get_pending("telegram", &peer_key, inbound.user_name.as_deref())
                .await
                .ok();
            code
        }
    } else {
        None
    };

    let input = TrustInput {
        peer: &inbound.peer,
        chat_kind: inbound.chat_kind,
        user_id: inbound.user_id,
        is_bot_mentioned: inbound.is_bot_mentioned,
        is_reply_to_bot: inbound.is_reply_to_bot,
    };

    match evaluate_telegram_trust(&cfg, &input, pairing_code.clone()) {
        TrustDecision::Allow => true,
        TrustDecision::Deny => false,
        TrustDecision::PendingPairing { code } => {
            let api_token = match TelegramApi::from_config(tg) {
                Ok(api) => api,
                Err(_) => return false,
            };
            let chat_id: i64 = inbound.peer.peer.parse().unwrap_or(0);
            let text = format!(
                "BobaClaw: pairing required.\nCode: `{code}`\nOn the server run: `bobaclaw pairing approve telegram {code}`"
            );
            let _ = api_token
                .send_message(
                    chat_id,
                    &text,
                    Some(inbound.message_id),
                    None,
                    TelegramFormat::Plain,
                )
                .await;
            false
        }
    }
}

async fn handle_slash_command(
    state: &StateDb,
    inbound: &InboundMessage,
    agent_group: &str,
    bot_username: Option<&str>,
) -> anyhow::Result<Option<(String, bool)>> {
    let Some((cmd, _args)) = parse_slash_command(&inbound.text, bot_username) else {
        return Ok(None);
    };

    match cmd {
        "new" | "clear" | "reset" => {
            let sessions = SessionStore::new(state.pool());
            let (ended, session_id) = sessions
                .reset_routed_session(&inbound.peer, agent_group, IngressKind::Telegram)
                .await?;
            let note = if ended > 0 {
                "История сброшена."
            } else {
                "Новая сессия."
            };
            Ok(Some((format!("{note}\nsession={session_id}"), false)))
        }
        "help" | "h" | "commands" => Ok(Some((telegram_help_text().to_string(), false))),
        "stop" => Ok(Some(("⚡ Прерываю текущий запрос…".into(), true))),
        _ => Ok(None),
    }
}

async fn handle_pairing_command(
    api: &TelegramApi,
    config: &BobaConfig,
    state: &StateDb,
    inbound: &InboundMessage,
) -> anyhow::Result<()> {
    if inbound.chat_kind != bobaclaw_core::ChatKind::Private {
        return Ok(());
    }
    let pairing = PairingStore::new(state.pool());
    let peer_key = inbound.user_id.to_string();
    let code = pairing
        .create_or_get_pending("telegram", &peer_key, inbound.user_name.as_deref())
        .await?;
    let chat_id: i64 = inbound.peer.peer.parse().unwrap_or(0);
    let text = format!(
        "BobaClaw pairing code: `{code}`\nOn the server run:\n`bobaclaw pairing approve telegram {code}`\n\nDM policy: {:?}",
        config.channels.telegram.dm_policy
    );
    api.send_message(
        chat_id,
        &text,
        Some(inbound.message_id),
        None,
        TelegramFormat::Plain,
    )
    .await?;
    Ok(())
}

pub async fn approve_pairing(
    paths: &BobaPaths,
    channel: &str,
    code: &str,
) -> anyhow::Result<Option<String>> {
    let state = StateDb::open(&paths.state_db).await?;
    let store = PairingStore::new(state.pool());
    let peer = store.approve_by_code(channel, code).await?;
    if let Some(ref p) = peer {
        let mut cfg = BobaConfig::load(&paths.config)?;
        let uid: i64 = p.parse().unwrap_or(0);
        if uid != 0 && !cfg.channels.telegram.allow_from.contains(&uid) {
            cfg.channels.telegram.allow_from.push(uid);
            BobaConfig::save(&paths.config, &cfg)?;
        }
    }
    Ok(peer)
}

pub async fn list_pending_pairing(
    paths: &BobaPaths,
    channel: Option<&str>,
) -> anyhow::Result<Vec<bobaclaw_state::PairingRow>> {
    let state = StateDb::open(&paths.state_db).await?;
    PairingStore::new(state.pool()).list_pending(channel).await
}
