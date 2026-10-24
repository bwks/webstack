# TLS and listeners

Webstack terminates TLS inside the application. Configure the listener in
`webstack.toml`; no reverse proxy is required.

## Listener modes

`tls.mode = "disabled"` serves plain HTTP on `server.http_port`.
`server.https_port` is not bound in this mode.

`tls.mode = "self_signed"` serves HTTPS on `server.https_port` with an
ephemeral in-memory certificate for `localhost`. Browsers and command-line
clients must explicitly trust or bypass verification for local development.

`tls.mode = "acme"` serves HTTPS on `server.https_port` and obtains a Let's
Encrypt certificate for `tls.domain`. `tls.acme_challenge` selects how the
certificate authority verifies the domain:

- `"tls_alpn"` (default) answers the TLS-ALPN-01 challenge on the HTTPS
  listener, so public port 443 must reach the application.
- `"dns_cloudflare"` answers the DNS-01 challenge with a temporary TXT record
  created through the Cloudflare API. The server never needs to be reachable
  from the internet, so it suits private networks such as a tailnet.

Certificates and ACME account keys are cached under `tls.acme_cache_dir`.

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

## ACME DNS-01 with Cloudflare

Use DNS-01 when the server is private or port 443 is unavailable:

```toml
[tls]
mode = "acme"
domain = "app.example.com"
acme_email = "you@example.com"
acme_cache_dir = "./data/acme"
acme_staging = true
acme_challenge = "dns_cloudflare"
```

Requirements:

- `tls.domain` must be in a zone hosted on Cloudflare. Its A or AAAA record
  may point at a private address; it only has to resolve for your clients.
- Create a Cloudflare API token with the **Zone → DNS → Edit** permission,
  restricted to that zone, and supply it only through the
  `CLOUDFLARE_API_TOKEN` environment variable. The token is never read from
  `webstack.toml`, and is redacted from diagnostics.
- Keep `tls.acme_cache_dir` persistent and private. It holds the certificate,
  its private key, and the ACME account key, written with owner-only
  permissions under `dns-01/staging` or `dns-01/production`.

At startup, Webstack serves a cached certificate that is not yet due for
renewal without contacting Let's Encrypt or Cloudflare. Otherwise it requests
one before binding any listener: it creates
`_acme-challenge.<domain>`, waits for the record to propagate, completes the
order, and deletes the record. Startup fails if no valid certificate can be
obtained. A cached certificate that is due for renewal but not yet expired is
still served when renewal fails.

A background task renews the certificate once two thirds of its lifetime has
passed and swaps it into the running listener without a restart. Failed
renewals are logged and retried hourly. Staging and production certificates
are cached separately, so switch `acme_staging` to `false` once the staging
certificate is issued.

## Runtime behavior

Database initialization, migrations, and authentication bootstrap finish
before any listener binds. If the primary or redirect listener cannot bind,
startup fails without leaving the other listener running. SIGINT or SIGTERM
stops both listeners and allows in-flight requests to finish for up to
`server.shutdown_timeout_seconds`, which defaults to 30 seconds.

Session cookies use the `Secure` attribute in self-signed and ACME modes.
HTTPS responses include `Strict-Transport-Security: max-age=31536000`.
`includeSubDomains` and preload are intentionally omitted.
