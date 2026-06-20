use axum::{http::StatusCode, response::IntoResponse, Router};

async fn not_found() -> impl IntoResponse {
    (StatusCode::NOT_FOUND, "custom fallback")
}

// Stateless (1-arg) form — the macro hands us the fully-composed router and we
// decide how to attach the fallback.
pub fn fallback<S>(router: Router<S>) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    router.fallback(not_found)
}
