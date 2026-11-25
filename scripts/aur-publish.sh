#!/usr/bin/env bash
# Usage: aur-publish.sh build|publish PACKAGE [VERSION]
# PACKAGE: ait-bin, aitd-bin (stable VERSION required), ait-git, aitd-git.
# Build as a non-root user, without AUR credentials; publish never runs prepare/build.
# AUR_WORKDIR retains candidates; publishing pushes only with AUR_PUSH=1.
set -euo pipefail

stage=${1:-}
pkg=${2:-}
version=${3:-}
repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
case "$stage" in
  build|publish) ;;
  *) echo 'Usage: aur-publish.sh build|publish PACKAGE [VERSION]' >&2; exit 2 ;;
esac
case "$pkg" in
  ait-bin|aitd-bin)
    [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "$pkg needs a stable VERSION" >&2; exit 2; } ;;
  ait-git|aitd-git) ;;
  *) echo "Unknown AUR package: $pkg" >&2; exit 2 ;;
esac
work=${AUR_WORKDIR:-$(mktemp -d)}
mkdir -p "$work"
work=$(cd "$work" && pwd)
aur="$work/aur"
candidate="$work/candidate"

srcinfo_field() {
  sed -n "s/^\t$1 = //p" "$2" | head -n 1
}
set_field() {
  sed -i "s/^$1=.*/$1=$2/" "$candidate/PKGBUILD"
}
same_as_aur() {
  diff -rq --exclude=.git --exclude=.SRCINFO "$candidate" "$aur" >/dev/null
}
set_checksums() {
  awk -v a="$1" -v b="$2" '
    /^sha256sums=\(/ { print "sha256sums=("; print "  '\''" a "'\''"; print "  '\''" b "'\''"; skip = 1; next }
    skip && /^\)/ { skip = 0 }
    !skip
  ' "$candidate/PKGBUILD" > "$work/PKGBUILD"
  mv "$work/PKGBUILD" "$candidate/PKGBUILD"
}

rm -rf "$aur" "$candidate"
# A successful clone with no commits is a new package. Authentication/network failures
# must fail here, never become a first publication with missing version history.
if ! git clone --quiet "https://aur.archlinux.org/$pkg.git" "$aur"; then
  echo "Cannot read AUR $pkg; check network and authentication." >&2
  exit 1
fi
aur_ver=
aur_rel=
if [ -f "$aur/.SRCINFO" ]; then
  aur_ver=$(srcinfo_field pkgver "$aur/.SRCINFO")
  aur_rel=$(srcinfo_field pkgrel "$aur/.SRCINFO")
  [[ -n "$aur_ver" && "$aur_rel" =~ ^[1-9][0-9]*$ ]] || { echo 'Invalid AUR .SRCINFO version' >&2; exit 1; }
elif git -C "$aur" rev-parse --verify HEAD >/dev/null 2>&1; then
  echo 'Existing AUR repository is missing .SRCINFO' >&2
  exit 1
fi
echo "AUR $pkg: ${aur_ver:-none}-${aur_rel:-none}"
cp -a "$repo_root/packaging/aur/$pkg/." "$candidate/"

if [[ "$pkg" == aitd-* ]]; then
  cp "$repo_root/packaging/systemd/aitd.service" "$candidate/aitd.service"
  service_sum=$(sha256sum "$candidate/aitd.service" | cut -d' ' -f1)
fi
if [[ "$pkg" == *-bin ]]; then
  if [ -n "$aur_ver" ] && [ "$(vercmp "$version" "$aur_ver")" -lt 0 ]; then
    echo "AUR already has newer $aur_ver; skipping $version."
    exit 0
  fi
  if [ "$pkg" = ait-bin ]; then
    asset=Ait-linux-x86_64.AppImage
    local_sum=$(sha256sum "$candidate/ait.desktop" | cut -d' ' -f1)
    cp "$repo_root/LICENSE" "$candidate/LICENSE"
  else
    asset="Ait-$version-daemon-linux-x64.tar.gz"
    local_sum=$service_sum
  fi
  asset_sum=$(curl --retry 2 --connect-timeout 20 --max-time 120 -fsSL "https://github.com/ait-app/ait/releases/download/v$version/SHA256SUMS" |
    awk -v asset="$asset" '$2 == asset { print $1 }')
  [[ "$asset_sum" =~ ^[0-9a-f]{64}$ ]] || { echo "Missing or invalid $asset checksum in v$version SHA256SUMS" >&2; exit 1; }
  set_checksums "$asset_sum" "$local_sum"
  set_field pkgver "$version"
  set_field pkgrel 1
  if [ "$version" = "$aur_ver" ]; then
    set_field pkgrel "$aur_rel"
    if ! same_as_aur; then set_field pkgrel "$((aur_rel + 1))"; fi
  fi
