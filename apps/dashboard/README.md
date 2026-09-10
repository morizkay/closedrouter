# ClosedRouter dashboard

React 19 + Vite admin console for the ClosedRouter gateway. TanStack Router handles client-side
navigation and TanStack Query owns gateway reads and mutations.

## Development

```bash
npm ci
npm run dev
```

Set `PUBLIC_GATEWAY_URL` when the gateway is not available at `http://localhost:8080`. The admin
token is entered in the browser login screen and stored in `localStorage`; upstream provider
credentials stay in the gateway.

## Verification

```bash
npm run check
npm run lint
npm run build
HOST=0.0.0.0 PORT=3000 npm run start
```

The production server serves `dist/` and falls back to `index.html` for client-side routes.
