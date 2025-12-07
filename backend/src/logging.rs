use crate::config::Config;

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

/// Keep guards alive for the lifetime of the app.
pub struct LogGuards {
    _file_guard: Option<WorkerGuard>,
}

fn split_path(path: &Path) -> (PathBuf, String) {
    let dir = path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();
    let file = path
        .file_name()
        .unwrap_or_else(|| OsStr::new("blaz.log"))
        .to_string_lossy()
        .to_string();
    (dir, file)
}

pub fn init_logging(config: &Config) -> LogGuards {
    let filter = EnvFilter::new(config.log_filter());

    // Stdout layer (pretty enough, ANSI enabled)
    let stdout_layer = fmt::layer()
        .with_target(false)
        .with_ansi(true)
        .compact()
        // requires tracing-subscriber "chrono" feature
        .with_timer(tracing_subscriber::fmt::time::ChronoLocal::new(
            "%Y-%m-%d %H:%M:%S".to_string(),
        ));

    // Optional file layer (ANSI disabled)
    let (file_layer, guard) = {
        // Works whether `log_file` is a `PathBuf` or a `&Path`
        let path: &Path = config.log_file.as_ref();

        let (dir, file) = split_path(path);
        let appender = tracing_appender::rolling::never(dir, file);
        let (nb, guard) = tracing_appender::non_blocking(appender);

        let layer = fmt::layer()
            .with_target(false)
            .with_ansi(false)
            .compact()
            .with_timer(tracing_subscriber::fmt::time::ChronoLocal::new(
                "%Y-%m-%d %H:%M:%S".to_string(),
            ))
            .with_writer(nb);

        (Some(layer), Some(guard))
    };

    let subscriber = tracing_subscriber::registry()
        .with(filter)
        .with(stdout_layer);

    if let Some(file_layer) = file_layer {
        subscriber.with(file_layer).init();
    } else {
        subscriber.init();
    }

    LogGuards { _file_guard: guard }
}
