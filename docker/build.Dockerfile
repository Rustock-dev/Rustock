# syntax=docker/dockerfile:1

# Must match the channel in rust-toolchain.toml. This default is a fallback for
# a hand-run `docker build`; scripts/build-binary.sh reads the real value out of
# rust-toolchain.toml and passes it. Either way the build checks it below.
ARG RUST_VERSION=1.93.1

# The binary links this image's glibc and libstdc++; docker/runtime.Dockerfile
# must stay on the same Debian release.
FROM rust:${RUST_VERSION}-bookworm AS build

# librocksdb-sys compiles C++, and zstd-sys runs bindgen, which needs libclang.
RUN apt-get update \
 && apt-get install -y --no-install-recommends build-essential clang libclang-dev \
 && rm -rf /var/lib/apt/lists/*

# Stops rustup from fetching the components listed in rust-toolchain.toml.
ARG RUST_VERSION
ENV RUSTUP_TOOLCHAIN=${RUST_VERSION}

WORKDIR /src
COPY . .

# The compiler is pinned in two places: RUST_VERSION above, which picks the base
# image and (via RUSTUP_TOOLCHAIN) the toolchain used here, and
# rust-toolchain.toml, which is what a developer's local `cargo build` obeys. A
# comment cannot hold an invariant across two files -- so check it here, the
# first point where both are present. Drift becomes a failed build instead of
# shipped binaries built by a compiler nobody is running locally.
RUN set -eu; \
    pinned="$(sed -n 's/^[[:space:]]*channel[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' rust-toolchain.toml)"; \
    [ -n "$pinned" ] || { echo "error: no [toolchain] channel found in rust-toolchain.toml" >&2; exit 1; }; \
    if [ "$pinned" != "${RUST_VERSION}" ]; then \
        echo "error: RUST_VERSION is ${RUST_VERSION} but rust-toolchain.toml pins ${pinned}" >&2; \
        echo "       fix: update the ARG default in docker/build.Dockerfile, or build with" >&2; \
        echo "            --build-arg RUST_VERSION=${pinned}" >&2; \
        exit 1; \
    fi; \
    actual="$(rustc --version | cut -d' ' -f2)"; \
    if [ "$actual" != "${pinned}" ]; then \
        echo "error: rust-toolchain.toml pins ${pinned} but this image's rustc is ${actual}" >&2; \
        exit 1; \
    fi; \
    echo "toolchain ${pinned} agrees across rust-toolchain.toml, RUST_VERSION and rustc"

# Comma-separated cargo features for rustock-cli, e.g. "smtp".
ARG FEATURES=""
# Maintenance tools: crates/cli/examples/<name>.rs, shipped beside the binary.
ARG TOOLS="rewind_to set_exec_head reset_exec_head block_info height_peek"
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    examples=""; for t in $TOOLS; do examples="$examples --example $t"; done; \
    cargo build --release --locked -p rustock-cli ${FEATURES:+--features "$FEATURES"} $examples \
 && install -D target/release/rustock-cli /out/rustock-cli \
 && for t in $TOOLS; do install -D "target/release/examples/$t" "/out/tools/$t"; done


FROM scratch AS binary
COPY --from=build /out/rustock-cli /rustock-cli

FROM scratch AS tools
COPY --from=build /out/tools/ /
