use axum::{extract::Path, Json};

use crate::User;

/// Fetch a single user by id.
pub async fn get(Path(id): Path<u64>) -> Json<User> {
    Json(User {
        id,
        name: "alice".to_string(),
    })
}
