use std::ops::ControlFlow;

use axum::{
    extract::Request,
    http::{Method, StatusCode},
    response::{IntoResponse, Response},
};

// Demonstrates a `FromRequestParts` extractor (`Method`) alongside the forwarded
// `Request`: the macro reproduces the extractor on the generated layer. The type
// is written fully qualified in the signature so it resolves at the
// `#[folder_router]` invocation site (which need not import `Method`).
pub async fn intercept(
    method: axum::http::Method,
    req: Request,
) -> ControlFlow<Response, Request> {
    if method == Method::POST {
        return ControlFlow::Break((StatusCode::FORBIDDEN, "no-post").into_response());
    }
    ControlFlow::Continue(req)
}
