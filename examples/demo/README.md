# Webstack Demo

The demo is the framework's visual reference application. It uses application-owned Askama templates, htmx partials, Tailwind CSS v4, and daisyUI while exercising local accounts, roles, CSRF, embedded SurrealDB, and the shared Items feature.

Its source mirrors newly generated applications: `src/application.rs` composes
the process, `src/domain` owns models and persistence contexts, and `src/web`
owns the router, handlers, views, navigation, assets, and error rendering.
Templates are grouped by feature under `templates/`.

From the repository root, provision the verified frontend tools once and build CSS:

```sh
just demo-setup
```

Start the server and CSS watcher together:

```sh
just demo-dev
```

Open `http://127.0.0.1:42069`. The HTTP listener redirects to the demo's
self-signed HTTPS listener at `https://127.0.0.1:7337`; accept the browser's
local certificate warning. From the development LAN, use
`http://10.100.58.10:42069` or `https://10.100.58.10:7337`. The local bootstrap
login is `admin` / `changeme`.

Generated CSS and downloaded tools are intentionally ignored. Release builds require `just demo-css` first; templates, CSS, htmx, and the theme script are then embedded in the executable.
