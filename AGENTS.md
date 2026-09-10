# Repository Guidelines

## Project Structure & Module Organization

- `crates/gateway/src/` contains the Rust Axum gateway, including transport, provider translation, auth, database, observability, and URL-policy modules.
- `apps/dashboard/` is the React + Vite admin UI (`src/pages/` for screens, `src/components/` for shell/auth, and `src/lib/` for API/session state). `apps/landing/` is the Astro marketing site; its Vercel project root is `apps/landing`.
- `docs/` holds API, architecture, and Hermes documentation. `deploy/` contains Docker, Postgres, Prometheus, Loki, and Grafana configuration. `examples/langchain/` contains TypeScript and Python client examples.
- Gateway tests live next to implementation code in `#[cfg(test)]` modules.

## Build, Test, and Development Commands

Install frontend dependencies with `npm ci` in each app. Common commands are:

```bash
make test       # Rust tests plus dashboard/landing type checks
make lint       # Clippy with warnings denied plus frontend linting
make fmt        # cargo fmt and Prettier write mode
make check      # lint followed by test
cargo run -p closedrouter-gateway
docker compose up --build
```

For focused UI work, run `cd apps/dashboard && npm run dev` or `cd apps/landing && npm run dev`. Use `npm run build` for a production build. The pinned Rust version is in `rust-toolchain.toml`.

## Coding Style & Naming Conventions

Run the formatters before committing. Rust follows `rustfmt`, with `snake_case` modules/functions and `PascalCase` types. Dashboard and landing sources use tabs, single quotes, no trailing commas, and a 100-column Prettier width; components use `PascalCase`, while TypeScript variables/functions use `camelCase`. Document provider/API behavior changes in `docs/`.

## Testing Guidelines

Run `cargo test --workspace` for gateway tests and `npm run check` for frontend type checks. Wiremock and translation tests require no API keys. Database tests use `DATABASE_URL` or Docker with `pgvector/pgvector:pg16`; CI supplies a Postgres service. Add focused `#[test]` cases alongside changed Rust modules, and run `npm run lint` for UI changes. No coverage threshold is configured.

## Commit & Pull Request Guidelines

Use Conventional Commits with the current branch name as the scope: `<type>(<branch>): <imperative summary>`. Examples: `feat(main): ...`, `fix(testing-1): ...`, and `chore(bug-xxxx): ...`. Follow the exact current branch name rather than substituting a generic scope. Keep subjects short and include an issue/PR reference when relevant. PRs should explain the behavior change, list validation commands, call out configuration/security implications, and include screenshots for UI changes. Keep unrelated formatting or generated files out of the diff.

For releases, update the root `changelog.md`, commit the release changes, push the current branch, create an annotated semantic-version tag, and push that tag.

## Security & Configuration Tips

Never commit `.env` files, provider keys, database credentials, or real admin tokens. Start from `.env.example` and `config.example.yaml`; use a unique `ADMIN_TOKEN` and do not use `change-me-now`. Preserve authentication, `/metrics` protection, CORS, and SSRF URL-policy safeguards when modifying gateway routes or upstream handling.
