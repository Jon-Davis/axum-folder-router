use axum::response::IntoResponse;

pub async fn get() -> impl IntoResponse {
    "hello"
}
