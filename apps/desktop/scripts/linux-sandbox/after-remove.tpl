#!/bin/bash

# rpm runs the old package's %postun after the new one's %post and passes the number
# of instances left; dpkg passes "upgrade". Only a real removal may undo the link and
# AppArmor profile that the new version has just installed.
case "$1" in
    upgrade|failed-upgrade|abort-upgrade|[1-9]*) exit 0 ;;
esac

# Alternatives are keyed by the target path, not by the link.
if type update-alternatives >/dev/null 2>&1; then
    update-alternatives --remove '${executable}' '/opt/${sanitizedProductName}/${executable}' >/dev/null 2>&1 || true
fi
# The ln -sf fallback, or an alternatives tool that never owned the link, leaves it dangling.
if [ -L '/usr/bin/${executable}' ] && [ ! -e '/usr/bin/${executable}' ]; then
    rm -f '/usr/bin/${executable}'
fi

rm -f '/etc/apparmor.d/${executable}'

# rpm does not own the install directories, so remove the empty tree it leaves behind.
if [ -d '/opt/${sanitizedProductName}' ]; then
    find '/opt/${sanitizedProductName}' -depth -type d -empty -delete 2>/dev/null || true
fi
