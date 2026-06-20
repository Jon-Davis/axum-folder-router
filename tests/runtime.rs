//! Runtime integration tests that boot the generated router and send requests
//! through it, exercising behaviour the expand/trybuild snapshots can't.

use axum::{body::Body, http::Request, Router};
use axum_folder_router::folder_router;
use tower::ServiceExt; // for `oneshot`

#[derive(Clone)]
struct AppState {
    _foo: String,
}

#[folder_router("examples/advanced/api", AppState)]
struct MyFolderRouter();

fn app() -> Router {
    MyFolderRouter::into_router().with_state(AppState {
        _foo: String::new(),
    })
}

async fn marker_header(app: Router, uri: &str) -> Option<String> {
    let response = app
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    response
        .headers()
        .get("x-folder-router-mw")
        .map(|v| v.to_str().unwrap().to_owned())
}

async fn body_string(app: Router, uri: &str) -> String {
    let response = app
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    String::from_utf8(bytes.to_vec()).unwrap()
}

#[tokio::test]
async fn path_param_route_matches_and_extracts() {
    // `[id]` routes a dynamic segment and the handler extracts it. (The capture
    // must be generated as `{id}`, not `{:id}`; the exact form is pinned by the
    // expand snapshot, since positional `Path<String>` extraction here matches
    // regardless of the capture name.)
    assert_eq!(body_string(app(), "/users/42").await, "User ID: 42");
}

#[tokio::test]
async fn catch_all_param_is_extracted() {
    assert_eq!(
        body_string(app(), "/files/a/b/c.txt").await,
        "Requested file path: a/b/c.txt"
    );
}

#[tokio::test]
async fn middleware_applies_to_its_subtree() {
    // `/users` is directly under the folder that owns `middleware.rs`.
    assert_eq!(
        marker_header(app(), "/users").await.as_deref(),
        Some("users")
    );
}

#[tokio::test]
async fn middleware_applies_to_nested_routes() {
    // `/users/{id}` is deeper in the same subtree, so it's covered too.
    assert_eq!(
        marker_header(app(), "/users/42").await.as_deref(),
        Some("users")
    );
}

#[tokio::test]
async fn middleware_is_scoped_out_of_sibling_subtrees() {
    // `/` and `/ping` live outside the `/users` folder and must be untouched.
    assert_eq!(marker_header(app(), "/").await, None);
    assert_eq!(marker_header(app(), "/ping").await, None);
}

#[tokio::test]
async fn routes_without_middleware_still_respond() {
    let response = app()
        .oneshot(Request::builder().uri("/ping").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);
}
