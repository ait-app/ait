#!/usr/bin/env bash
# Smoke-tests a running web client image, for example:
#   bash apps/mobile/web-console/check-routes.sh http://127.0.0.1:8080
set -euo pipefail

base=${1:?usage: check-routes.sh <base-url>}
body=$(mktemp)
headers=$(mktemp)
trap 'rm -f "$body" "$headers"' EXIT

# fetch <path> <expected status>; leaves the response in $body and $headers.
fetch() {
  local status
  status=$(curl -sS --retry 15 --retry-all-errors --retry-delay 1 \
    -o "$body" -D "$headers" -w '%{http_code}' "$base$1")
  [[ $status == "$2" ]] || {
    echo "GET $1: expected $2, got $status" >&2
    exit 1
  }
}
expect_header() {
  grep -qi "^$1: .*$2" "$headers" || {
    echo "GET $3: missing '$1: $2'" >&2
    cat "$headers" >&2
    exit 1
  }
}

fetch /healthz 200
[[ $(cat "$body") == ok ]] || { echo "GET /healthz: unexpected body" >&2; exit 1; }

fetch / 200
expect_header Content-Type text/html /
expect_header Cache-Control no-cache /
expect_header Referrer-Policy no-referrer /
shell=$(cat "$body")
asset=$(grep -o '/_expo/static/[^"]*\.js' "$body" | head -n 1)
[[ -n $asset ]] || { echo "GET /: no /_expo/static script" >&2; exit 1; }

# Client routes are deep links into the single-page export.
for route in /settings /settings/general /auth/callback /h/example/workspace/main; do
  fetch "$route" 200
  [[ $(cat "$body") == "$shell" ]] || { echo "GET $route: not the app shell" >&2; exit 1; }
done

fetch "$asset" 200
expect_header Content-Type javascript "$asset"
expect_header Cache-Control immutable "$asset"
expect_header Referrer-Policy no-referrer "$asset"
curl -sS -o /dev/null -D "$headers" -H 'Accept-Encoding: gzip' "$base$asset"
expect_header Content-Encoding gzip "$asset"
fetch /_expo/static/js/web/missing.js 404

fetch /api/v1/me 404
fetch /client-build.json 200
grep -q '"commit"' "$body" || { echo "GET /client-build.json: no commit" >&2; exit 1; }

echo "Web client routes OK at $base"
