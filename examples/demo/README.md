# Webstack Demo

The demo is the framework's visual reference application. It uses application-owned Askama templates, htmx partials, Tailwind CSS v4, and daisyUI while exercising local accounts, roles, CSRF, embedded SurrealDB, and the shared Items feature.

From the repository root, provision the verified frontend tools once and build CSS:

```sh
just demo-setup
```

Start the server and CSS watcher together:

```sh
just demo-dev
```

Open `http://127.0.0.1:42069`. The local bootstrap login is `admin` / `changeme`.

Generated CSS and downloaded tools are intentionally ignored. Release builds require `just demo-css` first; templates, CSS, htmx, and the theme script are then embedded in the executable.
