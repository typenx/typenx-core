# Typenx Core

Typenx Core is the Rust backend workspace for a self-hostable anime discovery and library platform.
It is inspired by Stremio's reusable core architecture, but Typenx focuses on anime, MAL/AniList
identity linking, list/progress sync, and remote metadata addons.

## Workspace

- `typenx-core`: domain types, auth models, provider sync contracts, and addon client logic.
- `typenx-server`: Axum REST API server with OpenAPI output.
- `typenx-storage`: repository traits plus SQLx-backed SQLite/Postgres/MySQL storage.
- `typenx-addon-sdk-schema`: shared addon protocol types for future TS, Python, and Rust SDKs.

## V1 boundaries

Typenx Core does not host anime files and does not return direct playback URLs. Remote addons return
metadata, catalogs, search results, and episode metadata only.

## Quick start

```powershell
cargo run -p typenx-server
```

For local configuration, copy `.env.example` to `.env` and fill in the OAuth credentials.

To run the full local backend stack, including the official MAL and AniList addon services:

```powershell
.\scripts\dev-backend.ps1 -Restart
```

Or from Bash/Git Bash:

```bash
./scripts/dev-backend.sh --restart
```

This loads `core\.env`, starts:

- Typenx Core on `http://127.0.0.1:8080`
- MyAnimeList addon on `http://127.0.0.1:8787`
- AniList addon on `http://127.0.0.1:8788`

Use `Ctrl+C` in that PowerShell window to stop the stack.
