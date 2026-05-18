# ecky-mcp pi extension

Project-local pi extension that bridges pi to Ecky's MCP HTTP server.

## What it does

- Connects to Ecky MCP (`http://127.0.0.1:39249/mcp` by default)
- Discovers remote MCP tools (`tools/list`)
- Registers local pi tools named like `ecky_mcp_<tool>`
- Forwards calls to Ecky MCP `tools/call`

## Commands

- `/ecky-mcp-connect [url]` — connect/discover/register tools (optional endpoint override)
- `/ecky-mcp-status` — show endpoint/session/tool mapping
- `/ecky-mcp-health` — run `health_check` via Ecky MCP and print response

## Notes

- Ecky must be running with MCP server enabled.
- In Ecky settings, **Connection Type must be `MCP`**, otherwise external MCP tool calls are blocked by Ecky.
- Optional env override:
  - `ECKY_MCP_URL=http://127.0.0.1:39249/mcp`
