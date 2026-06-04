use std::sync::Arc;

use bobaclaw_agent::AgentLoop;
use bobaclaw_core::{
    evaluate_telegram_trust, resolve_agent_group, BobaConfig, BobaPaths, DmPolicy,
    NormalizedRequest, TrustDecision, TrustInput,
};
use bobaclaw_state::{PairingStore, StateDb};
use tokio::sync::Mutex;
use tracing::{info, warn};

use crate::api::TelegramApi;
use crate::ingress::{parse_message, InboundMessage};
use crate::stream::TelegramStream;

pub async fn run_telegram_polling(
    paths: BobaPaths,
    config: BobaConfig,
) -> anyhow::Result<()> {
    let tg = &config.channels.telegram;
    let api = TelegramApi::from_config(tg)?;
    let me = api.get_me().await?;
    info!(
        "telegram bot connected: id={} username={:?}",
        me.id, me.username
    );

    let agent = AgentLoop::new(paths.clone(), config.clone()).await?;
    let agent = Arc::new(Mutex::new(agent));
    let state = StateDb::open(&paths.state_db).await?;
    let bot_id = me.id;
    let bot_username = me.username.clone();
    let stream_ms = tg.stream_edit_interval_ms;

    let mut offset: i64 = 0;
    loop {
        let updates = match api.get_updates(offset, 50).await {
            Ok(u) => u,
            Err(e) => {
                warn!("telegram getUpdates: {e}");
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
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

            let thread_id = inbound.peer.thread_id.as_ref().and_then(|t| t.parse().ok());
            let placeholder = api
                .send_message(
                    inbound.peer.peer.parse().unwrap_or(0),
                    "…",
                    Some(inbound.message_id),
                    thread_id,
                )
                .await?;

            let _ = api
                .send_chat_action(
                    inbound.peer.peer.parse().unwrap_or(0),
                    "typing",
                )
                .await;

            let stream = TelegramStream::new(
                api.clone(),
                placeholder.chat.id,
                placeholder.message_id,
                stream_ms,
            );

            let req = NormalizedRequest::telegram(&inbound.text, &agent_group, inbound.peer.clone());
            let agent = agent.clone();
            let result = {
                let a = agent.lock().await;
                a.handle_with_progress(req, Some(&stream)).await
            };

            match result {
                Ok(resp) => {
                    if let Err(e) = stream.finalize(&resp.text).await {
                        warn!("telegram finalize: {e}");
                        let _ = api
                            .send_message(
                                placeholder.chat.id,
                                &resp.text,
                                None,
                                thread_id,
                            )
                            .await;
                    }
                }
                Err(e) => {
                    let err = format!("Ошибка: {e}");
                    let _ = stream.finalize(&err).await;
                }
            }
        }
    }
}

async fn check_trust(
    config: &BobaConfig,
    state: &StateDb,
    inbound: &InboundMessage,
) -> bool {
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
                .create_or_get_pending(
                    "telegram",
                    &peer_key,
                    inbound.user_name.as_deref(),
                )
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
                "BobaClaw: нужна привязка.\nКод: `{code}`\nНа сервере: `bobaclaw pairing approve telegram {code}`"
            );
            let _ = api_token
                .send_message(chat_id, &text, Some(inbound.message_id), None)
                .await;
            false
        }
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
        "Код привязки BobaClaw: `{code}`\nВыполните на сервере:\n`bobaclaw pairing approve telegram {code}`\n\nDM policy: {:?}",
        config.channels.telegram.dm_policy
    );
    api.send_message(chat_id, &text, Some(inbound.message_id), None)
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
