use axum::response::IntoResponse;

// Lives under the `secret/` middleware subtree.
pub async fn get() -> impl IntoResponse {
    "secret"
}
