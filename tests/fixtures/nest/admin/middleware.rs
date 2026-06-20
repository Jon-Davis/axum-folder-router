use axum::Router;

// No-op middleware: its only job here is to make `admin` a boundary (its own
// nested sub-router) WITHOUT a fallback, so we can test that a deeper
// `fallback.rs` under a fallback-less layer is not silently overridden.
pub fn middleware<S>(router: Router<S>) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    router
}
