//! ClosedRouter gateway binary.

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    closedrouter_gateway::run().await
}
