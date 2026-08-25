# htmx Patterns

`webstack::htmx::HxRequest` is an infallible extractor. It reports an htmx
request only when `HX-Request` is `true` (case-insensitive); absent, false, and
malformed values are ordinary requests. Handlers use it to choose a full Askama
page or an application-owned partial.

Replaceable partials must own a stable outer element. The generated Items
feature returns `#items-region` after create, update, and delete, using
`hx-swap="outerHTML"`. Its `#item-errors` live region is the target for 4xx and
5xx responses.

Unsafe forms carry CSRF in two places: `_csrf` supports normal browser form
submission, while explicit `X-CSRF-Token` `hx-headers` supports htmx. Keep the
header on the element issuing the request because htmx 4 does not implicitly
inherit attributes.

Application handlers return `AppError`. Webstack chooses full or partial safe
HTML and preserves the status. Applications can register one renderer with
`ApplicationBuilder::error_renderer`; returning `None` uses the escaped
framework fallback. `AppError::internal` logs its source but exposes only
generic public text.

Wrap transaction-safe Turso mutations with `retry_write`, but keep template
rendering, follow-up reads, and all external side effects outside its closure.
The closure can execute once initially and three more times after conflicts.
