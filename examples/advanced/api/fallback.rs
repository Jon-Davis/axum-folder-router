use axum::Router;
use tower_http::services::ServeDir;

use crate::server::AppState;

// `fallback.rs` declares the fallback for this folder's subtree. The macro hands
// us the composed `Router` and we choose how to attach it — here `fallback_service`
// with a `ServeDir`, so any request that doesn't match a `route.rs` is served from
// the `examples/advanced/static` directory (e.g. `/index.html`). Nested folders
// with no `fallback.rs` of their own inherit this one, so `/users/...` falls back
// here too (behind that subtree's intercept).
pub fn fallback(router: Router<AppState>) -> Router<AppState> {
    router.fallback_service(ServeDir::new("examples/advanced/static"))
}
