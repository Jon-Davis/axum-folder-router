use axum::{response::IntoResponse, Extension};

// Echoes the value the intercept inserted (or a sentinel if absent). `Option<_>`
// so a missing extension is a clean fallthrough, not a 500.
pub async fn get(ext: Option<Extension<crate::Injected>>) -> impl IntoResponse {
    match ext {
        Some(Extension(crate::Injected(v))) => v,
        None => "no-extension".to_string(),
    }
}
