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

# Start dev services (postgres + minio)
dev-up:
    docker compose -f crates/ozzy-server/docker/docker-compose.yml up -d

# Stop dev services
dev-down:
    docker compose -f crates/ozzy-server/docker/docker-compose.yml down

# Clean dev volumes
dev-clean:
    docker compose -f crates/ozzy-server/docker/docker-compose.yml down -v

# Run server locally against dev services
dev-server:
    cd crates/ozzy-server && cargo run

# Check + clippy + test
ci:
    cargo check --workspace
    cargo clippy --workspace -- -D warnings
    cargo test --workspace
