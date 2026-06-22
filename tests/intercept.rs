//! Runtime tests for the `intercept.rs` convention: per-request interception
//! that can divert (`ControlFlow::Break`) or pass through after mutating the
//! request (`ControlFlow::Continue`), in both its stateless and state-aware
//! forms, and crucially running over a *fallback*-served path (the property that
//! lets it gate a UI subtree that has no `route.rs`).

use axum::{body::Body, http::Request, http::StatusCode, Router};
use axum_folder_router::folder_router;
use tower::ServiceExt; // for `oneshot`

#[derive(Clone)]
pub struct AppState {
    pub marker: String,
}

/// A value an intercept inserts into the request so a downstream handler can
/// prove the `Continue(req)` path carried the mutation forward.
#[derive(Clone)]
pub struct Injected(pub String);

#[folder_router("tests/fixtures/intercept", AppState)]
struct StatelessIntercept();

#[folder_router("tests/fixtures/intercept_stateful", AppState)]
struct StatefulIntercept();

#[folder_router("tests/fixtures/intercept_fallback", AppState)]
struct InterceptOverFallback();

#[folder_router("tests/fixtures/intercept_extractor", AppState)]
struct ExtractorIntercept();

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

fn stateless_app() -> Router {
    StatelessIntercept::into_router().with_state(AppState {
        marker: String::new(),
    })
}

#[tokio::test]
async fn intercept_passes_through_and_augments_the_request() {
    // `/` is allowed; the intercept inserts `Injected("seen")` before proceeding,
    // and the route echoes it — proving `Continue(req)` carried the mutation.
    let (status, body) = get(stateless_app(), "/").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "seen");
}

#[tokio::test]
async fn intercept_diverts_with_break() {
    // `/blocked` has no route, but the intercept runs *before* routing (it's a
    // `.layer`) and short-circuits with its own response.
    let (status, body) = get(stateless_app(), "/blocked").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body, "blocked");
}

#[tokio::test]
async fn stateful_intercept_receives_the_app_state_value() {
    // Only `into_router_with_state` exists (stateful intercept gates `into_router`
    // off). The diverted body can only come from the state value we pass in.
    let app = StatefulIntercept::into_router_with_state(AppState {
        marker: "from-state".to_owned(),
    });
    let (status, body) = get(app, "/blocked").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body, "from-state");
}

#[tokio::test]
async fn stateful_intercept_allows_other_paths() {
    let app = StatefulIntercept::into_router_with_state(AppState {
        marker: "from-state".to_owned(),
    });
    let (status, body) = get(app, "/").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "root");
}

#[tokio::test]
async fn intercept_runs_a_request_parts_extractor() {
    // The intercept takes a `Method` extractor (plus the forwarded `Request`).
    // The macro reproduces it on the generated layer, so the decision can read
    // request parts without manually digging into the `Request`.
    let app = ExtractorIntercept::into_router().with_state(AppState {
        marker: String::new(),
    });

    // GET is allowed through to the route.
    let (status, body) = get(app.clone(), "/").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "root");

    // POST is diverted by the intercept, based on the extracted method.
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(String::from_utf8(bytes.to_vec()).unwrap(), "no-post");
}

#[tokio::test]
async fn intercept_only_folder_gates_the_fallback() {
    // `guarded/` has an `intercept.rs` but no `route.rs`. Unmatched requests under
    // it resolve to the inherited root fallback — and the intercept still runs
    // over that fallback-served path, diverting "blocked" and letting others
    // through to the fallback.
    let app = InterceptOverFallback::into_router_with_state(AppState {
        marker: String::new(),
    });

    let (status, body) = get(app.clone(), "/guarded/blocked").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body, "guard");

    let (status, body) = get(app.clone(), "/guarded/open").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "fallback");

    // The *bare* boundary path (no sub-segment) must also be gated: this is the
    // `/admin` / `/admin/` case where the only handler is the inherited fallback.
    let (status, body) = get(app.clone(), "/guarded").await;
    assert_eq!(status, StatusCode::FORBIDDEN, "bare prefix must hit intercept");
    assert_eq!(body, "guard");

    let (status, body) = get(app.clone(), "/guarded/").await;
    assert_eq!(status, StatusCode::FORBIDDEN, "bare prefix + slash must hit intercept");
    assert_eq!(body, "guard");

    // The intercept is scoped to `guarded/` only; siblings are untouched.
    let (status, body) = get(app.clone(), "/elsewhere").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "fallback");

    let (status, body) = get(app, "/").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "root");
}
