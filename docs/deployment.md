# Deployment

Generated applications contain the deployable artifacts; Webstack itself is a
framework workspace rather than a production service.

## Alpine image

Run `just setup` once and then `just image` in a generated application. CSS is
built before Docker receives the context. The multi-stage Dockerfile uses
`rust:1.97.0-alpine3.24` for compilation and `alpine:3.24` at runtime. The runtime contains only CA certificates required by the application.

The process runs as UID/GID 10001. Mount production configuration at
`/app/webstack.toml` and persistent state at `/app/data`; configure the server
to bind `0.0.0.0` and keep the database below `./data`. The immutable migration
history is included in the image. Probe `/healthz` externally rather than
installing diagnostic clients in the runtime image.

Alpine uses musl, so CI builds the real image to cover Turso and
rustls native dependencies. The initial CI platform is `linux/amd64`; native
ARM64 builders can use the same Dockerfile.

## systemd

Generated applications include a unit for an unprivileged dedicated account.
Install the binary, configuration, and migrations together under `/opt`, store
writable data under `/var/lib`, and keep the configured shutdown deadline below
systemd's stop timeout. Ports 8080 and 8443 require no capabilities. A firewall
may forward 443 to 8443 without terminating TLS.
