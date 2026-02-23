use std::sync::OnceLock;

static NTFY_URL: OnceLock<Option<String>> = OnceLock::new();

pub fn init(url: Option<String>) {
    NTFY_URL.set(url).ok();
}

pub fn notify(msg: &str) {
    let Some(Some(url)) = NTFY_URL.get() else {
        return;
    };
    let url = url.clone();
    let msg = msg.to_string();
    tokio::spawn(async move {
        if let Err(e) = reqwest::Client::new().post(&url).body(msg).send().await {
            tracing::warn!("Failed to send ntfy notification: {e}");
        }
    });
}
