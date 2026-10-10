#!/usr/bin/env bash
# Linux desktop plugin validation in a disposable container (Docker/OrbStack/Podman).
#
#   tools/plugin-platform/linux-docker.sh [--platform linux/arm64|linux/amd64] [--game-root DIR]
#
# The checkout and optional game root are mounted read-only at their host paths
# (git worktree metadata stays valid). Build output lives in a named volume; the
# summary and reports are copied to target/plugin-platform/linux-<arch>/.
# No game data is copied out: real-data reports contain digests and counts only.
# `test_read_save_item` is skipped: it needs an absent local fixture on every platform.
set -euo pipefail

platform=""
game_root=""
while [ $# -gt 0 ]; do
  case "$1" in
    --platform) platform="$2"; shift 2 ;;
    --game-root) game_root="$(cd "$2" && pwd)"; shift 2 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

repo="$(cd "$(dirname "$0")/../.." && pwd)"
common_git="$(cd "$repo" && cd "$(git rev-parse --git-common-dir)" && pwd)"
# Always pass an explicit platform: a cached base image of another architecture would
# otherwise be reused silently (e.g. under Rosetta/QEMU emulation).
if [ -z "$platform" ]; then platform="linux/$(docker version --format '{{.Server.Arch}}')"; fi
docker_platform=(--platform "$platform")
case "$platform" in
  linux/arm64|linux/aarch64) arch=aarch64 ;;
  linux/amd64|linux/x86_64) arch=x86_64 ;;
  *) echo "unsupported platform: $platform" >&2; exit 2 ;;
esac
image="rfvp-plugin-linux:$arch"
out="$repo/target/plugin-platform/linux-$arch"
mkdir -p "$out"

docker build --pull "${docker_platform[@]}" -t "$image" - <<'EOF'
FROM rust:1-bookworm
RUN apt-get update && apt-get install -y --no-install-recommends \
      pkg-config libasound2-dev libunwind-dev libgstreamer1.0-dev \
      libgstreamer-plugins-base1.0-dev time git python3 \
    && rm -rf /var/lib/apt/lists/* \
    && rustup target add wasm32-unknown-unknown \
    && git config --system --add safe.directory '*'
EOF

mounts=(-v "$repo:$repo:ro" -v "$out:/out")
# A linked worktree's .git file points into the main repository's common dir.
case "$common_git" in "$repo"/*) ;; *) mounts+=(-v "$common_git:$common_git:ro") ;; esac
if [ -n "$game_root" ]; then mounts+=(-v "$game_root:/game:ro"); fi

docker run --rm "${docker_platform[@]}" "${mounts[@]}" \
  -v "rfvp-plugin-target-$arch:/work/target" \
  -v "rfvp-plugin-cargo-$arch:/usr/local/cargo/registry" \
  -e CARGO_TARGET_DIR=/work/target -e REPO="$repo" -e GAME="${game_root:+/game}" \
  -w "$repo" "$image" bash -c '
set -uo pipefail
summary=/out/summary.txt
: > "$summary"
step() {
  local name="$1"; shift
  local log="/out/$name.log"
  local start=$SECONDS
  if "$@" > "$log" 2>&1; then status=PASS; else status=FAIL; fi
  echo "$status $name ($((SECONDS - start)) s)" | tee -a "$summary"
}
{ uname -a; echo "dpkg architecture: $(dpkg --print-architecture)"; rustc -vV; cargo -V; python3 -V; } > /out/environment.txt
rm -rf /work/target/plugin-conformance
step gpu-cli-build cargo build --locked --release -p rfvp --bin rfvp --features sakura-moyu,external-plugins
sha256sum /work/target/release/rfvp >> /out/environment.txt
od -An -j18 -N2 -tx2 /work/target/release/rfvp | sed "s/^/ELF e_machine:/" >> /out/environment.txt
step gpu-library-check cargo check --locked -p rfvp --lib --features sakura-moyu,external-plugins
step software-desktop-check cargo check --locked -p rfvp --no-default-features --features soft-render-desktop,sakura-moyu,external-plugins --bins --lib
step default-build cargo build --locked --release -p rfvp --bin rfvp
step plugin-crate-tests cargo test --locked -p rfvp-plugin-host --features wasmi-runtime -p rfvp-plugin-sdk -p rfvp-plugin-api -p rfvp-plugin-sakura
step preparation-save-tests cargo test --locked -p rfvp --lib --no-default-features --features soft-render-core,no-audio,sakura-moyu,external-plugins -- preparation save --skip test_read_save_item
step gpu-options-tests cargo test --locked -p rfvp --lib --no-default-features --features gpu-render,no-audio,sakura-moyu,external-plugins -- preparation save desktop_options_ffi --skip test_read_save_item
step software-desktop-tests cargo test --locked -p rfvp --lib --no-default-features --features soft-render-desktop,sakura-moyu,external-plugins -- software_launch
step conformance python3 tools/plugin-conformance/run.py
cp /work/target/plugin-conformance/results.json /out/conformance-results.json 2>/dev/null
if [ -n "$GAME" ]; then
  step real-data python3 tools/plugin-real-data/run.py --game-root /game --timing-runs 5
  report=$(ls -d /work/target/plugin-real-data/run-* 2>/dev/null | tail -1)
  [ -n "$report" ] && cp "$report/report.json" /out/real-data-report.json
fi
! grep -q "^FAIL" "$summary"
'
echo "Results: $out/summary.txt"
