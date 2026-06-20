use axum::{http::StatusCode, Router};

use crate::AppState;

// State-aware (2-arg) form: the macro passes a clone of the app state value. We
// bake `marker` into the fallback handler at construction time, proving the
// state value actually reached this function (independent of any extractor).
pub fn fallback(router: Router<AppState>, state: AppState) -> Router<AppState> {
    let marker = state.marker;
    router.fallback(move || {
        let marker = marker.clone();
        async move { (StatusCode::NOT_FOUND, marker) }
    })
}
