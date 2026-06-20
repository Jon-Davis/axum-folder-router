use axum_folder_router::folder_router;

#[derive(Clone)]
struct AppState;

#[folder_router("../../../../tests/failures/nest_catchall", AppState)]
struct MyFolderRouter();

fn main() {}
