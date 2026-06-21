use axum::response::IntoResponse;

pub async fn get() -> impl IntoResponse {
    "root"
}

// Never reached for POST: the intercept diverts it before routing. Present only
// so `/` registers a POST method too.
pub async fn post() -> impl IntoResponse {
    "posted"
}
