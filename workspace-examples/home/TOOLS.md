# TOOLS.md — environment notes (optional)

Local facts the agent cannot infer from the workspace alone:

- SSH hosts and aliases
- Device or camera names
- Preferred package managers
- Non-default ports or URLs

Update when you learn something stable. Do not put secrets here.

## Sources in answers

When your answer uses facts from the web, MCP, `web_fetch`, or memory recall this turn, end with **## Sources** — markdown links to each URL or workspace path you actually read. Examples:

- Web: `[Example page](https://example.com/doc)`
- Memory: `[MEMORY.md](MEMORY.md)` or `[notes](memory/notes.md)`

Skip Sources only when answering from the current user message or bootstrap files already in context.

## Obscura browser (MCP)

Headless browser for JS pages, forms, and scraping. Runs on the **host via Docker** (stdio MCP), not inside bubblewrap.

- Install image: `make install-obscura-mcp` (pulls `h4ckf0r0day/obscura`; Apple Silicon uses `linux/amd64`)
- Config: `mcp_servers.obscura` with `command: docker` / `args: [run, --rm, -i, …]` — see `config.example.yaml`
- One named container (`bobaclaw-mcp-obscura`) per BobaClaw process; tools are `mcp_obscura_browser_*`
- Cleanup orphans: `make stop-obscura-mcp`
- Verify: `bobaclaw doctor` → `mcp obscura: OK, 12 tool(s)`
- Prefer MCP browser tools over `exec` + `curl` when the page needs JavaScript or interaction
- BobaClaw injects `waitUntil: domcontentloaded` on `browser_navigate` when omitted — `load` never fires on heavy sites (ya.ru) and hangs until MCP timeout
- After browsing for facts, include those URLs in **## Sources** (see above)
- Optional HTTP (`url: http://127.0.0.1:3000/mcp`): native Obscura binary on the host (`obscura mcp --http`); Docker HTTP is not usable yet (binds loopback inside the image)
