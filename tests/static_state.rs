//! Confirms the `nest_service` boundary mounting works when the router's state
//! type is a `&'static` reference (the realistic deployment shape: build the
//! state once, leak it, pass `&'static State` so every per-request `State` clone
//! and every boundary's `with_state` is a pointer copy rather than a deep clone).

use axum::{
    body::Body,
    extract::State,
    http::{Request, StatusCode},
    Router,
};
use axum_folder_router::folder_router;
use tower::ServiceExt;

#[derive(Clone)]
pub struct AppState {
    pub marker: String,
}

// Stateful intercept over a nested fallback boundary, parameterized over a
// `&'static AppState`.
#[folder_router("tests/fixtures/intercept_fallback", &'static AppState)]
struct StaticInterceptOverFallback();

async fn status(app: Router, uri: &str) -> StatusCode {
    app.oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap()
        .status()
}

#[tokio::test]
async fn nest_service_works_with_static_reference_state() {
    let state: &'static AppState = Box::leak(Box::new(AppState {
        marker: String::new(),
    }));
    let app = StaticInterceptOverFallback::into_router_with_state(state);

    // The guard runs on the bare boundary path in both slash forms…
    assert_eq!(status(app.clone(), "/guarded").await, StatusCode::FORBIDDEN);
    assert_eq!(status(app.clone(), "/guarded/").await, StatusCode::FORBIDDEN);
    // …and lets siblings reach the inherited fallback.
    assert_eq!(status(app.clone(), "/elsewhere").await, StatusCode::OK);
    assert_eq!(status(app, "/").await, StatusCode::OK);
}

// The intercept fixture's `intercept.rs` is stateless; this just proves a
// `State<&'static AppState>` extractor resolves against the reference state.
#[allow(dead_code)]
async fn _state_extractor_typechecks(State(_s): State<&'static AppState>) {}
