/*!

[`macro@folder_router`] is a procedural macro for the Axum web framework
that automatically generates router boilerplate based on your file
structure. It simplifies route organization by using filesystem conventions
to define your API routes.

# Installation

Add the dependency to your ```Cargo.toml```:

```toml
[dependencies]
axum_folder_router = "0.3"
axum = "0.8"
```

See [Avoiding Cache Issues](#avoiding-cache-issues) on how to fix cargos
caching, which may cause new ```route.rs``` files to be ignored.

# Crate Features

* **nightly** -
  Enables use of unstable [`track_path`](https://doc.rust-lang.org/beta/unstable-book/library-features/track-path.html) feature to [avoid cache issues](#avoiding-cache-issues).
* **debug** -
  Adds some debug logging

# Basic Usage

The macro scans a directory for ```route.rs``` files and automatically
creates an Axum router based on the file structure:

```rust,no_run
*/
#![doc = include_str!("../examples/simple/main.rs")]
/*!
```

## Folder Structure

The macro converts your file structure into routes:
```text
src/api/
├── route.rs                 -> "/"
├── hello/
│   └── route.rs             -> "/hello"
├── users/
│   ├── route.rs             -> "/users"
│   └── [id]/
│       └── route.rs         -> "/users/{id}"
└── files/
    └── [...path]/
        └── route.rs         -> "/files/\*path"
```

Each ```route.rs``` file can contain HTTP method handlers that are automatically mapped to the corresponding route.

## Route Handlers

Inside each ```route.rs``` file, define async functions named after HTTP methods:
```rust
*/
#![doc = include_str!("../examples/simple/api/route.rs")]
/*!
```

# Detailed Usage

## HTTP Methods

The macro supports all standard HTTP methods as defined in RFC9110.
- ```get```
- ```post```
- ```put```
- ```delete```
- ```patch```
- ```head```
- ```options```
- ```trace```
- ```connect```

And additionally
- ```any```, which matches all methods

## Path Parameters

Dynamic path segments are defined using brackets:
```text
src/api/users/[id]/route.rs   -> "/users/{id}"
```

Inside the route handler:
```rust
use axum::{
  extract::Path,
  response::IntoResponse
};

pub async fn get(Path(id): Path<String>) -> impl IntoResponse {
    format!("User ID: {}", id)
}
```

## Catch-all Parameters

Use the spread syntax for catch-all segments:
```text
src/api/files/[...path]/route.rs   -> "/files/\*path"
```
```rust
use axum::{
  extract::Path,
  response::IntoResponse
};

pub async fn get(Path(path): Path<String>) -> impl IntoResponse {
    format!("Requested file path: {}", path)
}
```

## State Extraction

The state type provided to the macro is available in all route handlers:
All routes share the same state type, though you can use ```FromRef``` for more granular state extraction.
```rust
use axum::{
  extract::State,
  response::IntoResponse
};

# #[derive(Debug, Clone)]
# struct AppState ();

pub async fn get(State(state): State<AppState>) -> impl IntoResponse {
    format!("State: {:?}", state)
}
```

## Middleware

Place a ```middleware.rs``` file in any folder to apply middleware to every
route in that folder's subtree (the folder's own ```route.rs``` and all nested
routes), while leaving sibling subtrees untouched:
```text
src/api/
├── route.rs                 -> "/"            (no middleware)
└── users/
    ├── middleware.rs        -> applies to "/users" and "/users/{id}"
    ├── route.rs             -> "/users"
    └── [id]/
        └── route.rs         -> "/users/{id}"
```

The file must expose a ```pub fn middleware``` that receives the subtree's
```Router``` and returns it. You decide how to attach your middleware — use
```layer``` to run it for every request routed into the subtree, or
```route_layer``` to skip it on unmatched paths (handy for auth):
```rust
use axum::{
  extract::Request,
  middleware::Next,
  response::Response,
  Router,
};

async fn my_middleware(request: Request, next: Next) -> Response {
    next.run(request).await
}

pub fn middleware<S>(router: Router<S>) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    router.route_layer(axum::middleware::from_fn(my_middleware))
}
```
The function may be generic over the state ```S``` (as above) for
state-agnostic middleware, or pin it to your concrete state type if the
middleware needs to read it. Nested ```middleware.rs``` files compose, with the
outer folder's middleware wrapping the inner one.

## Middleware that needs state

Some middleware needs the application state itself — e.g. to build a
```from_fn_with_state``` layer that reads a database or session store. Give
```middleware``` a **second parameter** and the macro hands it a clone of the
state value:
```rust
use axum::{
  extract::{Request, State},
  middleware::{from_fn_with_state, Next},
  response::Response,
  Router,
};

# #[derive(Clone)]
# struct AppState;

async fn require_auth(State(_state): State<AppState>, request: Request, next: Next) -> Response {
    // inspect `_state` (DB pool, session store, ...) to authorize the request
    next.run(request).await
}

pub fn middleware(router: Router<AppState>, state: AppState) -> Router<AppState> {
    router.route_layer(from_fn_with_state(state, require_auth))
}
```
The state value only exists at construction time, so a router containing any
state-aware middleware is built with ```into_router_with_state``` instead of
```into_router```. That call threads the state into each state-aware
```middleware``` and applies ```with_state``` for you, returning a ready
```axum::Router```:
```rust,ignore
let app = ApiRouter::into_router_with_state(state);
```
When any ```middleware.rs``` uses this two-argument form, ```into_router``` is
**not** generated — so state-aware middleware can't be accidentally skipped.
Routers whose middleware is all state-agnostic keep both ```into_router``` and
```into_router_with_state```.

## Fallback

A ```fallback.rs``` in any folder declares the fallback for that folder's subtree
the same way ```middleware.rs``` declares a layer: it receives the subtree's
```Router``` and you decide how to attach the fallback (```fallback``` for a
handler, ```fallback_service``` for a ```Service``` such as a static
```ServeDir```):
```rust
use axum::{http::StatusCode, response::IntoResponse, Router};

async fn not_found() -> impl IntoResponse {
    (StatusCode::NOT_FOUND, "not found")
}

pub fn fallback<S>(router: Router<S>) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    router.fallback(not_found)
}
```
Like middleware, ```fallback``` can take a second ```state``` parameter to receive a
clone of the app state (e.g. to serve a directory behind a state-aware auth
layer), and a stateful ```fallback``` forces ```into_router_with_state``` just as
stateful middleware does.

### Scoping and inheritance

A folder that owns a ```middleware.rs``` or ```fallback.rs``` is composed as its
own nested router (```Router::nest```). An unmatched request resolves to the
**most specific fallback at or above** its path: a subtree's own ```fallback.rs```
if it has one, otherwise the nearest ancestor's, down to the root. Folders with no
fallback transparently inherit their ancestor's, so a single ```fallback.rs```
covers its whole subtree:
```text
src/api/
├── fallback.rs          -> serves unmatched /api, /api/admin, /api/admin/...
├── route.rs
└── admin/
    └── route.rs         -> unmatched requests under /admin inherit /api/fallback.rs
```
A deeper ```fallback.rs``` overrides the inherited one for its own subtree and is
never silently shadowed, even when an intervening folder has no fallback of its
own.

### Limitation

A ```fallback.rs``` (or ```middleware.rs```) cannot live in a catch-all directory
(```[...rest]```): such a subtree would have to be nested at a wildcard prefix,
which axum forbids. This is a compile error.

## Avoiding Cache Issues

By default newly created route.rs files may be ignored due to cargo's build-in caching.

### Nightly Rust

If you're using a nightly toolchain, just enable the `nightly` feature.
```toml
[dependencies]
axum_folder_router = { version = "0.3", features = ["nightly"] }
```
This enables us to use the unstable [`track_path`](https://doc.rust-lang.org/beta/unstable-book/library-features/track-path.html) API to tell cargo to watch for changes in your route directories.

### Stable Rust (requires `build.rs`)

On stable, you'll need to add this `build.rs` to your crate root:
```rust
fn main() {
   // Watch routes folder, so it picks up new routes
   println!(
       "cargo:rerun-if-changed={routes_folder}",
       routes_folder = "my/routes" // Replace with your actual routes dir
   );
}
```
*/
#![forbid(unsafe_code)]
#![cfg_attr(feature = "nightly", feature(proc_macro_tracked_path))]

