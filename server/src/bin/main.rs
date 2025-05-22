// src/main.rs (Simplified Example)
use anyhow::Result;
use tokio::{fs, io, net::TcpListener};
use tracing::info;

use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
struct CommandLineArgs {
    #[arg(long, default_value = "./data/")]
    pub data_path: PathBuf,
    #[arg(long)]
    pub use_structured_logging: bool,
    
    #[arg(long, default_value = "../client/dist")]
    pub client_files: PathBuf
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = CommandLineArgs::parse(); // If needed, pass args.data_path to create_server

    server::configure_logging(args.use_structured_logging);

    info!("Starting server...");
    info!("CLI Args: [{:?}]", args);

    if let Err(err) = fs::create_dir(&args.data_path).await {
        if err.kind() != io::ErrorKind::AlreadyExists {
            panic!("Couldn't create directory [{:?}]", &args.data_path);
        }
    }

    // Call the library function to get the router
    let app = server::create_server(args.data_path, args.client_files)?; // Potentially pass args here: create_server(args)?

    let listener = TcpListener::bind("0.0.0.0:3000").await?;
    info!("Listening on {}", listener.local_addr()?);

    axum::serve(listener, app).await?;

    Ok(())
}