else
  if [ "$pkg" = aitd-git ]; then set_checksums SKIP "$service_sum"; fi
  # Ignore upstream commits alone: AUR VCS recipes are updated only when the recipe changes.
  if [ -n "$aur_ver" ]; then
    set_field pkgver "$aur_ver"
    set_field pkgrel "$aur_rel"
  fi
fi
if [ "$stage" = publish ] && same_as_aur; then
  echo "AUR $pkg is up to date."
  exit 0
fi

mkdir -p "$work/build" "$work/packages" "$work/sources"
(
  cd "$candidate"
  export BUILDDIR="$work/build" PKGDEST="$work/packages" SRCDEST="$work/sources"
  if [[ "$pkg" == *-git ]]; then
    # Fetch and run the trusted recipe's pkgver(), without npm/Cargo or upstream build code.
    makepkg --nobuild --noprepare --nodeps --cleanbuild
  elif [ "$stage" = publish ]; then
    makepkg --verifysource --nodeps
  fi
)
if [[ "$pkg" == *-git ]]; then
  new_ver=$(sed -n 's/^pkgver=//p' "$candidate/PKGBUILD")
  if [ "$new_ver" = "$aur_ver" ]; then
    if ! same_as_aur; then set_field pkgrel "$((aur_rel + 1))"; fi
  else
    set_field pkgrel 1
  fi
fi
if [ "$stage" = build ]; then
  (
    cd "$candidate"
    BUILDDIR="$work/build" PKGDEST="$work/packages" SRCDEST="$work/sources" \
      makepkg --force --syncdeps --noconfirm --cleanbuild
  )
fi
(cd "$candidate" && makepkg --printsrcinfo > .SRCINFO)
if [ "$stage" = build ]; then
  if command -v namcap >/dev/null; then
    # namcap reports advisory dependency findings; retain them in the build log.
    namcap "$candidate/PKGBUILD" "$work/packages/"*.pkg.tar.* || true
  fi
  echo "Built $pkg. Candidate: $candidate; packages: $work/packages"
  exit 0
fi
find "$aur" -mindepth 1 -maxdepth 1 ! -name .git -exec rm -rf {} +
cp -a "$candidate/." "$aur/"
full_ver="$(srcinfo_field pkgver "$aur/.SRCINFO")-$(srcinfo_field pkgrel "$aur/.SRCINFO")"
git -C "$aur" add -A
if git -C "$aur" diff --cached --quiet; then
  echo "AUR $pkg is up to date."
  exit 0
fi
# Local runs use the maintainer's Git identity; CI supplies its own explicitly.
identity=()
if [ -n "${AUR_GIT_NAME:-}" ]; then identity+=(-c "user.name=$AUR_GIT_NAME"); fi
if [ -n "${AUR_GIT_EMAIL:-}" ]; then identity+=(-c "user.email=$AUR_GIT_EMAIL"); fi
git -C "$aur" "${identity[@]}" commit --quiet -m "Update $pkg to $full_ver" \
  -m "Source: https://github.com/ait-app/ait/tree/${GITHUB_SHA:-main}/packaging/aur/$pkg"
git -C "$aur" --no-pager show --stat --format='%s' HEAD
if [ "${AUR_PUSH:-0}" = 1 ]; then
  git -C "$aur" push "ssh://aur@aur.archlinux.org/$pkg.git" HEAD:master
else
  echo "AUR_PUSH is not 1; not pushing $pkg $full_ver. Candidate: $candidate"
fi
