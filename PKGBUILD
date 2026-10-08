# Maintainer: Ait contributors <https://github.com/ait-app/ait>
# Local working-tree build. Run makepkg -si from this repository's root.
pkgname=ait
pkgver=0.0.23
_ait_version=0.0.23
pkgrel=1
pkgdesc='Local-first multi-agent manager, built from the local source tree'
arch=('x86_64')
url='https://github.com/ait-app/ait'
license=('Apache-2.0')
depends=(
  'alsa-lib' 'at-spi2-core' 'bash' 'cairo' 'coreutils' 'dbus' 'expat'
  'libgcc' 'libstdc++' 'git' 'glib2' 'glibc' 'grep' 'gtk3'
  'libcups' 'libxcb' 'libxcomposite' 'libxdamage' 'libxext'
  'libxfixes' 'libxkbcommon' 'libxrandr' 'libx11' 'mesa' 'nspr' 'nss'
  'pango' 'systemd-libs' 'util-linux' 'xdg-utils'
)
makedepends=('rust' 'nodejs' 'npm' 'python' 'rsync')
optdepends=(
  'libsecret: desktop credential storage'
  'org.freedesktop.secrets: Secret Service provider for credential storage'
)
conflicts=('ait-bin')
# GCC LTO objects in native Rust dependencies cannot be linked by Rust's lld.
options=('!strip' '!debug' '!lto')

# Deliberately override makepkg's in-tree defaults, including its package/log outputs.
# Separate checkouts have separate build trees. Never use pkgver(): makepkg writes
# the resulting version back into the original PKGBUILD.
_ait_checkout="$(dirname "$(realpath "${BASH_SOURCE[0]}")")"
_ait_checkout_key="$(printf '%s' "$_ait_checkout" | sha256sum)"
_ait_cache="$(realpath -m "${AIT_MAKEPKG_DIR:-${XDG_CACHE_HOME:-$HOME/.cache}/ait/makepkg/${_ait_checkout_key:0:16}}")"
case "$_ait_cache/" in
  "$_ait_checkout/"*)
    printf '%s\n' 'AIT_MAKEPKG_DIR must be outside the source checkout.' >&2
    return 1 ;;
esac
BUILDDIR="$_ait_cache/build"
PKGDEST="$_ait_cache/packages"
SRCDEST="$_ait_cache/sources"
SRCPKGDEST="$_ait_cache/source-packages"
LOGDEST="$_ait_cache/logs"
mkdir -p "$BUILDDIR" "$PKGDEST" "$SRCDEST" "$SRCPKGDEST" "$LOGDEST"

prepare() {
  # Read the working tree, including uncommitted/new source, without copying .git,
  # ignored dependencies, credentials, databases or existing build products.
  git -C "$_ait_checkout" ls-files --cached --others --exclude-standard --deduplicate -z \
    > "$srcdir/working-tree-files"
  rm -rf -- "$srcdir/ait"
  mkdir -p "$srcdir/ait"
  rsync -a --from0 --ignore-missing-args --files-from="$srcdir/working-tree-files" \
    "$_ait_checkout/" "$srcdir/ait/"
}

build() {
  cd "$srcdir/ait"
  local -x npm_config_cache="$_ait_cache/npm"
  local -x ELECTRON_CACHE="$_ait_cache/electron"
  local -x ELECTRON_BUILDER_CACHE="$_ait_cache/electron-builder"
  local -x XDG_CACHE_HOME="$_ait_cache/tools"
  local -x TMPDIR="$_ait_cache/tmp"
  local -x CARGO_TARGET_DIR="$_ait_cache/cargo-target"
  local -x AIT_SERVER_BIN="$CARGO_TARGET_DIR/release/daemon"
  local -x CI=1 EXPO_NO_TELEMETRY=1
  mkdir -p "$TMPDIR"

  node scripts/verify-release-version.mjs "v$_ait_version"
  npm ci --no-audit --no-fund
  cargo build --locked --release -p daemon --bin daemon
  npm run build:desktop-assets
  node apps/desktop/scripts/prepare-daemon.mjs
  npm run build:main --workspace=@ait/desktop
  # Only produce the unpacked app: pacman supplies the installer, not AppImage.
  cd apps/desktop
  npm exec -- electron-builder --config electron-builder.yml --linux --x64 --dir --publish never
}

package() {
  local app="$srcdir/ait/apps/desktop/release/linux-unpacked"
  install -dm755 "$pkgdir/usr/lib/ait" "$pkgdir/usr/bin"
  cp -a --no-preserve=ownership "$app/." "$pkgdir/usr/lib/ait/"
  chmod 4755 "$pkgdir/usr/lib/ait/chrome-sandbox"
  ln -s /usr/lib/ait/Ait "$pkgdir/usr/bin/ait"
  install -Dm644 "$srcdir/ait/packaging/aur/ait-bin/ait.desktop" \
    "$pkgdir/usr/share/applications/ait.desktop"
  install -Dm644 "$app/resources/icon.png" "$pkgdir/usr/share/pixmaps/ait.png"
  install -Dm644 "$srcdir/ait/LICENSE" "$pkgdir/usr/share/licenses/ait/LICENSE"
  ln -s /usr/lib/ait/LICENSE.electron.txt "$pkgdir/usr/share/licenses/ait/LICENSE.electron.txt"
  ln -s /usr/lib/ait/LICENSES.chromium.html "$pkgdir/usr/share/licenses/ait/LICENSES.chromium.html"
}
