#!/bin/bash
# Builds rustock-cli and its maintenance tools for one Linux architecture in
# Docker and writes
#   dist/rustock-<version>-linux-<arch>
#   dist/rustock-tools-<version>-linux-<arch>.tar.gz
# each with a .sha256 file.
#
# Usage: scripts/build-binary.sh <amd64|arm64>
#        FEATURES=smtp scripts/build-binary.sh amd64
#        TOOLS="rewind_to block_info" scripts/build-binary.sh amd64
#
# <version> is <crate version>-<short commit>, plus -dirty when the working
# tree has uncommitted changes. A non-native architecture builds under
# emulation, which is slow.

set -euo pipefail

fail() {
    echo "error: $*" >&2
    exit 1
}

for tool in docker git awk tar; do
    command -v "$tool" >/dev/null 2>&1 || fail "missing required tool: $tool"
done
docker buildx version >/dev/null 2>&1 || fail "docker buildx is required"

if command -v sha256sum >/dev/null 2>&1; then
    sha256_of() { sha256sum "$1" | awk '{print $1}'; }
elif command -v shasum >/dev/null 2>&1; then
    sha256_of() { shasum -a 256 "$1" | awk '{print $1}'; }
else
    fail "missing required tool: sha256sum or shasum"
fi

[ "$#" -eq 1 ] || fail "usage: $0 <amd64|arm64>"
arch="$1"
case "$arch" in
    amd64|arm64) ;;
    *) fail "unsupported architecture '$arch' (expected amd64 or arm64)" ;;
esac

repo="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo"

crate_version="$(awk -F'"' '/^version *=/ {print $2; exit}' crates/cli/Cargo.toml)"
[ -n "$crate_version" ] || fail "cannot read the version from crates/cli/Cargo.toml"
# One source of truth for the compiler: rust-toolchain.toml is what a local
# `cargo build` obeys, so the container is told to use the same one rather than
# relying on the Dockerfile's ARG default staying in step by hand.
rust_version="$(sed -n 's/^[[:space:]]*channel[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' rust-toolchain.toml)"
[ -n "$rust_version" ] || fail "cannot read the [toolchain] channel from rust-toolchain.toml"

commit="$(git rev-parse --short HEAD)"
version="${crate_version}-${commit}"
if [ -n "$(git status --porcelain)" ]; then
    version="${version}-dirty"
fi

name="rustock-${version}-linux-${arch}"
out="dist/${name}"
mkdir -p dist
staging="$(mktemp -d "$repo/dist/.build.XXXXXX")"
trap 'rm -rf "$staging"' EXIT

echo "building $name"
docker buildx build \
    --platform "linux/${arch}" \
    --file docker/build.Dockerfile \
    --target binary \
    --build-arg "RUST_VERSION=${rust_version}" \
    --build-arg "FEATURES=${FEATURES:-}" \
    ${TOOLS:+--build-arg "TOOLS=${TOOLS}"} \
    --output "type=local,dest=${staging}" \
    .

[ -f "$staging/rustock-cli" ] || fail "the build produced no rustock-cli"

tools_name="rustock-tools-${version}-linux-${arch}.tar.gz"
docker buildx build \
    --platform "linux/${arch}" \
    --file docker/build.Dockerfile \
    --target tools \
    --build-arg "RUST_VERSION=${rust_version}" \
    --build-arg "FEATURES=${FEATURES:-}" \
    ${TOOLS:+--build-arg "TOOLS=${TOOLS}"} \
    --output "type=local,dest=${staging}/tools" \
    .

[ -n "$(ls -A "$staging/tools")" ] || fail "the build produced no tools"
tar -C "$staging/tools" -czf "$staging/$tools_name" .

chmod 755 "$staging/rustock-cli"
mv "$staging/rustock-cli" "$out"
mv "$staging/$tools_name" "dist/$tools_name"

write_sha() {
    local file="$1" name hash
    name="$(basename "$file")"
    hash="$(sha256_of "$file")"
    [ -n "$hash" ] || fail "could not compute the checksum of $name"
    printf '%s  %s\n' "$hash" "$name" > "${file}.sha256"
    echo "wrote $file"
    echo "wrote ${file}.sha256"
}

write_sha "$out"
write_sha "dist/$tools_name"
