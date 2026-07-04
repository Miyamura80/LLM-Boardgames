# syntax=docker/dockerfile:1

# ---- Builder ----------------------------------------------------------------
FROM rust:1-bookworm AS builder
WORKDIR /app

# Cache dependencies first for faster rebuilds.
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
# `appctl new` embeds this template via include_str!; the (ungated) scaffold
# module needs it present at compile time, so it must be in the build context.
COPY templates ./templates
RUN cargo build --locked --release -p appctl

# ---- Runtime ----------------------------------------------------------------
FROM debian:bookworm-slim AS runtime

# TLS roots for outbound HTTPS (e.g. the http_request command).
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/appctl /usr/local/bin/appctl
# The config crate resolves its YAML relative to a compile-time path that does
# not exist in this slim image, so ship the file and point APP_CONFIG_PATH at it.
COPY --from=builder /app/crates/config/global_config.yaml /etc/appctl/global_config.yaml
ENV APP_CONFIG_PATH=/etc/appctl/global_config.yaml

# Override individual values with APP__* env vars. Bind all interfaces in-container.
ENV APP__SERVER__HOST=0.0.0.0
ENV APP__SERVER__PORT=8080
EXPOSE 8080

# Drop root: run the server as a dedicated unprivileged user.
RUN useradd --system --no-create-home --uid 10001 appctl
USER appctl

ENTRYPOINT ["appctl", "serve"]
