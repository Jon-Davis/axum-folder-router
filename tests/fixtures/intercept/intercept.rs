use std::ops::ControlFlow;

use axum::{
    extract::Request,
    http::StatusCode,
    response::{IntoResponse, Response},
};

// Stateless (1-arg) form. Diverts `/blocked`; otherwise augments the request and
// proceeds.
pub async fn intercept(mut req: Request) -> ControlFlow<Response, Request> {
    if req.uri().path() == "/blocked" {
        return ControlFlow::Break((StatusCode::FORBIDDEN, "blocked").into_response());
    }
    req.extensions_mut().insert(crate::Injected("seen".to_string()));
    ControlFlow::Continue(req)
}
