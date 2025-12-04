use clap::Parser;
use std::{net::SocketAddr, path::PathBuf};

/// Blaz server configuration
#[derive(Parser, Debug, Clone)]
#[command(
    name = "blaz-server",
    version,
    about = "HTTP API server for Blaz",
    long_about = None
)]
pub struct Config {
    /// Address to bind the HTTP server to
    #[arg(long, env = "BLAZ_BIND_ADDR", default_value = "0.0.0.0:8080")]
    pub bind: SocketAddr,

    /// Directory to store media files
    #[arg(long, env = "BLAZ_MEDIA_DIR", default_value = "media")]
    pub media_dir: PathBuf,

    /// Database URL (optional, can still come from env only)
    #[arg(long, env = "DATABASE_URL")]
    pub database_url: Option<String>,
}

impl Config {
    pub fn from_env() -> Self {
        Self::parse()
    }
}
