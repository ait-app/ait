#!/bin/sh
# E2E launcher: exec the loopback DSH Host fixture with the real native CLI arguments.
exec node "$(dirname "$0")/fake-dsh-host.mjs" "$@"