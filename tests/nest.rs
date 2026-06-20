//! Runtime tests for `nest`-based composition and per-subtree `fallback.rs`,
//! including the inheritance that stops a deeper fallback from being silently
//! overridden by an ancestor through a fallback-less layer.

use axum::{body::Body, http::Request, Router};
use axum_folder_router::folder_router;
use tower::ServiceExt; // oneshot

#[derive(Clone)]
struct AppState;

#[folder_router("tests/fixtures/nest", AppState)]
struct NestRouter();

fn app() -> Router {
    NestRouter::into_router().with_state(AppState)
}

async fn body(uri: &str) -> String {
    let res = app()
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    String::from_utf8(bytes.to_vec()).unwrap()
}

#[tokio::test]
async fn matched_routes_resolve_at_their_nested_paths() {
    assert_eq!(body("/").await, "root");
    assert_eq!(body("/shop").await, "shop");
    assert_eq!(body("/shop/items").await, "items");
    assert_eq!(body("/admin").await, "admin");
    assert_eq!(body("/admin/secret").await, "secret");
}

#[tokio::test]
async fn root_fallback_serves_unmatched_top_level() {
    assert_eq!(body("/nope").await, "root-fb");
}

#[tokio::test]
async fn subtree_fallback_serves_its_own_unmatched() {
    // `/shop` owns a fallback.
    assert_eq!(body("/shop/nope").await, "shop-fb");
}

#[tokio::test]
async fn subtree_fallback_is_inherited_by_descendants() {
    // `/shop/items` has no fallback of its own -> inherits shop's.
    assert_eq!(body("/shop/items/nope").await, "shop-fb");
}

#[tokio::test]
async fn fallback_less_boundary_inherits_ancestor() {
    // `/admin` has middleware but no fallback -> unmatched falls to the root fb.
    assert_eq!(body("/admin/nope").await, "root-fb");
}

#[tokio::test]
async fn deep_fallback_under_fallback_less_layer_is_not_overridden() {
    // The footgun fixed: `/admin/secret` owns a fallback even though its parent
    // `/admin` has none. It must win, not be clobbered by the root fallback.
    assert_eq!(body("/admin/secret/nope").await, "secret-fb");
}
