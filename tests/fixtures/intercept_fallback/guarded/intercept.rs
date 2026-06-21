use std::ops::ControlFlow;

use axum::{
    extract::Request,
    http::StatusCode,
    response::{IntoResponse, Response},
};

// Intercept-only folder: no `route.rs` here. It still makes `guarded/` a nested
// boundary, and the intercept runs over the inherited fallback-served path —
// diverting anything containing "blocked", letting the rest reach the fallback.
// (Path is matched with `contains` so it's robust to the nest prefix strip.)
pub async fn intercept(req: Request) -> ControlFlow<Response, Request> {
    if req.uri().path().contains("blocked") {
        return ControlFlow::Break((StatusCode::FORBIDDEN, "guard").into_response());
    }
    ControlFlow::Continue(req)
}
