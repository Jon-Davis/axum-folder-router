use std::ops::ControlFlow;

use axum::{extract::Request, response::Response};

// `Request` is not the last parameter (an extractor follows it). axum requires
// the body-consuming extractor last, so the macro rejects this at compile time.
pub async fn intercept(
    req: Request,
    _method: axum::http::Method,
) -> ControlFlow<Response, Request> {
    ControlFlow::Continue(req)
}
