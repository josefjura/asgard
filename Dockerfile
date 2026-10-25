# syntax=docker/dockerfile:1

# Alpine's Rust targets musl and links statically, SQLite included.
# Matches rust-toolchain.toml, so the image is built by the compiler CI checks with.
FROM rust:1.99.1-alpine AS builder
RUN apk add --no-cache musl-dev
WORKDIR /src
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --locked --profile dist \
    && cp target/dist/asgard /asgard \
    && mkdir -p /data/backups

# Static binary only: CA certificates and tzdata come with the base, no shell.
FROM gcr.io/distroless/static-debian12:nonroot
COPY --from=builder /asgard /usr/local/bin/asgard
COPY --from=builder --chown=nonroot:nonroot /data /data
ENV DATABASE_URL=sqlite:/data/asgard.db \
    BACKUP_DIR=/data/backups \
    PORT=8080
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/asgard"]
