#!/usr/bin/env bash
# Zaparoo Frontend
# Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
# SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
#
# Runs a command inside the pinned toolchain image (Dockerfile.toolchain), with
# the repository mounted at /src and the working directory mapped to the same
# place inside it. The justfile's lint, test, MiSTer, portable, translation,
# and notices recipes go through here, so Linux, macOS, and CI build alike.
#
#   scripts/toolchain.sh just _lint
#   scripts/toolchain.sh cargo tree -p frontend
#
# Image selection, first match wins:
#   ZAPAROO_TOOLCHAIN_IMAGE=<ref>  use that image (CI passes the tag it resolved)
#   USE_LOCAL_TOOLCHAIN=1          build Dockerfile.toolchain locally and use it
#                                  (ZAPAROO_TOOLCHAIN_REBUILD=1 rebuilds it)
#   (default)                      pull the published image for
#                                  scripts/toolchain/VERSION, or build it locally
#                                  when it cannot be pulled
#
# Container builds use their own target directory (rust/target/docker) and
# cargo cache (rust/target/docker-cargo), so they never invalidate a host
# `just build`. The image is linux/amd64; on Apple Silicon, Docker Desktop
# runs it under emulation (enable Rosetta in its settings for speed).
set -euo pipefail

# Already inside the toolchain container: run the command as is.
if [[ -n "${ZAPAROO_TOOLCHAIN:-}" ]]; then
    exec "$@"
fi

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
repo_root="$(cd -- "$script_dir/.." && pwd -P)"

error() {
    echo "Error: $*" >&2
    exit 1
}

if [[ $# -eq 0 ]]; then
    echo "Usage: scripts/toolchain.sh <command> [args...]" >&2
    exit 2
fi

if ! command -v docker > /dev/null 2>&1; then
    error "Docker is required for this recipe (Docker Engine on Linux, Docker Desktop on macOS)."
fi

version="$(tr -d '[:space:]' < "$repo_root/scripts/toolchain/VERSION")"
if ! printf '%s' "$version" | grep -Eq '^[A-Za-z0-9_][A-Za-z0-9_.-]{0,127}$'; then
    error "scripts/toolchain/VERSION is not a valid Docker tag: '$version'"
fi
official_image="ghcr.io/zaparooproject/zaparoo-frontend-toolchain:${version}"
local_image="zaparoo-frontend-toolchain:${version}"
platform="${DOCKER_PLATFORM:-linux/amd64}"

build_local() {
    echo "Building toolchain image ${local_image} (one time per VERSION; several minutes)" >&2
    docker build --platform "$platform" --tag "$local_image" \
        --file "$repo_root/Dockerfile.toolchain" "$repo_root/scripts/toolchain" >&2
}

if [[ -n "${ZAPAROO_TOOLCHAIN_IMAGE:-}" ]]; then
    image="$ZAPAROO_TOOLCHAIN_IMAGE"
elif [[ "${USE_LOCAL_TOOLCHAIN:-0}" == "1" ]]; then
    image="$local_image"
    if [[ "${ZAPAROO_TOOLCHAIN_REBUILD:-0}" == "1" ]] \
        || ! docker image inspect "$image" > /dev/null 2>&1; then
        build_local
    fi
else
    image="$official_image"
    if ! docker image inspect "$image" > /dev/null 2>&1 \
        && ! docker pull --platform "$platform" "$image" >&2; then
        echo "Could not pull ${image}; using a local build instead." >&2
        image="$local_image"
        docker image inspect "$image" > /dev/null 2>&1 || build_local
    fi
fi

here="$(pwd -P)"
case "$here/" in
    "$repo_root"/*) workdir="/src${here#"$repo_root"}" ;;
    *) error "run this from inside the repository ($repo_root)" ;;
esac

mkdir -p "$repo_root/rust/target/docker" "$repo_root/rust/target/docker-cargo"

args=(
    run --rm --init
    --platform "$platform"
    --user "$(id -u):$(id -g)"
    --volume "$repo_root:/src"
    --workdir "$workdir"
    --env HOME=/tmp
    --env CARGO_HOME=/src/rust/target/docker-cargo
    --env CARGO_TARGET_DIR=/src/rust/target/docker
    # sccache lives on the host, not in the image.
    --env RUSTC_WRAPPER=
)
[[ -t 0 ]] && args+=(--interactive)
[[ -t 1 ]] && args+=(--tty)

# Settings the recipes and CI pass in. Build provenance comes from
# `just release`; the rest shape cargo and nextest output.
for name in \
    CI GITHUB_ACTIONS CARGO_TERM_COLOR CARGO_INCREMENTAL CARGO_PROFILE_TEST_DEBUG \
    RUSTFLAGS NEXTEST_PROFILE \
    ZAPAROO_OFFICIAL_BUILD ZAPAROO_BUILD_COMMIT ZAPAROO_BUILD_DATE; do
    if [[ -n "${!name+set}" ]]; then
        args+=(--env "$name")
    fi
done

exec docker "${args[@]}" "$image" "$@"
