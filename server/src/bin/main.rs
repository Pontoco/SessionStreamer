// src/main.rs (Simplified Example)
use anyhow::Result;
use tokio::net::TcpListener;
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

    // Call the library function to get the router
    let app = server::create_server()?; // Potentially pass args here: create_server(args)?

    let listener = TcpListener::bind("0.0.0.0:3000").await?;
    info!("Listening on {}", listener.local_addr()?);

    axum::serve(listener, app).await?;

    Ok(())
}
