//! Runtime integration tests that boot the generated router and send requests
//! through it, exercising behaviour the expand/trybuild snapshots can't.

use axum::{
    body::Body,
    http::{header::AUTHORIZATION, Request, StatusCode},
    Router,
};
use axum_folder_router::folder_router;
use tower::ServiceExt; // for `oneshot`

// The toy credential the `users/intercept.rs` guard accepts.
const AUTH: &str = "Bearer password";

#[derive(Clone)]
struct AppState {
    _foo: String,
}

#[folder_router("examples/advanced/api", AppState)]
struct MyFolderRouter();

fn app() -> Router {
    MyFolderRouter::into_router_with_state(AppState {
        _foo: String::new(),
    })
}

// Send a request, optionally with the `Authorization` header the `users` guard
// wants, and return the marker header (`middleware.rs`), status and body.
async fn send(app: Router, uri: &str, auth: bool) -> (Option<String>, StatusCode, String) {
    let mut builder = Request::builder().uri(uri);
    if auth {
        builder = builder.header(AUTHORIZATION, AUTH);
    }
    let response = app
        .oneshot(builder.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let marker = response
        .headers()
        .get("x-folder-router-mw")
        .map(|v| v.to_str().unwrap().to_owned());
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (marker, status, String::from_utf8(bytes.to_vec()).unwrap())
}

#[tokio::test]
async fn path_param_route_matches_and_extracts() {
    // `[id]` routes a dynamic segment and the handler extracts it. (The capture
    // must be generated as `{id}`, not `{:id}`; the exact form is pinned by the
    // expand snapshot, since positional `Path<String>` extraction here matches
    // regardless of the capture name.) `/users/{id}` is behind the auth guard.
    let (_, status, body) = send(app(), "/users/42", true).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "User ID: 42");
}

#[tokio::test]
async fn catch_all_param_is_extracted() {
    // `/files` has no `intercept.rs`, so the catch-all is reachable without auth.
    let (_, status, body) = send(app(), "/files/a/b/c.txt", false).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "Requested file path: a/b/c.txt");
}

#[tokio::test]
async fn root_middleware_marks_every_response() {
    // `middleware.rs` lives at the router root, so it tags *every* response with
    // `root` — including the fallback-served `/`, an unguarded route, and even the
    // guard's own 401 (the root middleware wraps the `users` intercept).
    assert_eq!(send(app(), "/ping", false).await.0.as_deref(), Some("root"));
    assert_eq!(send(app(), "/files/x", false).await.0.as_deref(), Some("root"));
    assert_eq!(send(app(), "/", false).await.0.as_deref(), Some("root"));
    assert_eq!(send(app(), "/users", false).await.0.as_deref(), Some("root"));
}

#[tokio::test]
async fn users_intercept_gates_its_subtree() {
    // Without the credential, the `users/intercept.rs` guard diverts with 401…
    let (_, status, body) = send(app(), "/users", false).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body, "unauthorized\n");

    // …and with it the route is reached.
    let (_, status, _) = send(app(), "/users", true).await;
    assert_eq!(status, StatusCode::OK);

    // Siblings outside `/users` are untouched by the guard.
    assert_eq!(send(app(), "/ping", false).await.1, StatusCode::OK);
}

#[tokio::test]
async fn intercept_gates_the_bare_boundary_path_in_both_slash_forms() {
    // The trailing-slash fix: `/users` is mounted with `nest_service`, so the guard
    // runs on the bare boundary path with *and* without a trailing slash — neither
    // form can slip past the layer into an ancestor fallback.
    assert_eq!(send(app(), "/users", false).await.1, StatusCode::UNAUTHORIZED);
    assert_eq!(send(app(), "/users/", false).await.1, StatusCode::UNAUTHORIZED);

    // And both forms reach the route once authorized.
    assert_eq!(send(app(), "/users", true).await.1, StatusCode::OK);
    assert_eq!(send(app(), "/users/", true).await.1, StatusCode::OK);
}

#[tokio::test]
async fn routes_without_middleware_still_respond() {
    let (_, status, _) = send(app(), "/ping", false).await;
    assert_eq!(status, StatusCode::OK);
}
