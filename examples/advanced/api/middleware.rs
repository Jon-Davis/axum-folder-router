use axum::{extract::Request, middleware::Next, response::Response, Router};

use crate::server::AppState;

// Runs for every request routed into this folder's subtree — i.e. the whole app,
// since this `middleware.rs` lives at the router root. It tags each response with a
// marker header so you can see the layer ran.
async fn add_marker(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    response.headers_mut().insert(
        "x-folder-router-mw",
        axum::http::HeaderValue::from_static("root"),
    );
    response
}

// `middleware.rs` is picked up by the macro and applied to this folder's subtree.
// The macro calls this function with the subtree's `Router`, and you decide how to
// attach the middleware (`.layer` vs `.route_layer`, stacking, etc.). Keeping it
// concrete `Router<AppState>`; make it generic over `S` instead if the middleware
// is state-agnostic and you want it to work regardless of the app state type.
pub fn middleware(router: Router<AppState>) -> Router<AppState> {
    router.layer(axum::middleware::from_fn(add_marker))
}
