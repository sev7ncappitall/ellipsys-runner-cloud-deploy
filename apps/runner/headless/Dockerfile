# Builds the headless Ellipsys Runner — the cloud-deployable counterpart to
# the desktop app. Holds broker credentials only in this container's own env
# vars (set by the subscriber in their own DigitalOcean/Railway/etc account);
# Ellipsys never receives them. Build from the apps/runner directory:
#   docker build -f headless/Dockerfile -t ellipsys-runner-headless ..

FROM rust:1.94-slim AS builder
WORKDIR /build
RUN apt-get update && apt-get install -y --no-install-recommends pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*
COPY core ./core
COPY headless ./headless
WORKDIR /build/headless
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /build/headless/target/release/ellipsys-runner-headless /usr/local/bin/ellipsys-runner-headless
ENTRYPOINT ["/usr/local/bin/ellipsys-runner-headless"]
