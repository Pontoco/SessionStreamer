// src/main.rs (Simplified Example)
use anyhow::Result;
use tokio::{fs, io, net::TcpListener};
use tracing::info;

use clap::Parser;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};
use std::{env, path::PathBuf};

#[derive(Parser, Debug)]
struct CommandLineArgs {
    #[arg(default_value = "./data/")]
    pub data_path: PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
    server::default_tracing_registry();

    info!("Starting server...");
    let args = CommandLineArgs::parse(); // If needed, pass args.data_path to create_server

    if let Err(err) = fs::create_dir(&args.data_path).await {
        if err.kind() != io::ErrorKind::AlreadyExists {
            panic!("Couldn't create directory [{:?}]", &args.data_path);
        }
    }

    // Call the library function to get the router
    let app = server::create_server(args.data_path)?; // Potentially pass args here: create_server(args)?

    let listener = TcpListener::bind("0.0.0.0:3000").await?;
    info!("Listening on {}", listener.local_addr()?);

    axum::serve(listener, app).await?;

    Ok(())
}
