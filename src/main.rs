mod socks;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    socks::serve().await
}
