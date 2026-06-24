# DigitalOcean smoke test

Use this after the GitHub Actions image publish completes.

1. Open the deploy button:
   `https://cloud.digitalocean.com/apps/new?repo=https://github.com/sev7ncappitall/ellipsys-runner-cloud-deploy/tree/main`
2. Confirm the Worker image resolves to `ghcr.io/sev7ncappitall/ellipsys-runner-cloud-headless:latest`.
3. Set:
   - `PORTAL_BASE_URL=https://ellipsys-app.vercel.app`
   - `RUNNER_TOKEN=<runner token from the portal>`
   - `VENUE=<alpaca|kraken|tradelocker>`
   - `IS_PAPER=true` for first deployment
4. Fill only the venue-specific `ELLIPSYS_CRED_*` values in DigitalOcean.
5. Deploy, then confirm the worker starts without crash looping.
6. In Ellipsys:
   - the runner heartbeat appears under the subscriber's broker/deployment state
   - the broker connection moves from pending to connected
7. Trigger one paper order and confirm:
   - the runner fetches it
   - the venue accepts it
   - the ack reaches `/api/portal/runner/ack`
   - deployment state updates in Postgres

Notes:
- IBKR is intentionally local-only right now because it still depends on a local TWS / IB Gateway session.
- Ellipsys never receives the broker credentials entered into DigitalOcean.
