# syntax=docker/dockerfile:1

# Alpine's Rust targets musl and links statically, SQLite included.
# Matches rust-toolchain.toml, so the image is built by the compiler CI checks with.
FROM rust:1.99.1-alpine AS builder
# perl and make build the vendored OpenSSL that webauthn-rs links against.
RUN apk add --no-cache musl-dev perl make
WORKDIR /src
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --locked --profile dist \
    && cp target/dist/asgard /asgard \
    && mkdir -p /data/backups

# Static binary only: CA certificates and tzdata come with the base, no shell.
# uid 10001 is the VPS's default for app containers, so the chart needs no override.
FROM gcr.io/distroless/static-debian12
COPY --from=builder /asgard /usr/local/bin/asgard
COPY --from=builder --chown=10001:10001 /data /data
USER 10001:10001
ENV DATABASE_URL=sqlite:/data/asgard.db \
    BACKUP_DIR=/data/backups \
    PORT=8080
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/asgard"]
