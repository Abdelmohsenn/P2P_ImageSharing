use tokio::net::{UdpSocket};
use std::io::{self};
use std::process;
use std::time::Duration;
use tokio::time::sleep;

pub async fn server_election(socket: &UdpSocket, peer_address: &str) -> io::Result<()> {
    // this function is used to detect who is a leader from two servers. To use it, just duplicate 
    // a new server with the same code and change ports(look at teh main.rs port part)

    let server_id = process::id();
    let mut leader = false;
    println!("Server Process ID: {}", server_id);
    socket.send_to(&server_id.to_be_bytes(), peer_address).await?;
    println!("Sent Process ID to peer at {}", peer_address);

    sleep(Duration::from_millis(500)).await;// delay

    let mut buffer = [0u8; 32];
    println!("Waiting for peer's Process ID or election result...");

    let (size, _) = socket.recv_from(&mut buffer).await?;
    let message = &buffer[..size];
        // 4 bytes  checking
    if message.len() == 4 {
        let peer_id = u32::from_be_bytes(message.try_into().unwrap());
        println!("Received peer's Process ID: {}", peer_id);

        if server_id > peer_id {
            // there is something wrong here, there is no printing regardless of the case

            println!("This server (Process ID: {}) is the leader.", server_id);

        } else if peer_id > server_id {
        // checking who is the highest
            println!("Peer server (Process ID: {}) is the leader.", peer_id);
            socket.send_to(b"You are the Leader", peer_address).await?;

        } 
    } else {

        let confirmation = String::from_utf8_lossy(message);
        println!("Received election confirmation from peer: {}", confirmation);
        leader = true;
    }
    // debugging for loop
    if leader == true {
        println!("This server is the leader.");
        
    } else {

        println!("I am not the leader.");
    }

    Ok(())
}