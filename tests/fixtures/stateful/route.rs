use axum::response::IntoResponse;

// Public root route, outside any middleware subtree.
pub async fn get() -> impl IntoResponse {
    "root"
}
