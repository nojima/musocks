use slog::Drain;

mod socks;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let logger = setup_logger();
    let server = socks::Server {
        logger: logger.clone(),
    };
    server.serve().await
}

fn setup_logger() -> slog::Logger {
    let decorator = slog_term::PlainSyncDecorator::new(std::io::stderr());
    let drain = slog_term::FullFormat::new(decorator).build().fuse();
    slog::Logger::root(drain, slog::o!())
}
