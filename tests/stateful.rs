//! Runtime tests for state-aware (`router, state`) middleware, which is wired
//! through the generated `into_router_with_state` entry point.

use axum::{body::Body, http::Request, Router};
use axum_folder_router::folder_router;
use tower::ServiceExt; // for `oneshot`

#[derive(Clone)]
pub struct AppState {
    pub marker: String,
}

#[folder_router("tests/fixtures/stateful", AppState)]
struct StatefulRouter();

// Because `secret/middleware.rs` is state-aware, only `into_router_with_state`
// is generated; it threads the state into the middleware and applies
// `.with_state` for us, so this returns a ready-to-serve `Router`.
fn app(marker: &str) -> Router {
    StatefulRouter::into_router_with_state(AppState {
        marker: marker.to_owned(),
    })
}

async fn marker_header(app: Router, uri: &str) -> Option<String> {
    let response = app
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    response
        .headers()
        .get("x-state-marker")
        .map(|v| v.to_str().unwrap().to_owned())
}

#[tokio::test]
async fn stateful_middleware_receives_the_app_state_value() {
    // The header value can only come from the state passed to into_router_with_state.
    assert_eq!(
        marker_header(app("hello-state"), "/secret").await.as_deref(),
        Some("hello-state")
    );
}

#[tokio::test]
async fn stateful_middleware_is_scoped_to_its_subtree() {
    // `/` is outside the `secret/` subtree, so the middleware must not touch it.
    assert_eq!(marker_header(app("hello-state"), "/").await, None);
}
