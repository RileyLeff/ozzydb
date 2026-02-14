# OzzyDB development tasks

# Run all non-Docker tests
test:
    cargo test --workspace

# Start test infrastructure (postgres + minio for integration/E2E tests)
test-infra-up:
    docker compose -f docker-compose.test.yml up -d
    @echo "Waiting for services..."
    @until docker compose -f docker-compose.test.yml exec -T postgres pg_isready -U ozzy_test -q 2>/dev/null; do sleep 1; done
    @echo "Test infrastructure ready."

# Stop test infrastructure
test-infra-down:
    docker compose -f docker-compose.test.yml down

# Clean test infrastructure (removes volumes)
test-infra-clean:
    docker compose -f docker-compose.test.yml down -v

# Run integration tests (requires test infra: just test-infra-up)
test-docker:
    cargo test -p ozzy-server --test integration_tests -- --ignored

# Run E2E tests including compute pipeline (requires test infra + Docker)
test-e2e:
    cargo test -p ozzy-server --test e2e_tests -- --ignored

# Run all tests including integration and E2E
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