#[cfg(feature = "nightly")]
use proc_macro::tracked;
use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::parse_macro_input;

mod generate;
mod parse;

/// Creates an Axum router module tree & creation function
/// by scanning a directory for `route.rs` files.
///
/// # Parameters
///
/// * `path` - A string literal pointing to the route directory, relative to the
///   Cargo manifest directory
/// * `state_type` - The type name of your application state that will be shared
///   across all routes
#[allow(clippy::missing_panics_doc)]
#[proc_macro_attribute]
pub fn folder_router(attr: TokenStream, item: TokenStream) -> TokenStream {
    #[cfg(feature = "debug")]
    println!(
        "/// [folder_router] Running folder_router macro attrs:({}) item: {}",
        attr, item
    );

    let mut errors = TokenStream2::new();

    let args = parse_macro_input!(attr as parse::FolderRouterArgs);

    #[cfg(feature = "nightly")]
    {
        #[cfg(feature = "debug")]
        println!(
            "/// [folder_router] Tracking path: {:?}",
            args.abs_path()
        );
        tracked::path(&*args.abs_path().to_string_lossy());
    }

    let item = parse_macro_input!(item as parse::FolderRouterItem);
    let routes = parse::FolderRouterRoutes::parse_from_path(&mut errors, &args.abs_path());

    let module_tree = generate::module_tree(&args, &item, &routes);
    let router_impl = generate::router_impl(&mut errors, &args, &item, &routes);

    quote! {
      #item
      #errors
      #module_tree
      #router_impl
    }
    .into()
}
