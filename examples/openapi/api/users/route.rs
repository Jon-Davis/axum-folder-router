use axum::Json;

use crate::{CreateUser, User};

/// Create a user.
///
/// Consumes a `CreateUser` body and returns the created `User`; both are
/// registered as component schemas via their `ToSchema` derives.
pub async fn post(Json(body): Json<CreateUser>) -> Json<User> {
    Json(User {
        id: 1,
        name: body.name,
    })
}
