# -- Base env for build/test -- #

FROM rust:1.97 AS base

WORKDIR /app
COPY . .

# -- Test stage -- #

FROM base AS test
RUN \
    cargo test \
        --no-fail-fast

# -- Prod build stage -- #

FROM base AS prod_build
RUN \
    cargo build --release

# -- Prod base/common runtime -- #

FROM debian:trixie-slim AS prod_base

# Create data volume
VOLUME /data

# Install runtime deps
RUN \
    apt update && \
    apt install -y \
        ca-certificates && \
    rm -rf /var/lib/apt/lists/*

# -- Prod serve API stage -- #

FROM prod_base AS prod_api
LABEL org.opencontainers.image.description="gh-scaleset-pve"
LABEL org.opencontainers.image.source=https://github.com/kylekingcdn/gh-scaleset-pve

COPY --from=prod_build \
    /app/target/release/gh-scaleset-pve /usr/local/bin/gh-scaleset-pve

# Use entrypoint over command to allow for generate cmd invocation
ENTRYPOINT ["/usr/local/bin/gh-scaleset-pve"]
CMD []
