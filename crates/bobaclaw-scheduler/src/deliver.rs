use std::path::Path;

use bobaclaw_channel_telegram::TelegramApi;
use bobaclaw_core::BobaConfig;

pub async fn deliver_message(
    config: &BobaConfig,
    paths_home: &Path,
    channel: &str,
    peer: Option<&str>,
    text: &str,
) -> anyhow::Result<()> {
    match channel {
        "telegram" => {
            let peer = peer.ok_or_else(|| anyhow::anyhow!("telegram deliver requires peer chat id"))?;
            let chat_id: i64 = peer.parse()?;
            let tg = &config.channels.telegram;
            let api = TelegramApi::from_config(tg)?;
            api.send_message(chat_id, text, None, None, tg.format).await?;
            Ok(())
        }
        "cli" | _ => {
            let outbox = paths_home.join("outbox");
            std::fs::create_dir_all(&outbox)?;
            let path = outbox.join(format!(
                "due_{}.txt",
                chrono::Utc::now().timestamp()
            ));
            std::fs::write(&path, text)?;
            tracing::info!(
                "scheduled delivery written to {}",
                path.display()
            );
            Ok(())
        }
    }
}
