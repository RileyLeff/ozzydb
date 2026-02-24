# Multi-stage build for ozzy-server using cargo-chef
# Build context should be the repo root

# --- Stage 1: Install cargo-chef ---
FROM rust:1-alpine AS chef
RUN apk add --no-cache musl-dev openssl-dev openssl-libs-static pkgconfig
RUN cargo install cargo-chef
WORKDIR /app

# --- Stage 2: Prepare dependency recipe ---
FROM chef AS planner
COPY Cargo.toml ./
COPY crates/ ./crates/
RUN cargo chef prepare --recipe-path recipe.json

# --- Stage 3: Cook dependencies (cached until Cargo.toml/lock change) ---
FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
ENV OPENSSL_STATIC=1
RUN cargo chef cook --release --recipe-path recipe.json --package ozzy-server

# Now copy actual source and build (only recompiles workspace code)
COPY Cargo.toml ./
COPY crates/ ./crates/
RUN cargo build --release --package ozzy-server && \
    cp /app/target/release/ozzy-server /usr/local/bin/ozzy-server

# --- Stage 4: Runtime image ---
FROM alpine:3.21

RUN apk add --no-cache ca-certificates curl docker-cli

WORKDIR /app

COPY --from=builder /usr/local/bin/ozzy-server /usr/local/bin/ozzy-server

# Server needs Docker socket access for compute pipeline,
# so it runs as root. Network isolation is handled by the
# compute containers themselves (--network=none, gVisor).

EXPOSE 3000

ENTRYPOINT ["ozzy-server"]
