[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![unsafe forbidden](https://img.shields.io/badge/unsafe-forbidden-success.svg)](https://github.com/rust-secure-code/safety-dance/)

# axum-folder-router

`#[folder_router(...)]` is a procedural attribute macro for the [Axum](https://github.com/tokio-rs/axum)
web framework that generates router boilerplate from your directory & file
structure. Inspired by file-system routing in frameworks like Next.js.

This is a fork of [axum-folder-router](https://github.com/vault81/axum-folder-router)
by vault81 that adds support for **middleware**, **fallbacks**, and **intercepts**.

## Features

- **File system-based routing** — define routes with an intuitive folder layout
- **Less boilerplate** — route-mapping code is generated for you
- **Composable conventions** — drop in `middleware.rs`, `fallback.rs`, or `intercept.rs` per folder
- **State-aware** — share a typed `AppState` across handlers, middleware, and intercepts
- **OpenAPI (optional)** — generate a `utoipa` spec from the route tree (feature `openapi`)

## Conventions

Four file names are recognized in any folder. See [the examples](./examples)
for runnable `simple` and `advanced` setups, and [docs.rs](https://docs.rs/axum-folder-router)
for the full reference (extractor rules, scoping, and inheritance).

### `route.rs` — handlers

Define one async function per HTTP method (`get`, `post`, `put`, `delete`,
`patch`, `head`, `options`, `trace`, `connect`, or `any`). They are ordinary
Axum handlers, so all extractors work — `State<AppState>`, `Path`, `Query`, and
so on:

```rust
use axum::{
    extract::{Path, State},
    response::{Html, IntoResponse},
};

pub async fn get() -> impl IntoResponse {
    Html("<h1>Hello World!</h1>")
}

pub async fn post(Path(id): Path<String>, State(state): State<AppState>) -> impl IntoResponse {
    format!("created {id}")
}
```

### `middleware.rs` — layers for a subtree

A `middleware.rs` applies to its folder's `route.rs` and every nested route,
leaving sibling subtrees untouched. The macro hands you the subtree's `Router`
and you decide how to attach the layer (`.layer` for every request,
`.route_layer` to skip unmatched paths):

```rust
use tower_http::limit::RequestBodyLimitLayer;

pub fn middleware(router: Router<AppState>) -> Router<AppState> {
    // Reject request bodies larger than 1 MiB for every route in this subtree.
    router.layer(RequestBodyLimitLayer::new(1024 * 1024))
}
```

Any [`tower`]/[`tower-http`] layer works here; attach your own `from_fn`
middleware the same way. Add a second `state: AppState` parameter when the
middleware needs the app state (e.g. for `from_fn_with_state`) — state-aware
middleware is built via `into_router_with_state(state)` instead of
`into_router()`.

[`tower`]: https://docs.rs/tower
[`tower-http`]: https://docs.rs/tower-http

### `fallback.rs` — fallback for a subtree

Declared like `middleware.rs`: you receive the `Router` and attach a fallback
handler (`.fallback`) or service (`.fallback_service`). An unmatched request
resolves to the most specific `fallback.rs` at or above its path, so a single
file covers its whole subtree:

```rust
use tower_http::services::ServeDir;

pub fn fallback(router: Router<AppState>) -> Router<AppState> {
    router.fallback_service(ServeDir::new("static"))
}
```

### `intercept.rs` — inspect, then continue or divert

For the common "check each request, then let it through or short-circuit" case,
an `intercept.rs` lets you write only the decision — the macro generates the
layer. Parameters are extractors (the forwarded `Request` must come **last**),
and you return `ControlFlow<Response, Request>`:

```rust
pub async fn intercept(
    headers: axum::http::HeaderMap,
    req: Request,
) -> ControlFlow<Response, Request> {
    let authorized = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        == Some("password");

    if authorized {
        ControlFlow::Continue(req) // proceed (optionally after mutating `req`)
    } else {
        ControlFlow::Break((StatusCode::UNAUTHORIZED, "unauthorized\n").into_response())
    }
}
```

Use `intercept.rs` to guard or augment requests on the way in; reach for
`middleware.rs` when you also need to touch the response on the way out.

A `middleware.rs`/`fallback.rs`/`intercept.rs` in a **subfolder** makes that
subtree a nested boundary, mounted with `Router::nest_service` so its layers run
on the bare boundary path in *both* slash forms (`/admin` and `/admin/`) — a
guard can't be bypassed by a trailing slash. Because a service has its state
baked in at construction, a tree with nested boundaries is built with
`into_router_with_state(state)` (not `into_router()`), even when every layer is
state-agnostic.

## OpenAPI generation (feature `openapi`)

Enable the feature and add [`utoipa`](https://docs.rs/utoipa) `5` to your crate:

```toml
axum-folder-router = { version = "0.4", features = ["openapi"] }
utoipa = "5"
```

Then add the `openapi` flag to the macro. Alongside the usual `into_router*`, the
macro emits a state-free `openapi()` constructor returning a
`utoipa::openapi::OpenApi`, built from the route tree:

```rust
use utoipa::ToSchema;

#[derive(ToSchema, serde::Serialize, serde::Deserialize)]
struct User { id: u64, name: String }

#[folder_router("src/api", AppState, openapi)]
struct Api();

let doc = Api::openapi(); // serve via utoipa-swagger-ui, or merge into a larger doc
```

Staying true to the macro's purely *syntactic* design, it reads each handler's
signature tokens and recognizes a small set of axum wrappers by name:

- **Paths, methods, path params, and doc comments** are derived directly from the
  file tree (`[id]` → a required `{id}` parameter; the first doc line becomes the
  operation summary, the rest its description).
- **`Json<T>` / `Form<T>`** parameters become the request body, **`Query<T>`**
  becomes query parameters (via `utoipa::IntoParams`), and a concrete
  **`Json<T>`**, **`Result<Json<T>, _>`**, or tuple return type becomes the `200`
  response body. The schemas themselves come from `#[derive(ToSchema)]` on `T` —
  the macro never inspects your types' fields; the compiler does. A **`Vec<T>`**
  body/response (e.g. `Json<Vec<T>>`) is emitted as an inline `array` of `T`
  refs, with `T` (not `Vec`) registered as the component schema.

Limitations: an opaque return type (`impl IntoResponse`, `String`, `Html<_>`)
yields a bodyless `200` — there is nothing to recover syntactically. The `any` and
`connect` verbs have no single OpenAPI operation and are omitted. As with
`intercept.rs` extractors, any schema/param type named in a handler signature must
be **nameable at the `#[folder_router]` site** (import it there or write it fully
qualified). See [`examples/openapi`](./examples/openapi) for a runnable demo
(`cargo run --example openapi --features openapi`).

## License

This repository is licensed permissively under the terms of the MIT license.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you shall be licensed as above, without any
additional terms or conditions.

### Attribution

This version is a fork of [axum-folder-router](https://github.com/vault81/axum-folder-router) by vault81.

The original macro is based on the [build.rs template by @richardanaya](https://github.com/richardanaya/axum-folder-router-htmx).
