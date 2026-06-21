use axum::{response::IntoResponse, Router};

async fn fb() -> impl IntoResponse {
    "fallback"
}

pub fn fallback<S>(router: Router<S>) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    router.fallback(fb)
}
