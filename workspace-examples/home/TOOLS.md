# TOOLS.md — environment notes (optional)

Local facts the agent cannot infer from the workspace alone:

- SSH hosts and aliases
- Device or camera names
- Preferred package managers
- Non-default ports or URLs

Update when you learn something stable. Do not put secrets here.

## Obscura browser (MCP)

Headless browser for JS pages, forms, and scraping. Runs on the **host via Docker** (stdio MCP), not inside bubblewrap.

- Install image: `make install-obscura-mcp` (pulls `h4ckf0r0day/obscura`; Apple Silicon uses `linux/amd64`)
- Config: `mcp_servers.obscura` with `command: docker` / `args: [run, --rm, -i, …]` — see `config.example.yaml`
- Container stays up while `bobaclaw chat` or `gateway` runs; tools are `mcp_obscura_browser_*`
- Verify: `bobaclaw doctor` → `mcp obscura: OK, 12 tool(s)`
- Prefer MCP browser tools over `exec` + `curl` when the page needs JavaScript or interaction
- Optional HTTP (`url: http://127.0.0.1:3000/mcp`): native Obscura binary on the host (`obscura mcp --http`); Docker HTTP is not usable yet (binds loopback inside the image)
