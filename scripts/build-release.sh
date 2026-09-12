#!/usr/bin/env bash
# Release build. The guest is built in Docker so the image ID reproduces on any machine.
#
# A plain `cargo build` embeds absolute paths in the ELF, so the image ID changes with
# the checkout location. Published receipts and manifests must come from here.
#
# ## Why a working copy is made
#
# risc0-build invokes `docker build -f <temp>/Dockerfile <repository root>`. BuildKit
# prefers the `Dockerfile.dockerignore` sitting next to the Dockerfile, so **a
# .dockerignore in the repository has no effect**. The one risc0 writes excludes only
# `.git`, `target`, `tmp` and `node_modules`, which leaves `out/` (several GB) and
# `examples/*/.lake/` (7.6 GB with Mathlib) in the build context. Measured, one build
# took 40 GB and a few filled the disk with 116 GB of cache.
#
# Simply pointing `root_dir` elsewhere does not work: risc0 builds relative paths by
# strip_prefix-ing `root_dir` from the guest manifest's real path, so **root_dir must
# be a genuine ancestor**. Hence a source-only working copy, built in place.
#
# Paths inside the container (`/src/zkvm/methods/...`) keep the same relative layout,
# so **the image ID is unchanged**.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
export PATH="$HOME/.risc0/bin:$HOME/.cargo/bin:$PATH"
STAGE="${LEAN_ZKVM_STAGE:-${TMPDIR:-/tmp}/lean-zkvm-release}"

docker info >/dev/null 2>&1 || { echo "the Docker daemon is not running"; exit 1; }

echo "==> creating a source-only working copy: $STAGE"
mkdir -p "$STAGE"
rsync -a --delete \
      --exclude='/out/' --exclude='/vendor/' --exclude='/demo/' --exclude='/slides/' \
      --exclude='.git/' --exclude='.lake/' --exclude='target/' --exclude='target-cpu/' \
      "$ROOT/" "$STAGE/"
printf "    context: %s\n" "$(du -sh "$STAGE" | cut -f1)"

echo "==> building the guest in Docker"
LEAN_ZKVM_DOCKER=1 cargo build --release --manifest-path "$STAGE/zkvm/Cargo.toml" "$@"

mkdir -p "$ROOT/out"
"$STAGE/zkvm/target/release/manifest" "$ROOT/out/manifest.json"
echo
echo "A verifier can take this manifest.json, build from the same commit with"
echo "  scripts/build-release.sh"
echo "and confirm the image IDs match."
echo
echo "If this machine also proves, pass --features cuda (the default is the CPU prover):"
echo "  scripts/build-release.sh --features cuda"
