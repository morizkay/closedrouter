# Changelog

All notable changes to ClosedRouter are documented here.

## [0.1.0] - 2026-09-10

### Added

- Replaced the SvelteKit dashboard with a React 19 + Vite admin console.
- Added TanStack Router navigation and TanStack Query gateway data management.
- Added runtime dashboard gateway configuration for Docker deployments.
- Added dashboard pages for status, API keys, providers, models, playground requests, and client setup.
- Refreshed the Astro landing page and deployment presentation.

### Fixed

- Kept failed dashboard connections on the login screen instead of switching to a broken authenticated state.
- Hardened the dashboard static server against path traversal and malformed URLs.
- Improved pinned HTTP and Postgres test behavior.

### Documentation

- Updated architecture, dashboard, landing page, and setup documentation for the React dashboard.
