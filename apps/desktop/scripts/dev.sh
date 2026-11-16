#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../../.." && pwd)"
export PATH="$ROOT_DIR/node_modules/.bin:$PATH"
install-electron
export AIT_SERVER_DATA_DIR="${AIT_SERVER_DATA_DIR:-$ROOT_DIR/.tmp/ait/server}"
export AIT_ELECTRON_USER_DATA_DIR="${AIT_ELECTRON_USER_DATA_DIR:-$ROOT_DIR/.tmp/ait/electron}"
export EXPO_PORT="${EXPO_PORT:-$(get-port 8082 8083 8084 8085 8086)}"
export EXPO_DEV_URL="http://localhost:$EXPO_PORT"
if [ -z "${AITD_BIN:-}" ]; then
  cargo build --manifest-path "$ROOT_DIR/Cargo.toml" --target-dir "$ROOT_DIR/target" -p daemon --bin daemon
  export AITD_BIN="$ROOT_DIR/target/debug/daemon"
fi
npm --prefix "$ROOT_DIR" run build:desktop-main
exec node "$SCRIPT_DIR/dev-runner.mjs" "$@"
