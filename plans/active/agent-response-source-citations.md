# Agent change plan

## Goal

Require the runtime agent to end user-facing answers with a **Sources** section — markdown links to every URL, page, or workspace file it actually used for factual claims this turn — not only when browsing via Obscura MCP.

## Context

Priority **P1 (answer quality / trust)** — part of the June 2026 reliability/autonomy review roadmap.

Partial baseline already shipped in [`telegram-reply-sources`](../completed/telegram-reply-sources.md): browser MCP answers must list visited URLs in `prompt.rs` `MCP_HINT` and `workspace-examples/home/TOOLS.md`. That rule is prompt-only and scoped to browser tools.

User expectation: any answer grounded in fetched or retrieved content should cite what was read — `web_fetch`, non-browser MCP tools that return URLs, `memory_search` / `memory_read` hits, and watcher/event payloads with links. Without this, Telegram/CLI replies look authoritative but are not auditable.

## Scope

### In scope

- Generalize the **Sources** rule in `prompt.rs` (stable-tier hint, not per-tool prose): when the turn used external or retrieved facts, end the reply with `## Sources` — one markdown link per URL or workspace-relative file path actually read; do not invent links; omit the section only when the answer is purely from the current user message or operator-trusted workspace bootstrap files already in context.
- Update `workspace-examples/home/TOOLS.md` with the same rule and examples (URL + `memory/…` path).
- When [`agent-native-file-web-tools.md`](agent-native-file-web-tools.md) lands `web_fetch`, ensure tool description and prompt hint reinforce citation of fetched URLs.
- When [`agent-recall-run-view-memory-search.md`](agent-recall-run-view-memory-search.md) lands recall tools, cite memory file paths (and session/date in link text when helpful).
- Unit test in `cargo test -p bobaclaw-agent prompt` that the generalized hint mentions Sources and covers non-browser retrieval.
- Optional eval smoke: agent asked a web fact must include at least one Sources link in the final reply.

### Out of scope

- Harness `tools/mcp.md` citation text (runtime owns answer shape; see completed plan).
- Automatic post-processing that appends links from tool-call metadata (prompt discipline first; structured injection is a follow-up if models ignore the rule).
- Citing `run_view` / exec stdout (internal audit artifacts, not user-facing sources unless the user asked for command output).
- Channel-specific formatting beyond what markdown links already give Telegram/CLI.

## Files likely to change

- `crates/bobaclaw-agent/src/prompt.rs` (generalize `MCP_HINT` or add adjacent stable `SOURCES_HINT`)
- `workspace-examples/home/TOOLS.md`
- `evals/` (optional smoke scenario)
- Cross-reference updates in `agent-native-file-web-tools.md`, `agent-recall-run-view-memory-search.md`

## Implementation steps

1. Draft stable English Sources hint covering browser MCP, `web_fetch`, memory recall, and other URL-bearing MCP results.
2. Replace or extend the browser-only `MCP_HINT` citation paragraph; keep prompt-cache stability (one durable section).
3. Mirror the rule in workspace `TOOLS.md` with URL and memory-path examples.
4. Add/extend prompt unit tests.
5. Coordinate with `web_fetch` and recall plans so tool descriptions do not contradict the global rule.
6. Run validation.

## Validation

```bash
make ci
cargo test -p bobaclaw-agent prompt
make eval-smoke
```

Additional checks:

- Manual: ask for a fact from a known URL via browser MCP or `web_fetch` — final reply includes Sources with that URL.
- Manual: ask to recall a fact from `MEMORY.md` — final reply cites the memory file path.

## Risks

- Over-citation noise on trivial turns (mitigate: "only when you used external or retrieved facts").
- Models may still skip Sources; structured tool-metadata injection is deferred follow-up.
- File-path sources are not clickable in all channels; use readable link text.

## Rollback plan

Revert the branch; citation is prompt/workspace-only with no schema or API changes.

## Completion notes

- changed files: `crates/bobaclaw-agent/src/sources.rs`, `tool_loop.rs`, `prompt.rs`, `workspace-examples/home/TOOLS.md`
- validation run: `cargo test -p bobaclaw-agent` (71 passed), `make ci` (pending)
- known gaps: Sources footer only for parent turns; MCP tools without `url` arg not tracked
- follow-up work: extend URL extraction for additional MCP tool shapes if needed
