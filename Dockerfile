# Production image for Ask Hava: one image, two commands (web and worker).
# SEC-021, SEC-045: every base image is pinned by digest; Dependabot's `docker`
# ecosystem proposes new digests. There is deliberately no `# syntax=` line
# (it would pull another image, D-491). Web: `askhava-server` (the default).
# Worker: start the same image with the entrypoint `worker`. Never enable
# `test-control` here (SEC-035). Running the worker needs the entrypoint override.

FROM rust:1.99.0-slim-trixie@sha256:2752b332db73fdbb7dc576f06c82ed1f312005784ef913d7e04a28f5f55dc581 AS chef
# rustup would otherwise fetch clippy and rustfmt for rust-toolchain.toml.
ENV RUSTUP_TOOLCHAIN=1.99.0
RUN cargo install cargo-chef --locked --version 0.1.78
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
# SQLx must never reach for a database during the build (offline query data).
ENV SQLX_OFFLINE=true
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --locked --recipe-path recipe.json -p askhava-server
COPY . .
RUN cargo build --release --locked -p askhava-server --bins \
    && cp target/release/server /usr/local/bin/askhava-server \
    && cp target/release/worker /usr/local/bin/worker

FROM debian:trixie-slim@sha256:f6b08b09a05725f9431502d3b5714f5c4576e82c1a84fa1debe0c24a7e0c3ec4 AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates libssl3t64 \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --uid 10001 --no-create-home --shell /usr/sbin/nologin askhava
COPY --from=builder /usr/local/bin/askhava-server /usr/local/bin/askhava-server
COPY --from=builder /usr/local/bin/worker /usr/local/bin/worker
USER 10001
ENTRYPOINT ["askhava-server"]
