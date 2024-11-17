use std::io;
mod bully_election;
mod middleware;

#[tokio::main]
async fn main() -> io::Result<()> {
    middleware::middleware().await?;
    Ok(())
}
