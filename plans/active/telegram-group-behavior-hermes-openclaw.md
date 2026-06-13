# Agent change plan

## Goal

Bring BobaClaw Telegram **group** behavior to parity with Hermes and OpenClaw: observe-mode context, finer allowlists, wake-word triggers, per-group overrides, and safer group prompts — without widening the attack surface by default.

## Context

**Current BobaClaw behavior** (`crates/bobaclaw-core/src/policy.rs`, `crates/bobaclaw-channel-telegram/src/runtime.rs`):

| Control | Today |
|---------|-------|
| Group access | `group_policy`: `allowlist` \| `open` \| `disabled` + `allowed_groups: []` |
| Trigger gate | `group_require_mention: true` — `@bot` entity or reply-to-bot only |
| Unmentioned messages | Silently dropped (no transcript, no agent dispatch) |
| Sender in groups | **Not** checked — `allow_from` applies to DMs only |
| Per-group overrides | None (global config only) |
| Wake words / regex | None |
| Forum topic denylist | None |
| Slash commands in groups | Parsed only **after** trust; `/new@bot` does not bypass mention gate unless entities include a mention |
| Pairing | DMs only |

Production config (`~/.bobaclaw/config.yaml`) has `allowed_groups: []`, so **all group traffic is denied** even if Telegram delivers it.

**Reference implementations** (do not copy wholesale; adapt to BobaClaw policy crate + SQLite sessions):

- Hermes: `references/hermes-agent/website/docs/user-guide/messaging/telegram.md` — `observe_unmentioned_group_messages`, `group_allowed_chats` vs `allowed_chats`, `group_allow_from`, `mention_patterns`, `ignored_threads`, `exclusive_bot_mentions`, sender tagging `[nickname\|user_id]`, per-turn safety prompt for observed context.
- OpenClaw: `references/openclaw/docs/channels/groups.md`, `references/openclaw/docs/channels/telegram.md` — `groupAllowFrom`, per-group `groups.{id}.requireMention`, `messages.groupChat.mentionPatterns`, `/activation always|mention`, `unmentionedInbound: "room_event"` (ambient), `visibleReplies: "message_tool"` for lurk rooms.

**Operator note (out of scope for code, document in harness):** Telegram BotFather **Group Privacy** must be off or the bot must be group admin, or ordinary messages never reach the gateway.

## Scope

### In scope (phased)

#### Phase 1 — Access + triggers (P1)

1. **Per-group config map** — replace flat `allowed_groups` + global flags with:
   ```yaml
   channels:
     telegram:
       group_policy: allowlist   # disabled | open | allowlist
       groups:
         "-1001234567890":
           require_mention: true
           observe_unmentioned: false
         "*":
           require_mention: true
   ```
   Back-compat: `allowed_groups` + `group_require_mention` migrate to `groups` entries via `bobaclaw doctor --fix` (or load-time merge).

2. **`group_allow_from`** — optional sender allowlist for group/supergroup triggers (Hermes/OpenClaw). Empty = any member of an allowed group may trigger. Does not grant DM access.

3. **`mention_patterns`** — case-insensitive regex list (Rust `regex` crate); match text + caption; invalid patterns log warning and are skipped. `/command@botname` counts as mention (Hermes parity).

4. **`ignored_threads`** — list of forum `thread_id` values; drop before other gates.

5. **`free_response_groups`** — chat IDs where `require_mention` is effectively false (Hermes `free_response_chats`).

6. **Doctor + harness** — checks: privacy-mode hint, empty `groups` under allowlist, regex validity; extend `harness/channels/telegram.md` with group flow diagram.

#### Phase 2 — Observe mode (P1.5, pairs with F5 taint)

7. **`observe_unmentioned`** (per group or global default `false`):
   - Unmentioned messages in allowlisted groups append to session as **observed** rows (new `ingress_kind` or message role `observed`), **no agent dispatch**.
   - Triggered turn (`@bot`, reply, pattern, slash) runs agent with prior observed lines in history.
   - Tag triggered inbound with `[display_name|user_id]` prefix (Hermes).
   - Inject per-turn group safety hint in `prompt.rs`: observed lines are ambient context, not instructions to obey (compose with F5 taint markers).

8. **Retention cap** — `observe_max_messages` per session (default 50) to bound SQLite growth.

#### Phase 3 — Session UX + delivery (P2)

