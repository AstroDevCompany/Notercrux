use notercrux_sync_server::{config::Config, serve};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    if std::env::args().any(|a| a == "--help" || a == "-h") {
        println!(
            "notercrux-sync-server\n\nConfigured through environment variables:\n  NCX_BIND            address to listen on (default 127.0.0.1:8787)\n  NCX_DATABASE_URL    PostgreSQL URL; unset runs in memory (data is lost on exit)\n  NCX_DATABASE_CA     PEM file of extra certificate authorities for sslmode=require\n  NCX_BLOB_DIR        directory for encrypted attachments; unset keeps them in memory\n  NCX_REGISTRATION    open | closed | token:<a>,<b>\n  NCX_MAX_DEVICES, NCX_MAX_OP_BYTES, NCX_MAX_BLOB_BYTES, NCX_MAX_OP_SIZE, NCX_MAX_BLOB_SIZE\n  NCX_DEVICE_RATE, NCX_IP_RATE   requests per minute\n  NCX_METRICS_TOKEN   require this bearer token on /metrics\n\nSee docs/self-hosting.md."
        );
        return;
    }
    let config = match Config::from_env() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("configuration error: {e}");
            std::process::exit(2);
        }
    };
    if let Err(e) = serve(config).await {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
