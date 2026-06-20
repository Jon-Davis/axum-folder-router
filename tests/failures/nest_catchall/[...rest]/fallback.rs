use axum::Router;

// Invalid: a `fallback.rs` in a catch-all directory would have to be nested at a
// wildcard prefix, which axum forbids. The macro must reject this.
pub fn fallback<S>(router: Router<S>) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    router.fallback(|| async { "nope" })
}
