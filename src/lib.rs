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

## Interception

```middleware.rs``` hands you the ```Router``` and makes you write the layer
plumbing yourself. For the common case — *inspect each request, then either let
it through (optionally after mutating it) or divert it* — place an
```intercept.rs``` in a folder instead. You write only the decision; the macro
generates the layer and always attaches it with ```layer``` (never
```route_layer```), so it runs over the subtree's routes **and** its fallback. A
pure access guard is just an intercept that only ever diverts.

The file exposes a ```pub async fn intercept``` returning
```ControlFlow<Response, Request>```:
- ```ControlFlow::Continue(req)``` — proceed with the (possibly mutated) request.
- ```ControlFlow::Break(response)``` — short-circuit with this response.

Its parameters are **axum extractors**, just like a handler's, with one rule: the
forwarded ```Request``` is the **last** parameter (any extractors come first).

```rust
use std::ops::ControlFlow;
use axum::{extract::Request, http::StatusCode, response::{IntoResponse, Response}};

pub async fn intercept(req: Request) -> ControlFlow<Response, Request> {
    if req.uri().path().ends_with("/secret") {
        return ControlFlow::Break((StatusCode::FORBIDDEN, "nope").into_response());
    }
    ControlFlow::Continue(req)
}
```

Because ```Continue``` carries the request forward, an intercept can also augment
it — e.g. resolve a session and ```req.extensions_mut().insert(principal)``` before
returning ```Continue(req)``` — so downstream handlers read it via
```Extension<_>```. (An intercept only sees the request, never the outgoing
response; reach for ```middleware.rs``` when you need to touch the response on the
way back out.)

### Extractors

Any **```FromRequestParts```** extractor may precede the ```Request```:
```State```, ```Path```, ```Query```, ```Extension```, ```Method```, header
extractors, cookie jars, and so on. Body-consuming **```FromRequest```**
extractors (```Json```, ```Bytes```, ```Form```, ```String```) are *not* usable:
the intercept must forward the ```Request``` intact on ```Continue```, and those
would consume its body. (axum's trait bounds enforce this at compile time.)

The macro reproduces your extractor parameters on the generated layer at the
```#[folder_router]``` invocation site. It fully-qualifies the two types it
recognises — the ```Request``` and ```State<…>``` — but **every other extractor
type must be nameable at the invocation site**: either import it there or write it
fully qualified in the signature (e.g. ```jar: axum_extra::extract::PrivateCookieJar```).

### Intercept that needs state

State is just the ```State<S>``` extractor, exactly as in a handler — its presence
makes the intercept state-aware (the macro wires a ```from_fn_with_state``` layer)
and forces ```into_router_with_state```, just as stateful middleware/fallback does.
Include a ```State<S>``` parameter whenever the intercept — or any other extractor
it uses, such as a cookie jar — needs the app state:
```rust
use std::ops::ControlFlow;
use axum::{extract::{Request, State}, response::Response};

# #[derive(Clone)]
# struct AppState;

pub async fn intercept(State(state): State<AppState>, req: Request) -> ControlFlow<Response, Request> {
    // inspect `state` (DB pool, session store, ...) to authorize the request
    let _ = state;
    ControlFlow::Continue(req)
}
```

The return type is plain ```std::ops::ControlFlow``` — this is a ```proc-macro```
crate, so it can't export a type alias for you. If the signature gets noisy,
declare a one-liner in your own crate:
```rust
use std::ops::ControlFlow;
use axum::{extract::Request, response::Response};

type Intercept = ControlFlow<Response, Request>;
```

Like ```middleware.rs```/```fallback.rs```, an ```intercept.rs``` makes its folder
a boundary and cannot live in a catch-all (```[...rest]```) directory.

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
