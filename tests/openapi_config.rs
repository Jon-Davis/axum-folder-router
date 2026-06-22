//! Exercises per-directory `openapi.toml` config: include scoping and
//! tag / auto_tag grouping.
//!
//! Run with: `cargo test --features openapi --test openapi_config`

#![cfg(feature = "openapi")]

use axum_folder_router::folder_router;

#[derive(Clone)]
struct AppState;

#[folder_router("tests/fixtures/openapi_config", AppState, openapi)]
struct ConfigDocs();

fn doc_json() -> serde_json::Value {
    let doc = ConfigDocs::openapi();
    serde_json::to_value(doc).unwrap()
}

// ---------- include / exclude scoping ----------

#[test]
fn api_subtree_is_included() {
    let v = doc_json();
    let paths = &v["paths"];
    assert!(paths.get("/api").is_some(), "/api must be present");
    assert!(paths.get("/api/users").is_some(), "/api/users must be present");
    assert!(paths.get("/api/hello").is_some(), "/api/hello must be present");
}

#[test]
fn root_excluded_paths_are_absent() {
    let v = doc_json();
    let paths = &v["paths"];
    assert!(paths.get("/health").is_none(), "/health must be absent (root include=false)");
    assert!(paths.get("/auth").is_none(), "/auth must be absent (root include=false)");
}

#[test]
fn internal_subtree_is_hidden_but_public_re_enabled() {
    let v = doc_json();
    let paths = &v["paths"];
    assert!(
        paths.get("/api/internal").is_none(),
        "/api/internal must be absent (include=false)"
    );
    assert!(
        paths.get("/api/internal/public").is_some(),
        "/api/internal/public must be present (include=true overrides parent)"
    );
}

// ---------- auto_tag and tag override ----------

#[test]
fn auto_tag_derives_child_segment() {
    let v = doc_json();
    let tags = &v["paths"]["/api/users"]["get"]["tags"];
    assert_eq!(
        tags[0], "users",
        "/api/users GET must be tagged 'users' via auto_tag at /api"
    );
}

#[test]
fn explicit_tag_overrides_auto_tag() {
    let v = doc_json();
    let tags = &v["paths"]["/api/hello"]["get"]["tags"];
    assert_eq!(
        tags[0], "Greetings",
        "/api/hello GET must be tagged 'Greetings' via explicit tag override"
    );
}

#[test]
fn api_root_has_no_tag() {
    let v = doc_json();
    // /api route.rs sits directly in the auto_tag config dir, no segment below
    // it — falls back to cfg.tag which is None.
    let tags = &v["paths"]["/api"]["get"]["tags"];
    assert!(
        tags.is_null() || tags.as_array().map_or(true, |a| a.is_empty()),
        "/api GET must have no tags; got: {tags}"
    );
}

#[test]
fn re_enabled_public_route_gets_auto_tag() {
    let v = doc_json();
    // /api/internal/public: include=true from public/, tagging walks up to /api
    // (auto_tag=true), picks dir["api","internal","public"][1] = "internal".
    let tags = &v["paths"]["/api/internal/public"]["get"]["tags"];
    assert_eq!(
        tags[0], "internal",
        "/api/internal/public GET must be tagged 'internal' via auto_tag at /api"
    );
}
