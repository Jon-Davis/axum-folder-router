use axum::Router;
use axum_folder_router::folder_router;

#[derive(Clone, Debug)]
struct AppState {
    _foo: String,
}

// Imports route.rs files & generates the router constructor(s). Because this tree
// has nested boundaries (`users/intercept.rs`), those subtrees are mounted with
// `nest_service`, which bakes the state value in at build time — so only the
// state-taking `into_router_with_state` is generated (not the bare `into_router`).
#[folder_router("examples/advanced/api", AppState)]
struct MyFolderRouter();

pub async fn server() -> anyhow::Result<()> {
    // Create app state
    let app_state = AppState {
        _foo: String::new(),
    };

    // Use the init fn generated above; it consumes the state and returns a ready
    // `Router<()>`.
    let app: Router<()> = MyFolderRouter::into_router_with_state(app_state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    println!("Listening on http://{}", listener.local_addr()?);
    axum::serve(listener, app).await?;
    Ok(())
}
