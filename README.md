# Ellipsys Runner Deploy Template

This repository intentionally contains no Ellipsys application source code and no Runner source code.

It exists only so DigitalOcean App Platform can read `.do/deploy.template.yaml` for a one-click deployment of the prebuilt `ghcr.io/sev7ncappitall/ellipsys-runner-headless:latest` image.

Subscriber broker credentials are entered directly into the subscriber-owned DigitalOcean app as secret environment variables. Ellipsys infrastructure does not receive or store those credentials.
