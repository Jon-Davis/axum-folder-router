use std::ops::ControlFlow;

use axum::{extract::Request, http::StatusCode, response::{IntoResponse, Response}};

// Missing the required `Request` parameter: an intercept must take (and forward)
// the request. The macro rejects this at compile time.
pub async fn intercept() -> ControlFlow<Response, Request> {
    ControlFlow::Break(StatusCode::FORBIDDEN.into_response())
}
