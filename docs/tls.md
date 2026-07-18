# TLS and listeners

Webstack terminates TLS inside the application. Configure the listener in
`webstack.toml`; no reverse proxy is required.

## Listener modes

`tls.mode = "disabled"` serves plain HTTP on `server.http_port`.
`server.https_port` is not bound in this mode.

`tls.mode = "self_signed"` serves HTTPS on `server.https_port` with an
ephemeral in-memory certificate for `localhost`. Browsers and command-line
clients must explicitly trust or bypass verification for local development.

`tls.mode = "acme"` serves HTTPS on `server.https_port` and obtains a
certificate for `tls.domain` with the ACME TLS-ALPN-01 challenge. The
certificate cache is stored under `tls.acme_cache_dir`.

When either TLS mode is active and `server.http_redirect = true`, Webstack also
binds `server.http_port` and permanently redirects every request to HTTPS. The
redirect preserves the path and query string. The two configured ports must be
different.

## Self-signed development

A typical local configuration is:

```toml
[server]
bind_addr = "127.0.0.1"
https_port = 8443
http_port = 8080
http_redirect = true

[tls]
mode = "self_signed"
```

Open `http://127.0.0.1:8080` to exercise the redirect, or connect directly to
`https://127.0.0.1:8443`. The certificate exists only in memory and changes on
every restart.

## ACME production requirements

Before enabling ACME:

- Point the configured DNS name at the server.
- Make public TCP port 443 reach `server.https_port`, directly or through a
  port mapping that does not terminate TLS.
- Make `server.http_port` reachable if the redirect listener is enabled.
- Persist `tls.acme_cache_dir` with the application data to avoid certificate
  authority rate limits.
- Set `tls.acme_staging = true` while verifying a deployment, then switch it to
  `false` for a trusted production certificate.

The configured email is registered as the ACME contact. Startup validates the
domain, email, and listener ports before binding either listener.

## Runtime behavior

Database initialization, migrations, and authentication bootstrap finish
before any listener binds. If the primary or redirect listener cannot bind,
startup fails without leaving the other listener running. SIGINT or SIGTERM
stops both listeners and allows in-flight requests to finish.

Session cookies use the `Secure` attribute in self-signed and ACME modes.
Strict-Transport-Security and the remaining security headers belong to the
hardening phase.
