//! Runtime tests for the root `fallback.rs` convention, in both its stateless
//! and state-aware forms.

use axum::{body::Body, http::Request, http::StatusCode, Router};
use axum_folder_router::folder_router;
use tower::ServiceExt; // for `oneshot`

#[derive(Clone)]
pub struct AppState {
    pub marker: String,
}

#[folder_router("tests/fixtures/fallback", AppState)]
struct StatelessFallback();

#[folder_router("tests/fixtures/fallback_stateful", AppState)]
struct StatefulFallback();

async fn get(app: Router, uri: &str) -> (StatusCode, String) {
    let response = app
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, String::from_utf8(bytes.to_vec()).unwrap())
}

#[tokio::test]
async fn stateless_fallback_serves_unmatched_requests() {
    let app = StatelessFallback::into_router().with_state(AppState {
        marker: String::new(),
    });
    let (status, body) = get(app, "/does-not-exist").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body, "custom fallback");
}

#[tokio::test]
async fn known_routes_take_precedence_over_fallback() {
    let app = StatelessFallback::into_router().with_state(AppState {
        marker: String::new(),
    });
    let (status, body) = get(app, "/").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "root");
}

#[tokio::test]
async fn stateful_fallback_receives_the_app_state_value() {
    // Only `into_router_with_state` exists here (stateful fallback gates
    // `into_router` off). The 404 body can only come from the state we pass in.
    let app = StatefulFallback::into_router_with_state(AppState {
        marker: "from-state".to_owned(),
    });
    let (status, body) = get(app, "/anything").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body, "from-state");
}
