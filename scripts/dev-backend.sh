#!/usr/bin/env bash
set -euo pipefail

RESTART=false
if [[ "${1:-}" == "--restart" || "${1:-}" == "-r" ]]; then
  RESTART=true
fi

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
CORE_DIR="$(cd -- "$SCRIPT_DIR/.." && pwd)"
WORKSPACE_DIR="$(cd -- "$CORE_DIR/.." && pwd)"
ENV_PATH="$CORE_DIR/.env"
LOG_DIR="$CORE_DIR/target/dev-logs"

mkdir -p "$LOG_DIR"

addon_repo_url() {
  case "$1" in
    typenx-addon-myanimelist) echo "https://github.com/typenx/typenx-addon-myanimelist.git" ;;
    typenx-addon-anilist) echo "https://github.com/typenx/typenx-addon-anilist.git" ;;
    typenx-addon-kitsu) echo "https://github.com/typenx/typenx-addon-kitsu.git" ;;
    *) return 1 ;;
  esac
}

load_env() {
  if [[ ! -f "$ENV_PATH" ]]; then
    echo "No .env found at $ENV_PATH. Continuing with existing shell environment." >&2
    return
  fi

  set -a
  # shellcheck disable=SC1090
  source "$ENV_PATH"
  set +a
}

find_addon_dir() {
  local name="$1"
  local workspace_path="$WORKSPACE_DIR/$name"
  if [[ -d "$workspace_path" ]]; then
    echo "$workspace_path"
    return 0
  fi

  local user_home="${HOME:-}"
  if [[ -z "$user_home" || ! -d "$user_home" ]]; then
    return 1
  fi

  echo "Searching $user_home for $name..." >&2
  find "$user_home" -type d -name "$name" -print -quit 2>/dev/null
}

ensure_addon_dir() {
  local name="$1"
  local addon_dir
  addon_dir="$(find_addon_dir "$name")"

  if [[ -n "$addon_dir" ]]; then
    echo "Using $name at $addon_dir" >&2
  else
    local repo_url
    repo_url="$(addon_repo_url "$name")"
    addon_dir="$WORKSPACE_DIR/$name"
    echo "$name was not found under the user directory. Cloning $repo_url to $addon_dir..." >&2
    git clone "$repo_url" "$addon_dir"
  fi

  if [[ -f "$addon_dir/package.json" && ! -d "$addon_dir/node_modules" ]]; then
    echo "Installing dependencies for $name..." >&2
    (cd "$addon_dir" && npm install)
  fi

  echo "$addon_dir"
}

stop_port_listener() {
  local port="$1"
  local pids=""

  if command -v lsof >/dev/null 2>&1; then
    pids="$(lsof -tiTCP:"$port" -sTCP:LISTEN 2>/dev/null || true)"
  elif command -v netstat >/dev/null 2>&1; then
    pids="$(
      netstat -ano 2>/dev/null |
        awk -v port=":$port" '$0 ~ /LISTEN/ && index($0, port) { print $NF }' |
        sort -u
    )"
  fi

  for pid in $pids; do
    if [[ -n "$pid" && "$pid" != "0" ]]; then
      echo "Stopping existing process on port $port (PID $pid)"
      kill -9 "$pid" 2>/dev/null || true
    fi
  done
}

start_service() {
  local name="$1"
  local cwd="$2"
  shift 2

  local stdout="$LOG_DIR/$name.log"
  local stderr="$LOG_DIR/$name.err.log"
  rm -f "$stdout" "$stderr"

  echo "Starting $name..."
  (
    cd "$cwd"
    "$@"
  ) >"$stdout" 2>"$stderr" &

  SERVICES+=("$name:$!")
}

cleanup() {
  echo ""
  echo "Stopping Typenx backend stack..."
  for service in "${SERVICES[@]:-}"; do
    local pid="${service##*:}"
    if kill -0 "$pid" 2>/dev/null; then
      kill "$pid" 2>/dev/null || true
    fi
  done

  sleep 0.5
  stop_port_listener 8080
  stop_port_listener 8787
  stop_port_listener 8788
  stop_port_listener 8789
}

load_env

if [[ "$RESTART" == true ]]; then
  stop_port_listener 8080
  stop_port_listener 8787
  stop_port_listener 8788
  stop_port_listener 8789
fi

if [[ -z "${MAL_CLIENT_ID:-}" ]]; then
  echo "MAL_CLIENT_ID is missing. Add it to core/.env before starting the backend stack." >&2
  exit 1
fi

SERVICES=()
trap cleanup EXIT INT TERM

MYANIMELIST_ADDON_DIR="$(ensure_addon_dir "typenx-addon-myanimelist")"
ANILIST_ADDON_DIR="$(ensure_addon_dir "typenx-addon-anilist")"
KITSU_ADDON_DIR="$(ensure_addon_dir "typenx-addon-kitsu")"

PORT=8787 start_service \
  "typenx-addon-myanimelist" \
  "$MYANIMELIST_ADDON_DIR" \
  npm run dev

PORT=8788 start_service \
  "typenx-addon-anilist" \
  "$ANILIST_ADDON_DIR" \
  npm run dev

PORT=8789 start_service \
  "typenx-addon-kitsu" \
  "$KITSU_ADDON_DIR" \
  npm run dev

start_service \
  "typenx-server" \
  "$CORE_DIR" \
  cargo run -p typenx-server

echo ""
echo "Typenx backend stack is starting:"
echo "  Core:        http://127.0.0.1:8080/health"
echo "  MAL addon:   http://127.0.0.1:8787/manifest"
echo "  AniList:     http://127.0.0.1:8788/manifest"
echo "  Kitsu:       http://127.0.0.1:8789/manifest"
echo ""
echo "Logs are in $LOG_DIR"
echo "Press Ctrl+C to stop the backend stack."

while true; do
  for service in "${SERVICES[@]}"; do
    name="${service%%:*}"
    pid="${service##*:}"
    if ! kill -0 "$pid" 2>/dev/null; then
      echo "$name exited. Last stderr:" >&2
      cat "$LOG_DIR/$name.err.log" >&2 || true
      exit 1
    fi
  done

  sleep 2
done
