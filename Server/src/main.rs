use std::io;
mod middleware;
mod bully_election;

#[tokio::main]
async fn main() -> io::Result<()> {
    middleware::middleware().await?;
    Ok(())
}
