use axum::{
    extract::{Request, State},
    middleware::{from_fn_with_state, Next},
    response::Response,
    Router,
};

// `crate::AppState` resolves to the state type defined in the integration test
// crate that invokes the macro — exactly how a real app would `use crate::AppState`.
use crate::AppState;

// Reads the state value (the `marker` passed at construction) and echoes it as a
// header, proving the app state actually reaches this middleware.
async fn echo_marker(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    response.headers_mut().insert(
        "x-state-marker",
        axum::http::HeaderValue::from_str(&state.marker).unwrap(),
    );
    response
}

// State-aware (two-argument) form: the macro hands us a clone of the app state,
// which `from_fn_with_state` needs at layer-build time.
pub fn middleware(router: Router<AppState>, state: AppState) -> Router<AppState> {
    router.route_layer(from_fn_with_state(state, echo_marker))
}
