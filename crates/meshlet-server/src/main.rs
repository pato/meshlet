use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use tokio::sync::Mutex;

use meshlet_server::{app_router, AppState, ServerDoc};

#[derive(Parser)]
#[command(name = "meshlet-server", version)]
struct Cli {
    /// Address to bind to
    #[arg(long, default_value = "127.0.0.1:3000")]
    bind: SocketAddr,

    /// Bearer token for authentication (optional)
    #[arg(long)]
    token: Option<String>,

    /// Data directory for state persistence
    #[arg(long)]
    data_dir: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    let data_dir = cli
        .data_dir
        .unwrap_or_else(|| dirs::data_dir().unwrap_or_default().join("meshlet-server"));

    let server_doc = ServerDoc::load_or_create(&data_dir);
    tracing::info!("server started");

    let state = Arc::new(AppState {
        doc: Mutex::new(server_doc),
        token: cli.token,
        data_dir,
    });

    let app = app_router(state);

    let listener = tokio::net::TcpListener::bind(cli.bind).await?;
    tracing::info!("listening on {}", cli.bind);

    axum::serve(listener, app).await?;

    Ok(())
}
