use axum::response::IntoResponse;

/// List the API root.
///
/// Returns a short greeting. This second paragraph becomes the operation
/// description.
pub async fn get() -> impl IntoResponse {
    "hello"
}
