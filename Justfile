# OzzyDB development tasks

# Run all non-Docker tests
test:
    cargo test --workspace

# Run Docker integration tests (requires Docker)
test-docker:
    cargo test -p ozzy-server --test integration_tests -- --ignored

# Run E2E tests including compute pipeline (requires Docker, slow first run)
test-e2e:
    cargo test -p ozzy-server --test e2e_tests -- --ignored

# Run all tests including Docker and E2E
test-all:
    cargo test --workspace
    cargo test -p ozzy-server --test integration_tests -- --ignored
    cargo test -p ozzy-server --test e2e_tests -- --ignored

# Start full local dev stack (postgres + minio + server)
dev-up:
    docker compose -f docker-compose.dev.yml up -d

# Stop local dev stack
dev-down:
    docker compose -f docker-compose.dev.yml down

# Clean local dev stack (removes volumes)
dev-clean:
    docker compose -f docker-compose.dev.yml down -v

# Start services only (postgres + minio, no server — for running server locally)
services-up:
    docker compose -f docker-compose.dev.yml up -d postgres minio minio-init

# Stop services only
services-down:
    docker compose -f docker-compose.dev.yml down

# Run server locally against dev services
dev-server:
    cd crates/ozzy-server && cargo run

# Check + clippy + test
ci:
    cargo check --workspace
    cargo clippy --workspace -- -D warnings
    cargo test --workspace
