use crate::channels::{ChannelPeer, DmPolicy, GroupPolicy, TelegramConfig};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatKind {
    Private,
    Group,
    Supergroup,
    Channel,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrustDecision {
    Allow,
    Deny,
    PendingPairing { code: String },
}

pub struct TrustInput<'a> {
    pub peer: &'a ChannelPeer,
    pub chat_kind: ChatKind,
    pub user_id: i64,
    pub is_bot_mentioned: bool,
    pub is_reply_to_bot: bool,
}

pub fn evaluate_telegram_trust(
    cfg: &TelegramConfig,
    input: &TrustInput<'_>,
    pairing_code: Option<String>,
) -> TrustDecision {
    match input.chat_kind {
        ChatKind::Private => evaluate_dm(cfg, input.user_id, pairing_code),
        ChatKind::Channel => TrustDecision::Deny,
        ChatKind::Group | ChatKind::Supergroup => evaluate_group(cfg, input, pairing_code),
    }
}

fn evaluate_dm(cfg: &TelegramConfig, user_id: i64, pairing_code: Option<String>) -> TrustDecision {
    match cfg.dm_policy {
        DmPolicy::Open => TrustDecision::Allow,
        DmPolicy::Allowlist => {
            if cfg.allow_from.contains(&user_id) {
                TrustDecision::Allow
            } else {
                TrustDecision::Deny
            }
        }
        DmPolicy::Pairing => {
            if cfg.allow_from.contains(&user_id) {
                TrustDecision::Allow
            } else if let Some(code) = pairing_code {
                TrustDecision::PendingPairing { code }
            } else {
                TrustDecision::Deny
            }
        }
    }
}

fn evaluate_group(
    cfg: &TelegramConfig,
    input: &TrustInput<'_>,
    _pairing_code: Option<String>,
) -> TrustDecision {
    let chat_id: i64 = input.peer.peer.parse().unwrap_or(0);

    let allowed = match cfg.group_policy {
        GroupPolicy::Disabled => return TrustDecision::Deny,
        GroupPolicy::Open => true,
        GroupPolicy::Allowlist => cfg.allowed_groups.contains(&chat_id),
    };
    if !allowed {
        return TrustDecision::Deny;
    }

    if cfg.group_require_mention && !input.is_bot_mentioned && !input.is_reply_to_bot {
        return TrustDecision::Deny;
    }

    TrustDecision::Allow
}

pub fn resolve_agent_group(
    default_group: &str,
    rules: &[crate::channels::RoutingRule],
    peer: &ChannelPeer,
) -> String {
    for rule in rules {
        if rule.r#match.channel != peer.channel {
            continue;
        }
        if route_peer_matches(&rule.r#match.peer, &peer.peer) {
            return rule.agent_group.clone();
        }
    }
    default_group.to_string()
}

fn route_peer_matches(pattern: &str, peer: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if let Some(prefix) = pattern.strip_suffix('*') {
        return peer.starts_with(prefix);
    }
    pattern == peer
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channels::{ChannelPeer, TelegramConfig};

    #[test]
    fn dm_allowlist_blocks_unknown() {
        let cfg = TelegramConfig {
            dm_policy: DmPolicy::Allowlist,
            allow_from: vec![42],
            ..Default::default()
        };
        let peer = ChannelPeer::telegram(42, None);
        let input = TrustInput {
            peer: &peer,
            chat_kind: ChatKind::Private,
            user_id: 99,
            is_bot_mentioned: false,
            is_reply_to_bot: false,
        };
        assert_eq!(
            evaluate_telegram_trust(&cfg, &input, None),
            TrustDecision::Deny
        );
    }

    #[test]
    fn group_requires_mention() {
        let cfg = TelegramConfig {
            group_policy: GroupPolicy::Open,
            group_require_mention: true,
            ..Default::default()
        };
        let peer = ChannelPeer::telegram(-100123, None);
        let input = TrustInput {
            peer: &peer,
            chat_kind: ChatKind::Supergroup,
            user_id: 1,
            is_bot_mentioned: false,
            is_reply_to_bot: false,
        };
        assert_eq!(
            evaluate_telegram_trust(&cfg, &input, None),
            TrustDecision::Deny
        );
    }

    #[test]
    fn routing_wildcard() {
        let rules = vec![crate::channels::RoutingRule {
            r#match: crate::channels::RouteMatch {
                channel: "telegram".into(),
                peer: "-100*".into(),
            },
            agent_group: "work".into(),
        }];
        let peer = ChannelPeer::telegram(-100555, None);
        assert_eq!(resolve_agent_group("home", &rules, &peer), "work");
    }
}
