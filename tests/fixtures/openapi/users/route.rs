use axum::Json;

use crate::{CreateUser, User};

/// List users.
pub async fn get() -> Json<Vec<User>> {
    Json(vec![User {
        id: 1,
        name: "alice".to_string(),
    }])
}

/// Create a user.
pub async fn post(Json(body): Json<CreateUser>) -> Json<User> {
    Json(User {
        id: 1,
        name: body.name,
    })
}
