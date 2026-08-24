---
title: "MCP Servers Overview"
---

# MCP Servers Overview

The [Meeting Assistant](meeting-assistant.md) gains external tools through
**Model Context Protocol (MCP)** servers: register a server once and its
tools appear in the agent's toolset on the next session.

## Registering a server

Settings → **MCP servers**: name, transport, and auth. Servers are stored
locally; secrets go to the OS keychain.

| Transport | Use for |
|---|---|
| Streamable HTTP | remote servers reachable by URL |
| Python package | local packages exposing an MCP entry point |

## Authentication methods

Auth config is stored as one JSON blob per server (secret values in the OS
keychain, referenced from the database row):

```json
{ "authType": "oauth_client_credentials",
  "clientId": "…", "clientSecretRef": "keychain:mcp/deepwiki",
  "tokenUrl": "https://…", "scopes": ["read"],
  "accessToken": null, "refreshToken": null, "expiresAt": null }
```

| `authType` | Fields used | Notes |
|---|---|---|
| `bearer` | token | static token |
| `custom_headers` | headers map | sent verbatim on every request |
| `oauth2` / `oauth_client_credentials` | clientId, secret, grantType, tokenUrl, scopes | tokens refresh just-in-time before expiry; refreshes never require re-registering the server |
| `env_vars` | env map | injected into package-transport servers |

## Server row shape (`mcp_servers` table)

```json
{ "server_id": "deepwiki", "transport": "streamable-http",
  "url_or_package": "https://mcp.example/mcp",
  "status": "ACTIVE", "auth_ref": "keychain:mcp/deepwiki" }
```

## Health and lifecycle

A config fingerprint is cached per server, so unchanged servers skip
reconnection. Failed servers are health-checked and restarted on demand;
a warmup ping keeps connections alive during long sessions. A broken MCP
server degrades to its failed thinking step in the agent timeline — it
never takes the assistant down.
