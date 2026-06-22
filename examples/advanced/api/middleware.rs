use axum::{extract::Request, middleware::Next, response::Response, Router};

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
// attach the middleware (`.layer` vs `.route_layer`, stacking, etc.). This one is
// state-agnostic, so it's generic over `S` and works regardless of the app state
// type; make it concrete `Router<AppState>` instead if it needs the state type.
pub fn middleware<S>(router: Router<S>) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    router.layer(axum::middleware::from_fn(add_marker))
}
