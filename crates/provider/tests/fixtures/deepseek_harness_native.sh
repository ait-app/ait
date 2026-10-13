#!/bin/sh
set -eu

case "$*" in
  *--dump-config*)
    if [ -n "${AIT_DSH_FIXTURE_CONFIG:-}" ]; then
      cat "$AIT_DSH_FIXTURE_CONFIG"
      exit 0
    fi
    cat <<'CONFIG'
- id: llm-deepseek
  name: '@deepseek-ai/dsh-llm-deepseek-api-key'
- id: credentials
  name: '@deepseek-ai/dsh-credentials-local'
CONFIG
    exit 0
    ;;
esac

# Immutable launcher; each test owns its Host and passes its port only to this child.
printf 'dsh web: http://127.0.0.1:%s/?token=fixture\n' "${AIT_DSH_FIXTURE_PORT:?fixture port is required}"
exec sleep 120
