#!/usr/bin/env bash
# Build the Linux daemon and native ACP bridge on the shared glibc 2.28+ baseline.
# Usage: build-linux-daemon.sh PACKAGE BINARY RUST_TARGET RUST_TOOLCHAIN
set -euo pipefail

package=$1
binary=$2
target=$3
toolchain=$4
image=${AIT_MANYLINUX_IMAGE:-quay.io/pypa/manylinux_2_28_x86_64}
max_glibc=2.28
# manylinux_2_28 links newer libstdc++ symbols statically; the rest comes from GCC 8.
max_glibcxx=3.4.25

docker run --rm \
  -v "$PWD:/src" -w /src \
  -e PACKAGE="$package" -e BINARY="$binary" -e TARGET="$target" -e RUSTUP_TOOLCHAIN="$toolchain" \
  -e CARGO_TERM_COLOR -e AIT_BUILD_VERSION -e RELEASE_TAG \
  "$image" bash -euo pipefail -c '
    curl --proto "=https" --tlsv1.2 -sSf https://sh.rustup.rs |
      sh -s -- -y --no-modify-path --profile minimal --default-toolchain "$RUSTUP_TOOLCHAIN" --target "$TARGET"
    . "$HOME/.cargo/env"
    bridge_args=()
    if [ -f bins/omp-acp/Cargo.toml ]; then
      bridge_args=(-p ait-omp-acp --bin ait-omp-acp)
    fi
    cargo build --locked --release -p "$PACKAGE" --bin "$BINARY" --target "$TARGET" "${bridge_args[@]}"
    chown -R "$(stat -c %u:%g /src)" /src/target
  '

highest() {
  objdump -T "$output" | { grep -o "$1_[0-9][0-9.]*" || true; } | sed "s/^$1_//" | sort -uV | tail -n 1
}
within() {
  [ "$(printf '%s\n%s\n' "$1" "$2" | sort -V | tail -n 1)" = "$2" ]
}
binaries=("$binary")
# Historical source tags predate the standalone bridge; never infer this from build output.
if [ -f bins/omp-acp/Cargo.toml ]; then binaries+=(ait-omp-acp); fi
for executable in "${binaries[@]}"; do
  output="target/$target/release/$executable"
  glibc=$(highest GLIBC)
  glibcxx=$(highest GLIBCXX)
  echo "$output requires GLIBC_$glibc and GLIBCXX_${glibcxx:-none}"
  within "$glibc" "$max_glibc" || { echo "GLIBC_$glibc exceeds the $max_glibc baseline" >&2; exit 1; }
  if [ -n "$glibcxx" ]; then
    within "$glibcxx" "$max_glibcxx" || { echo "GLIBCXX_$glibcxx exceeds $max_glibcxx" >&2; exit 1; }
  fi
done
