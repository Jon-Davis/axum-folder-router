use axum_folder_router::folder_router;

#[derive(Clone)]
struct AppState;

#[folder_router("../../../../tests/failures/intercept_no_request", AppState)]
struct MyFolderRouter();

fn main() {}
