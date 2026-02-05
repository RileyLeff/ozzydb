# Multi-stage build for ozzy-server
# Build context should be crates/ directory

FROM rust:1.83-alpine AS builder

RUN apk add --no-cache musl-dev openssl-dev openssl-libs-static pkgconfig

WORKDIR /app

# Copy workspace files
COPY Cargo.toml Cargo.lock ./
COPY ozzy-core ./ozzy-core
COPY ozzy-cli ./ozzy-cli
COPY ozzy-server ./ozzy-server

# Build release binary
ENV OPENSSL_STATIC=1
RUN cargo build --release --package ozzy-server

# Runtime image
FROM alpine:3.21

RUN apk add --no-cache ca-certificates curl

WORKDIR /app

COPY --from=builder /app/target/release/ozzy-server /usr/local/bin/ozzy-server

# Create non-root user
RUN adduser -D -u 1000 ozzy
USER ozzy

EXPOSE 3000

ENTRYPOINT ["ozzy-server"]
