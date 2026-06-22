//! `cargo run --example openapi --features openapi`
//!
//! Demonstrates the `openapi` flag: the macro generates `Api::openapi()`
//! alongside the router, returning a `utoipa::openapi::OpenApi` built from the
//! route tree. Schemas come from `#[derive(ToSchema)]` on the handler types.
//!
//! Note the schema/extractor types (`CreateUser`, `User`) are imported *here*,
//! at the `#[folder_router]` site, so the generated `openapi()` can name them —
//! the same scoping rule that applies to `intercept.rs` extractors.

use axum_folder_router::folder_router;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Clone)]
pub struct AppState;

#[derive(Serialize, Deserialize, ToSchema)]
pub struct CreateUser {
    pub name: String,
}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct User {
    pub id: u64,
    pub name: String,
}

#[folder_router("examples/openapi/api", AppState, openapi)]
struct Api();

#[tokio::main]
async fn main() {
    // The router itself is built exactly as without the flag…
    let _app = Api::into_router_with_state(AppState);

    // …and the document is a separate, state-free constructor you can serve
    // (e.g. via `utoipa-swagger-ui`) or, here, just print.
    let mut doc = Api::openapi();
    doc.info = utoipa::openapi::InfoBuilder::new()
        .title("Folder Router Demo")
        .version("0.1.0")
        .build();

    println!("{}", doc.to_pretty_json().unwrap());
}
