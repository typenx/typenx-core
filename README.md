# Typenx Core

The Rust backend for Typenx — a self-hostable anime hub that pulls tracking, metadata, your own media, and a real recommender into one open platform.

Typenx Core is the engine: an Axum API, OAuth flows for AniList and MyAnimeList, an addon protocol that lets any HTTP service plug in as a metadata or video source, multi-database storage, and a recommendation model trained on your own watch history. There is no SaaS layer above it. Run it on a homelab, a VPS, or a workstation, and it stays yours.

## What's in the box

- **Axum REST API and OpenAPI schema** — every route is described in the generated spec at `/openapi.json`.
- **Provider OAuth and list sync** — AniList and MyAnimeList sign-in, with periodic syncs of scores, statuses, and progress.
- **Addon orchestration** — register any number of remote HTTP addons; Typenx Core fans out catalog, search, metadata, and video-source requests to them.
- **Recommendations** — an explainable hybrid recommender out of the box, plus a training pipeline for an implicit-feedback matrix-factorization model.
- **Pluggable storage** — SQLite, Postgres, MySQL, and MongoDB through the same repository boundary.

## Workspace

- `typenx-core` — domain types, auth models, provider sync contracts, and addon client logic.
- `typenx-server` — the Axum REST server with OpenAPI output.
- `typenx-storage` — repository traits plus SQLite, Postgres, MySQL, and MongoDB adapters.
- `typenx-addon-sdk-schema` — shared addon protocol types consumed by the TypeScript, Python, and Rust SDKs.

## V1 boundaries

Typenx Core does not host anime files. Remote addons return metadata, catalogs, search results, and episode metadata. Addons that explicitly opt into the `video_sources` resource can also return episode stream URLs they control — that is the only path through which playback URLs enter the system.

## Quick start

```powershell
cargo run -p typenx-server
```

Copy `.env.example` to `.env` and fill in the OAuth credentials you want to use (both AniList and MyAnimeList are optional for boot, required for sign-in).

To bring up the full local backend stack — Typenx Core plus the official metadata and media addons — run:

```powershell
.\scripts\dev-backend.ps1 -Restart
```

Or from Bash:

```bash
./scripts/dev-backend.sh --restart
```

The script loads `core/.env` and starts:

- Typenx Core on `http://127.0.0.1:8080`
- MyAnimeList addon on `http://127.0.0.1:8787`
- AniList addon on `http://127.0.0.1:8788`
- Kitsu addon on `http://127.0.0.1:8789`
- Video Library addon on `http://127.0.0.1:8791`
- NXVideo addon on `http://127.0.0.1:8792`
- Plex addon on `http://127.0.0.1:8793`
- Jellyfin addon on `http://127.0.0.1:8794`

Use `Ctrl+C` in that PowerShell window to stop the stack.

## Storage

Typenx supports SQLite, Postgres, MySQL, and MongoDB through a single repository boundary. Switch adapters with `TYPENX_DATABASE_URL`:

```env
TYPENX_DATABASE_URL=sqlite://typenx.sqlite?mode=rwc
TYPENX_DATABASE_URL=postgres://typenx:typenx@127.0.0.1:5432/typenx
TYPENX_DATABASE_URL=mysql://typenx:typenx@127.0.0.1:3306/typenx
TYPENX_DATABASE_URL=mongodb://127.0.0.1:27017/typenx
```

SQLite is the default. Nothing else in the stack needs to change when you switch.

## Recommendations

Typenx owns recommendations centrally. AniList, MyAnimeList, and Kitsu are treated as imported signal and metadata providers, not as the recommendation brain. The rationale: provider recommenders are tied to provider populations and provider product goals; users who watch widely across sources end up with weaker signal everywhere than if they consolidated.

`POST /me/recommendations` reads the signed-in user's synced library and watch progress, then builds a taste profile from:

- Provider list scores and statuses from AniList and MyAnimeList sync.
- Dropped, paused, completed, watching, and planning states.
- Watch progress and episode completion captured inside Typenx.
- Addon metadata: genres, tags, studios, source, media type, score, era.

The first model is an explainable hybrid recommender. It builds weighted positive and negative feature vectors, gathers candidates from enabled metadata addons, enriches the top candidates with metadata, ranks them, and applies diversity pressure so the feed doesn't collapse into repetitive clones.

Request:

```json
{
  "addon_id": "optional-addon-uuid",
  "limit": 24,
  "candidate_limit": 120,
  "include_reasons": true
}
```

### Roadmap

1. Persist recommendation impressions, clicks, hides, completions, rewatches, and dwell time as training events.
2. Train an implicit-feedback candidate generator using matrix factorization over Typenx user-item interactions.
3. Blend collaborative candidates with the current content model for cold-start coverage and explanation quality.
4. Add contextual bandit exploration so the model learns without overfitting users into a narrow bubble.
5. Evaluate against retention, completion rate, dislike avoidance, novelty, diversity, and calibration.

### Local GPU smoke test

On Windows AMD GPUs, Typenx uses PyTorch DirectML for local recommender experiments:

```powershell
python -m venv .venv-ml
.\.venv-ml\Scripts\python.exe -m pip install -r scripts\requirements-ml.txt
.\.venv-ml\Scripts\python.exe scripts\recommendation_gpu_smoke.py
```

The script trains a compact implicit-feedback matrix-factorization model and reports the active backend. Pass `--cpu` to compare CPU behavior.

### Production GPU training

Train the artifact consumed by `POST /me/recommendations`:

```powershell
.\.venv-ml\Scripts\python.exe scripts\train_recommendation_model.py --database typenx.sqlite --output recommendations.model.json
```

Then point the server at it:

```env
TYPENX_RECOMMENDER_MODEL_PATH=recommendations.model.json
```

When the artifact contains the signed-in user, Typenx serves the GPU-trained recommendations directly. If the user isn't in the artifact yet, the server falls back to the live hybrid ranker — there is always a result.

## Useful endpoints

- `GET /health` — liveness check.
- `GET /openapi.json` — the full API spec, regenerated at build time.
- `POST /me/recommendations` — recommender entry point.
- `GET /me/library` — the user's synced library.

## Build an addon

Addons are HTTP services that speak a typed protocol. Pick a language:

- [TypeScript SDK](https://github.com/typenx/typenx-addon-TS-sdk)
- [Python SDK](https://github.com/typenx/typenx-addon-python-sdk)
- [Rust SDK](https://github.com/typenx/typenx-addon-rust-sdk)

Register the addon URL with `TYPENX_DEFAULT_ADDONS` (deletable) or `TYPENX_BUILTIN_ADDONS` (always-on). The official addons live next door in the [typenx org](https://github.com/typenx).
