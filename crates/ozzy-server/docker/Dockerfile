# Multi-stage build for ozzy-server
# Build context should be the repo root

FROM rust:1-alpine AS builder

RUN apk add --no-cache musl-dev openssl-dev openssl-libs-static pkgconfig

WORKDIR /app

# Copy workspace files
COPY Cargo.toml ./
COPY crates/ ./crates/

# Build release binary
ENV OPENSSL_STATIC=1
RUN cargo build --release --package ozzy-server

# Runtime image
FROM alpine:3.21

RUN apk add --no-cache ca-certificates curl docker-cli

WORKDIR /app

COPY --from=builder /app/target/release/ozzy-server /usr/local/bin/ozzy-server

# Server needs Docker socket access for compute pipeline,
# so it runs as root. Network isolation is handled by the
# compute containers themselves (--network=none, gVisor).

EXPOSE 3000

ENTRYPOINT ["ozzy-server"]
