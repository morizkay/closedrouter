.PHONY: test lint fmt check

# Unit tests + wiremock. Postgres tests run when DATABASE_URL is set
# or when Docker is available (testcontainers + pgvector/pgvector:pg16).
test:
	cargo test --workspace
	cd apps/dashboard && npm run check
	cd apps/landing && npm run check

# Same as `cargo test`, skipping UI typecheck.
test-rust:
	cargo test --workspace

lint:
	cargo clippy -p closedrouter-gateway --all-targets -- -D warnings
	cd apps/dashboard && npm run lint
	cd apps/landing && npm run lint

fmt:
	cargo fmt --all
	cd apps/dashboard && npm run format
	cd apps/landing && npm run format

check: lint test
