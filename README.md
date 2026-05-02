# Typenx Core

Typenx Core is the Rust backend workspace for a self-hostable anime discovery and library platform.
It is inspired by Stremio's reusable core architecture, but Typenx focuses on anime, MAL/AniList
identity linking, list/progress sync, and remote metadata addons.

## Workspace

- `typenx-core`: domain types, auth models, provider sync contracts, and addon client logic.
- `typenx-server`: Axum REST API server with OpenAPI output.
- `typenx-storage`: repository traits plus SQLite/Postgres/MySQL and MongoDB storage.
- `typenx-addon-sdk-schema`: shared addon protocol types for future TS, Python, and Rust SDKs.

## V1 boundaries

Typenx Core does not host anime files. Remote addons can return metadata, catalogs, search
results, episode metadata, and, when they explicitly opt into `video_sources`, episode stream
URLs controlled by the addon operator.

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
- Kitsu addon on `http://127.0.0.1:8789`
- Video library addon on `http://127.0.0.1:8791`
- NXVideo addon on `http://127.0.0.1:8792`

Use `Ctrl+C` in that PowerShell window to stop the stack.

## Storage

Typenx supports SQLite, Postgres, MySQL, and MongoDB through the same repository
boundary. Configure the active adapter with `TYPENX_DATABASE_URL`:

```env
TYPENX_DATABASE_URL=sqlite://typenx.sqlite?mode=rwc
TYPENX_DATABASE_URL=postgres://typenx:typenx@127.0.0.1:5432/typenx
TYPENX_DATABASE_URL=mysql://typenx:typenx@127.0.0.1:3306/typenx
TYPENX_DATABASE_URL=mongodb://127.0.0.1:27017/typenx
```
