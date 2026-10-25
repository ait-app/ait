#!/usr/bin/env bash
# Sync an AUR package from packaging/aur/PACKAGE when the recipe or release changed.
# Usage: aur-publish.sh build|publish ait-bin VERSION | aur-publish.sh build|publish ait-git
#   build:   full makepkg as a check; never touches AUR credentials.
#   publish: no build, so no upstream npm/Cargo code runs next to the AUR key; regenerates
#            .SRCINFO, commits and pushes to AUR.
# Run as a non-root user; build needs sudo for makepkg --syncdeps.
set -euo pipefail

stage=$1
pkg=$2
version=${3:-}
repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
recipe="$repo_root/packaging/aur/$pkg"
work=${AUR_WORKDIR:-$(mktemp -d)}
aur="$work/aur"
candidate="$work/candidate"

case "$stage" in
  build|publish) ;;
  *) echo "Unknown stage: $stage" >&2; exit 2 ;;
esac
case "$pkg" in
  ait-bin) [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "ait-bin needs a stable VERSION" >&2; exit 2; } ;;
  ait-git) ;;
  *) echo "Unknown AUR package: $pkg" >&2; exit 2 ;;
esac

srcinfo_field() {
  sed -n "s/^\t$1 = //p" "$2" | head -n 1
}
set_field() {
  sed -i "s/^$1=.*/$1=$2/" "$candidate/PKGBUILD"
}
# Compare the files AUR will receive; .SRCINFO is derived from PKGBUILD.
same_as_aur() {
  diff -rq --exclude=.git --exclude=.SRCINFO "$candidate" "$aur" >/dev/null
}

rm -rf "$aur" "$candidate"
git clone --quiet "https://aur.archlinux.org/$pkg.git" "$aur"
aur_ver=
aur_rel=
if [ -f "$aur/.SRCINFO" ]; then
  aur_ver=$(srcinfo_field pkgver "$aur/.SRCINFO")
  aur_rel=$(srcinfo_field pkgrel "$aur/.SRCINFO")
fi
echo "AUR $pkg: ${aur_ver:-none}-${aur_rel:-none}"

cp -a "$recipe/." "$candidate/"

if [ "$pkg" = ait-bin ]; then
  if [ -n "$aur_ver" ] && [ "$(vercmp "$version" "$aur_ver")" -lt 0 ]; then
    echo "AUR already has newer $aur_ver; skipping $version."
    exit 0
  fi
  cp "$repo_root/LICENSE" "$candidate/LICENSE"
  appimage_sum=$(curl -fsSL "https://github.com/ait-app/ait/releases/download/v$version/SHA256SUMS" |
    awk '$2 == "Ait-linux-x86_64.AppImage" { print $1 }')
  [[ "$appimage_sum" =~ ^[0-9a-f]{64}$ ]] || { echo "No AppImage checksum in v$version SHA256SUMS" >&2; exit 1; }
  desktop_sum=$(sha256sum "$candidate/ait.desktop" | cut -d' ' -f1)
  awk -v a="$appimage_sum" -v d="$desktop_sum" '
    /^sha256sums=\(/ { print; print "  '\''" a "'\''"; print "  '\''" d "'\''"; skip = 1; next }
    skip && /^\)/ { skip = 0 }
    !skip
  ' "$candidate/PKGBUILD" > "$work/PKGBUILD" && mv "$work/PKGBUILD" "$candidate/PKGBUILD"
  set_field pkgver "$version"
  if [ "$version" = "$aur_ver" ]; then
    set_field pkgrel "$aur_rel"
    if same_as_aur; then
      echo "AUR $pkg is up to date."
      exit 0
    fi
    set_field pkgrel "$((aur_rel + 1))"
  else
    set_field pkgrel 1
  fi
else
  # pkgver() derives the version from the checkout; only recipe changes need an AUR update.
  if [ -n "$aur_ver" ]; then
    set_field pkgver "$aur_ver"
    set_field pkgrel "$aur_rel"
    if same_as_aur; then
      echo "AUR $pkg is up to date."
      exit 0
    fi
  fi
fi

mkdir -p "$work/build" "$work/packages" "$work/sources"
if [ "$stage" = build ]; then
  (
    cd "$candidate"
    BUILDDIR="$work/build" PKGDEST="$work/packages" SRCDEST="$work/sources" \
      makepkg --syncdeps --noconfirm --cleanbuild
  )
  if command -v namcap >/dev/null; then
    namcap "$candidate/PKGBUILD" "$work/packages/"*.pkg.tar.* || true
  fi
  echo "Built $pkg; publish runs separately."
  exit 0
fi

if [ "$pkg" = ait-git ]; then
  # Only fetch the source and run pkgver(), which reads the source date and hash; prepare() runs npm ci.
  (
    cd "$candidate"
    BUILDDIR="$work/build" SRCDEST="$work/sources" \
      makepkg --nobuild --noprepare --nodeps --cleanbuild
  )
  # makepkg wrote the pkgver() result back and reset pkgrel to 1.
  new_ver=$(sed -n 's/^pkgver=//p' "$candidate/PKGBUILD")
  if [ "$new_ver" = "$aur_ver" ]; then
    set_field pkgrel "$((aur_rel + 1))"
  fi
fi
(cd "$candidate" && makepkg --printsrcinfo > .SRCINFO)

find "$aur" -mindepth 1 -maxdepth 1 ! -name .git -exec rm -rf {} +
cp -a "$candidate/." "$aur/"
full_ver="$(srcinfo_field pkgver "$aur/.SRCINFO")-$(srcinfo_field pkgrel "$aur/.SRCINFO")"
git -C "$aur" add -A
git -C "$aur" \
  -c user.name="${AUR_GIT_NAME:-Ait Release Bot}" \
  -c user.email="${AUR_GIT_EMAIL:-41898282+github-actions[bot]@users.noreply.github.com}" \
  commit --quiet -m "Update $pkg to $full_ver" \
  -m "Source: https://github.com/ait-app/ait/tree/${GITHUB_SHA:-main}/packaging/aur/$pkg"
git -C "$aur" show --stat --format='%s' HEAD

if [ "${AUR_PUSH:-0}" = 1 ]; then
  git -C "$aur" push "ssh://aur@aur.archlinux.org/$pkg.git" HEAD:master
else
  echo "AUR_PUSH is not 1; not pushing $pkg $full_ver."
fi
