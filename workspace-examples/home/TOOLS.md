# TOOLS.md — environment notes (optional)

Local facts the agent cannot infer from the workspace alone:

- SSH hosts and aliases
- Device or camera names
- Preferred package managers
- Non-default ports or URLs

Update when you learn something stable. Do not put secrets here.

## Obscura browser (MCP)

Long-lived headless browser for JS pages, clicks, forms — not `exec` + `curl`.

- Install: `make install-obscura-mcp` (pulls image, starts container `bobaclaw-obscura-mcp`)
- On Apple Silicon the script uses `linux/amd64` (Docker emulation); first MCP connect may be slow
- Config: `mcp_servers.obscura` with `docker exec -i bobaclaw-obscura-mcp /obscura mcp` (see `config.example.yaml`)
- Tools: `mcp_obscura_browser_navigate`, `mcp_obscura_browser_snapshot`, …
- Check: `bobaclaw doctor` → `mcp obscura: OK, 12 tool(s)`
- Stop: `make stop-obscura-mcp`
