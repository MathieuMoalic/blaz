use clap::{ArgAction, Parser};
use std::{net::SocketAddr, path::PathBuf};

/// Blaz server configuration
#[derive(Parser, Debug, Clone)]
#[command(name = "blaz-server", version, about = "HTTP API server for Blaz")]
pub struct Config {
    /// Increase verbosity (-v, -vv, -vvv)
    #[arg(short = 'v', action = ArgAction::Count, global = true)]
    pub verbose: u8,

    /// Decrease verbosity (-q, -qq, -qqq)
    #[arg(short = 'q', action = ArgAction::Count, global = true)]
    pub quiet: u8,

    /// Address to bind the HTTP server to
    #[arg(long, env = "BLAZ_BIND_ADDR", default_value = "0.0.0.0:8080")]
    pub bind: SocketAddr,

    /// Directory to store media files
    #[arg(long, env = "BLAZ_MEDIA_DIR", default_value = "media")]
    pub media_dir: PathBuf,

    /// Database path
    #[arg(long, env = "BLAZ_DATABASE_PATH", default_value = "blaz.sqlite")]
    pub database_path: String,

    /// Optional log file path (logs are written to stdout + this file)
    #[arg(long, env = "BLAZ_LOG_FILE", default_value = "blaz.logs")]
    pub log_file: PathBuf,

    /// CORS allowed origin (e.g., <https://blaz.yourdomain.com>)
    /// If not set, allows all origins (⚠️ insecure for production!)
    #[arg(long, env = "BLAZ_CORS_ORIGIN")]
    pub cors_origin: Option<String>,
}

impl Config {
    #[must_use]
    pub fn verbosity_delta(&self) -> i16 {
        i16::from(self.verbose) - i16::from(self.quiet)
    }
    #[must_use]
    pub fn log_filter(&self) -> &'static str {
        match self.verbosity_delta() {
            d if d <= -2 => "error",
            -1 => "warn",
            0 => "info,blaz=info,axum=info,tower_http=info",
            1 => "debug,blaz=debug,axum=info,tower_http=info,sqlx=warn",
            2 => "trace,blaz=trace,axum=debug,tower_http=trace,sqlx=info,hyper=info",
            _ => "trace,blaz=trace,axum=trace,tower_http=trace,sqlx=debug,hyper=debug",
        }
    }
}
