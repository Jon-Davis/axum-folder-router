//! Exercises the `openapi` feature: the macro emits an `openapi()` constructor
//! that builds a `utoipa::openapi::OpenApi` from the route tree.
//!
//! Run with: `cargo test --features openapi --test openapi`

#![cfg(feature = "openapi")]

use axum_folder_router::folder_router;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Serialize, Deserialize, ToSchema)]
pub struct CreateUser {
    pub name: String,
}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct User {
    pub id: u64,
    pub name: String,
}

#[derive(Clone)]
struct AppState;

#[folder_router("tests/fixtures/openapi", AppState, openapi)]
struct ApiDocs();

fn doc_json() -> serde_json::Value {
    let doc = ApiDocs::openapi();
    serde_json::to_value(doc).unwrap()
}

#[test]
fn registers_paths_and_methods() {
    let v = doc_json();
    let paths = &v["paths"];
    assert!(paths.get("/").is_some(), "root path present");
    assert!(paths.get("/users").is_some(), "/users present");
    assert!(
        paths.get("/users/{id}").is_some(),
        "path param converted to {{id}}: {paths}"
    );

    assert!(paths["/"].get("get").is_some());
    // `/users` carries two verbs from one `route.rs`: both must land on the same
    // PathItem (regression guard — chaining `.operation()` for the 2nd verb only
    // compiles on `PathItemBuilder`, not `PathItem::new(..)`).
    assert!(paths["/users"].get("get").is_some());
    assert!(paths["/users"].get("post").is_some());
    assert!(paths["/users/{id}"].get("get").is_some());
}

#[test]
fn doc_comments_become_summary_and_description() {
    let v = doc_json();
    assert_eq!(v["paths"]["/"]["get"]["summary"], "List the API root.");
    assert!(v["paths"]["/"]["get"]["description"]
        .as_str()
        .unwrap()
        .contains("Returns a short greeting"));
    assert_eq!(
        v["paths"]["/users"]["post"]["summary"],
        "Create a user."
    );
}

#[test]
fn request_body_and_response_schemas_are_referenced() {
    let v = doc_json();

    // POST /users consumes CreateUser, returns User.
    let req_ref = &v["paths"]["/users"]["post"]["requestBody"]["content"]
        ["application/json"]["schema"]["$ref"];
    assert_eq!(req_ref, "#/components/schemas/CreateUser");

    let resp_ref = &v["paths"]["/users"]["post"]["responses"]["200"]["content"]
        ["application/json"]["schema"]["$ref"];
    assert_eq!(resp_ref, "#/components/schemas/User");

    // Both component schemas are registered.
    let schemas = &v["components"]["schemas"];
    assert!(schemas.get("CreateUser").is_some());
    assert!(schemas.get("User").is_some());

    // GET /users returns `Json<Vec<User>>`: the response is an inline array of
    // `User` refs, not a `$ref` to a bogus "Vec" component.
    let list = &v["paths"]["/users"]["get"]["responses"]["200"]["content"]
        ["application/json"]["schema"];
    assert_eq!(list["type"], "array");
    assert_eq!(list["items"]["$ref"], "#/components/schemas/User");
    assert!(
        schemas.get("Vec").is_none(),
        "Vec<T> must not register a component named 'Vec': {schemas}"
    );
}

#[test]
fn path_parameter_is_declared() {
    let v = doc_json();
    let params = v["paths"]["/users/{id}"]["get"]["parameters"]
        .as_array()
        .expect("parameters array");
    let id = params
        .iter()
        .find(|p| p["name"] == "id")
        .expect("id parameter present");
    assert_eq!(id["in"], "path");
    assert_eq!(id["required"], true);
}
