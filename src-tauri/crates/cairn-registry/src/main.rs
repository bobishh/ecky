use std::{path::PathBuf, process::ExitCode};
#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("cairn-registry: {error}");
            ExitCode::FAILURE
        }
    }
}
async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let root =
        PathBuf::from(std::env::var("CAIRN_DATA_DIR").unwrap_or_else(|_| "./cairn-data".into()));
    let token = match std::env::var("CAIRN_PUBLISH_TOKEN_FILE") {
        Ok(path) => std::fs::read_to_string(path)?.trim().to_string(),
        Err(_) => std::env::var("CAIRN_PUBLISH_TOKEN")?,
    };
    let address = std::env::var("CAIRN_LISTEN").unwrap_or_else(|_| "127.0.0.1:8247".into());
    let assets = std::env::var_os("CAIRN_STATIC_DIR").map(PathBuf::from);
    let app = cairn_registry::router_with_assets(&root, &token, assets.as_deref())
        .map_err(std::io::Error::other)?;
    let listener = tokio::net::TcpListener::bind(&address).await?;
    eprintln!("Cairn listening on {}", listener.local_addr()?);
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
