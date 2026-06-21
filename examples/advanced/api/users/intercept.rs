use std::ops::ControlFlow;

use axum::{
    extract::Request,
    http::{StatusCode, header::AUTHORIZATION},
    response::{IntoResponse, Response},
};

// `intercept.rs` guards this folder's subtree (`/users` and `/users/{id}`). You
// write only the decision; the macro generates the layer and always attaches it
// with `.layer`, so it gates the routes *and* the inherited fallback.
//
// Parameters are axum extractors, just like a handler's — here `HeaderMap` to read
// the `Authorization` header — with one rule: the forwarded `Request` is always the
// last parameter. Return `Continue(req)` to proceed (optionally after mutating the
// request) or `Break(resp)` to short-circuit.
//
// The macro reproduces an intercept's parameter types at its invocation site, so
// any extractor other than `Request`/`State<_>` must be named by a path that
// resolves there — hence the fully qualified `axum::http::HeaderMap`.
//
// This is a toy guard: a request is allowed through only with the header
// `Authorization: Bearer password`. Anything else gets a 401.
pub async fn intercept(
    headers: axum::http::HeaderMap,
    req: Request,
) -> ControlFlow<Response, Request> {
    let authorized = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        == Some("password");

    if authorized {
        ControlFlow::Continue(req)
    } else {
        ControlFlow::Break((StatusCode::UNAUTHORIZED, "unauthorized\n").into_response())
    }
}