9. **`/activation always|mention`** — Telegram slash (session-scoped, persisted in `sessions` metadata); overrides per-group `require_mention` until reset.

10. **`exclusive_bot_mentions`** — when multiple bots share a group, ignore messages that @mention a different bot (default `true` if low cost).

11. **Optional `visible_replies: automatic | message_tool`** for groups — defer until message tool exists; document as follow-up.

### Out of scope

- New channels (Discord/Slack) — see backlog non-goals.
- Webhook mode for Telegram.
- OpenClaw ambient room events beyond observe transcript (full `room_event` pipeline).
- `contextVisibility: allowlist` hardening (OpenClaw planned feature).
- DM forum topics / `dm_topics` auto-provisioning (Hermes Bot API 9.4) — separate plan if needed.
- Changing default `group_policy` to `open` — stay `allowlist`, opt-in groups.

## Files likely to change

- `crates/bobaclaw-core/src/channels.rs` — config types, defaults, migration helpers
- `crates/bobaclaw-core/src/policy.rs` — group evaluation, pattern match, observe vs dispatch decision
- `crates/bobaclaw-core/src/config.rs` — deserialize + doctor hints
- `crates/bobaclaw-channel-telegram/src/ingress.rs` — mention/command entity detection, sender display
- `crates/bobaclaw-channel-telegram/src/runtime.rs` — observe path (persist only), trigger path
- `crates/bobaclaw-channel-telegram/src/commands.rs` — `/activation`
- `crates/bobaclaw-state/src/session.rs` — observed message rows, session metadata
- `crates/bobaclaw-agent/src/prompt.rs` — group safety hint, sender tags
- `crates/bobaclaw/src/doctor.rs` — group config checks
- `config.example.yaml`
- `harness/channels/telegram.md`
- `docs/as-built.md`
- `migrations/` — if session metadata or message role column needed

## Implementation steps

### Phase 1

1. Extend `TelegramConfig` with `groups: HashMap<String, TelegramGroupConfig>`, `group_allow_from`, `mention_patterns`, `ignored_threads`, `free_response_groups`.
2. Refactor `evaluate_group` → return `GroupDecision::{Allow, Deny, ObserveOnly}` (ObserveOnly unused until Phase 2).
3. Add `message_triggers_bot()` combining mention entity, reply-to-bot, `bot_command` with `@bot`, and regex patterns.
4. Wire runtime: trust check uses new policy; log deny reason at `debug` for operator troubleshooting.
5. Unit tests in `bobaclaw-core` policy module for each gate combination.
6. Update `config.example.yaml` with commented group example; doctor warns when `group_policy: allowlist` and `groups` empty.

### Phase 2

7. Add observed message persistence (role or `ingress_kind`); `SessionStore::append_observed`.
8. Runtime branch: `ObserveOnly` → append + return; `Allow` → normal turn.
9. Prompt: group turn preamble + F5 taint compatibility review.
10. Integration test: unmentioned messages visible after `@bot` trigger in same session.

### Phase 3

11. `/activation` in `handle_slash_command`; store in session row.
12. `exclusive_bot_mentions` in ingress parse (optional).

## Validation

```bash
cargo test -p bobaclaw-core -p bobaclaw-channel-telegram -p bobaclaw-state -p bobaclaw-agent
make ci
```

Manual:

1. Add test group ID to config; verify deny without ID, allow with `@bot`.
2. `observe_unmentioned: true` — chatter without reply; `@bot` question references prior chatter.
3. `group_allow_from` — only listed user can trigger; other members ignored.
4. `mention_patterns: ["^chompy\\b"]` — wake word triggers without `@`.
5. `ignored_threads` — topic silent; general topic still works.

## Risks

- **Prompt injection via observed context** — mitigated by F5 taint + explicit safety preamble; observe mode **off by default**.
- **SQLite growth** — observe cap + compaction policy review.
- **Regex DoS** — use `regex` with size limits; cap pattern count (e.g. 16).
- **Breaking config** — keep `allowed_groups` / `group_require_mention` as deprecated aliases for one release.

## Rollback plan

Revert feature branch; config aliases keep old keys working. Observed messages table/role can remain unused.

## Dependencies

- **F5 (taint)** should land before or with Phase 2 observe mode.
- **F5 operator tagging** overlaps with `[nickname|user_id]` — implement once.
- Independent of F1–F4 ordering for Phase 1.

## Completion notes

(fill after implementation)
