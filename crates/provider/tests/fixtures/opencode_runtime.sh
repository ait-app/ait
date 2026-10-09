#!/bin/sh
# Immutable launcher for the Rust-owned HTTP fixture. Per-instance data lives
# beside the launcher symlink so unscoped discovery can use a different cwd.
set -eu
{
    IFS= read -r release
    IFS= read -r address
} < "${0%/*}/runtime.conf"

if [ "${1-}" = '--version' ]; then
    printf '%s\n' "$release"
    exit 0
fi
[ "$*" = 'serve --hostname 127.0.0.1 --port 0' ] || exit 12
case "$release" in
    1.*) [ -n "${OPENCODE_SERVER_PASSWORD-}" ] || exit 13 ;;
    2.*) [ -n "${OPENCODE_PASSWORD-}" ] || exit 13 ;;
    *) exit 14 ;;
esac
printf 'opencode server listening on http://%s\n' "$address"
exec sleep 600
