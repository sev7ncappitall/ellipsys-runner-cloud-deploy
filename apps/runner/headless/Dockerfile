# Builds the headless Ellipsys Runner — the cloud-deployable counterpart to
# the desktop app. Holds broker credentials only in this container's own env
# vars (set by the subscriber in their own DigitalOcean/Railway/etc account);
# Ellipsys never receives them. Build from the apps/runner directory:
#   docker build -f headless/Dockerfile -t ellipsys-runner-headless ..
#
# Builds against musl (Alpine's native libc) instead of glibc so the binary
# has no runtime libc version dependency at all — a glibc-vs-runtime-image
# mismatch between the Debian builder and runtime stages previously made
# every published image crash on startup regardless of source correctness.
# reqwest already uses rustls (no OpenSSL), so musl needs no extra TLS setup.

FROM rust:1.94-alpine AS builder
RUN apk add --no-cache musl-dev build-base
WORKDIR /build
COPY core ./core
COPY headless ./headless
WORKDIR /build/headless
RUN cargo build --release

FROM alpine:3
RUN apk add --no-cache ca-certificates
COPY --from=builder /build/headless/target/release/ellipsys-runner-headless /usr/local/bin/ellipsys-runner-headless
ENTRYPOINT ["/usr/local/bin/ellipsys-runner-headless"]
