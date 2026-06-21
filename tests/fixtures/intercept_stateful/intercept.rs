use std::ops::ControlFlow;

use axum::{
    extract::{Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};

use crate::AppState;

// State-aware form: a `State<…>` extractor selects the `from_fn_with_state`
// wiring, so the macro hands axum the app state and it's extracted here. The
// diverted body comes from `marker`, proving the state value reached this
// function. `Request` is last so it can be forwarded on `Continue`.
pub async fn intercept(
    State(state): State<AppState>,
    req: Request,
) -> ControlFlow<Response, Request> {
    if req.uri().path() == "/blocked" {
        return ControlFlow::Break((StatusCode::FORBIDDEN, state.marker).into_response());
    }
    ControlFlow::Continue(req)
}
