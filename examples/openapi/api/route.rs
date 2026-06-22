use axum::response::IntoResponse;

/// Health check.
///
/// A bare `impl IntoResponse` is opaque to the macro, so this operation is
/// documented with a bodyless `200`.
pub async fn get() -> impl IntoResponse {
    "ok"
}
