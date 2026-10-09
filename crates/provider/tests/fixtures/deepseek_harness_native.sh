#!/bin/sh
set -eu

# Immutable launcher; each test owns its Host and passes its port only to this child.
printf 'dsh web: http://127.0.0.1:%s/?token=fixture\n' "${AIT_DSH_FIXTURE_PORT:?fixture port is required}"
exec sleep 120
