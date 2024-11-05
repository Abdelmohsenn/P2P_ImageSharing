use tokio::net::UdpSocket;
use rand::seq::SliceRandom;
use std::error::Error;
use std::net::SocketAddr;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // List of target addresses (IP:Port)
    let targets = vec![
        "127.0.0.1:9000".parse::<SocketAddr>()?,
        // "127.0.0.1:9001".parse::<SocketAddr>()?,
        // "127.0.0.1:9002".parse::<SocketAddr>()?,
    ];

    // Create a UDP socket bound to a random local port
    let socket = UdpSocket::bind("0.0.0.0:0").await?;
    
    // Set TTL for the message (optional)
    socket.set_ttl(5)?;

    println!("Starting UDP message sender...");

    // Message to send
    let message = "SLEEP";

    // Loop to send the message every 60 seconds
    loop {
        // Select one random target from the list
        let target = targets.choose(&mut rand::thread_rng())
            .expect("Failed to select a random target");

        // Send message to the randomly selected target
        match socket.send_to(message.as_bytes(), target).await {
            Ok(sent) => {
                println!("Sent {} bytes to {}", sent, target);
            },
            Err(e) => {
                eprintln!("Failed to send to {}: {}", target, e);
            }
        }

        // Wait for 60 seconds before sending the next message
        tokio::time::sleep(Duration::from_secs(120)).await;
    }
}
