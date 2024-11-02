use tokio::net::UdpSocket;
use std::io::{self};
use std::process;
use std::time::Duration;
use tokio::time::{sleep, timeout};
use sysinfo::{System, SystemExt, ProcessExt};
use std::sync::Arc;
use tokio::sync::Mutex;

pub async fn server_election(socket: &Arc<Mutex<UdpSocket>>, peer_address: &str) -> io::Result<bool> {
    let cpu_usage = match get_cpu_usage() {
        Some(usage) => usage,
        None => {
            eprintln!("Failed to retrieve CPU usage.");
            return Err(io::Error::new(io::ErrorKind::Other, "Failed to retrieve CPU usage."));
        }
    };

    let server_id = process::id();
    println!("CPU Usage: {:.2}%", cpu_usage);
    println!("Server Process ID: {}", server_id);

    let mut buffer = [0u8; 32];

    // Send both CPU usage and server ID to the peer
    let cpu_bytes = cpu_usage.to_be_bytes();
    let id_bytes = server_id.to_be_bytes();
    socket.lock().await.send_to(&[&cpu_bytes[..], &id_bytes[..]].concat(), peer_address).await?;

    // Receive peer's CPU usage and server ID
    let result = timeout(Duration::from_secs(15), socket.lock().await.recv_from(&mut buffer)).await;
    let mut leader = false;

    match result {
        Ok(Ok((size, _))) => {
            let message = &buffer[..size];

            if message.len() >= 8 {
                // Extract peer's CPU usage and server ID from the message
                let peer_cpu_usage = f32::from_be_bytes(message[0..4].try_into().unwrap());
                let peer_id = u32::from_be_bytes(message[4..8].try_into().unwrap());

                println!("Received peer's CPU Usage: {:.2}%", peer_cpu_usage);
                println!("Received peer's Process ID: {}", peer_id);

                // Compare CPU usage first, then use server_id as a tiebreaker
                if cpu_usage < peer_cpu_usage {
                    println!("This server (Process ID: {}) is the leader based on lower CPU usage.", server_id);
                    leader = true;
                } else if cpu_usage > peer_cpu_usage {
                    println!("Peer server (Process ID: {}) is the leader based on lower CPU usage.", peer_id);
                    socket.lock().await.send_to(b"You are the Leader", peer_address).await?;
                    leader = false;
                } else {
                    // If CPU usage is equal, compare by server_id
                    if server_id < peer_id {
                        println!("This server (Process ID: {}) is the leader based on lower Process ID.", server_id);
                        leader = true;
                    } else {
                        println!("Peer server (Process ID: {}) is the leader based on lower Process ID.", peer_id);
                        socket.lock().await.send_to(b"You are the Leader", peer_address).await?;
                        leader = false;
                    }
                }
            } else {
                let confirmation = String::from_utf8_lossy(message);
                println!("Received election confirmation from peer: {}", confirmation);
                leader = true;
            }
        }
        Ok(Err(e)) => {
            eprintln!("Error receiving message: {:?}", e);
            return Err(e);
        }
        Err(_) => {
            eprintln!("Timeout reached, no message received.");
            return Err(io::Error::new(io::ErrorKind::TimedOut, "No message received before timeout."));
        }
    }

    Ok(leader)
}

fn get_cpu_usage() -> Option<f32> {
    let mut sys = System::new_all();
    sys.refresh_all();

    let pid = sysinfo::get_current_pid().ok()?;
    sys.process(pid).map(|process| process.cpu_usage())
}
