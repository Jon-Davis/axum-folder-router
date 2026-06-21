use axum_folder_router::folder_router;

#[derive(Clone)]
struct AppState;

#[folder_router("../../../../tests/failures/intercept_request_not_last", AppState)]
struct MyFolderRouter();

fn main() {}
