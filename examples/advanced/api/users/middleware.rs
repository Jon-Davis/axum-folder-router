use axum::{extract::Request, middleware::Next, response::Response, Router};

// Runs for every request routed under `/users` (including `/users/{id}`),
// but not for routes outside this folder.
async fn add_marker(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    response.headers_mut().insert(
        "x-folder-router-mw",
        axum::http::HeaderValue::from_static("users"),
    );
    response
}

// `middleware.rs` is picked up by the macro and applied to this folder's
// subtree. The macro calls this function with the subtree's `Router`, and you
// decide how to attach your middleware (`.layer` vs `.route_layer`, stacking,
// etc.). Keeping it generic over the state `S` means it works regardless of the
// app state type; pin it to a concrete state if your middleware needs to read it.
pub fn middleware<S>(router: Router<S>) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    router.route_layer(axum::middleware::from_fn(add_marker))
}
