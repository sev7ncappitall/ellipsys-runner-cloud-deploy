# Ellipsys Runner Cloud Deploy

This public repository contains only the generic Ellipsys Runner cloud pieces:

- `.do/deploy.template.yaml` for the DigitalOcean one-click flow
- `apps/runner/core` shared broker + poller logic
- `apps/runner/headless` for the cloud worker binary
- `.github/workflows/publish-headless.yml` to publish `ghcr.io/sev7ncappitall/ellipsys-runner-cloud-headless`

It does not contain the private Ellipsys web app, Titan backend, strategies, signal-generation code, or other proprietary server-side sources.

## Publish flow

Every push to `main` builds the headless Docker image from `apps/runner/headless/Dockerfile` and publishes it to GHCR as:

- `ghcr.io/sev7ncappitall/ellipsys-runner-cloud-headless:latest`
- `ghcr.io/sev7ncappitall/ellipsys-runner-cloud-headless:sha-<commit>`

## Deploy button

[Deploy to DigitalOcean](https://cloud.digitalocean.com/apps/new?repo=https://github.com/sev7ncappitall/ellipsys-runner-cloud-deploy/tree/main)

Subscribers enter broker credentials directly into their own DigitalOcean app as environment variables. Ellipsys infrastructure does not receive or store those credentials.
