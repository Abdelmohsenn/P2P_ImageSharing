use std::io::{self};
use std::process;
use std::sync::Arc;
use std::time::Duration;
use sysinfo::{ProcessExt, System, SystemExt};
use tokio::net::UdpSocket;
use tokio::sync::Mutex;
use tokio::time::{timeout,Instant};

pub async fn server_election(socket: &Arc<Mutex<UdpSocket>>, peers: Vec<&str>) -> io::Result<bool> {
    let cpu_usage = match get_cpu_usage() {
        Some(usage) => usage,
        None => {
            eprintln!("Failed to retrieve CPU usage.");
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "Failed to retrieve CPU usage.",
            ));
        }
    };

    let server_id = process::id();
    println!("CPU Usage: {:.2}%", cpu_usage);
    println!("Server Process ID: {}", server_id);

    // Convert CPU usage and server ID to bytes
    let cpu_bytes = cpu_usage.to_be_bytes();
    let id_bytes = server_id.to_be_bytes();
    // Concatenate CPU usage and server ID into a single message
    let message = [&cpu_bytes[..], &id_bytes[..]].concat();

    // Broadcast CPU usage and server ID to all peers
    for peer_address in &peers {
        socket.lock().await.send_to(&message, peer_address).await?;
    }

    // Collect responses from peers
    let mut received_responses = Vec::new();
    let mut buffer = [0u8; 32];
    let expected_responses = peers.len();
    let timeout_duration = Duration::from_secs(5);
    let start_time = Instant::now();

    while received_responses.len() < expected_responses && start_time.elapsed() < timeout_duration {
        // Calculate the remaining time for the timeout
        let remaining_time = timeout_duration.checked_sub(start_time.elapsed()).unwrap_or_default();
        let result = timeout(remaining_time, socket.lock().await.recv_from(&mut buffer)).await;

        match result {
            Ok(Ok((size, _))) => {
                let message = &buffer[..size];
                if message.len() >= 8 {
                    let peer_cpu_usage = f32::from_be_bytes(message[0..4].try_into().unwrap());
                    let peer_id = u32::from_be_bytes(message[4..8].try_into().unwrap());
                    received_responses.push((peer_cpu_usage, peer_id));
                }
            }
            Ok(Err(e)) => {
                eprintln!("Error receiving message from peer: {:?}", e);
            }
            Err(_) => {
                // If the timeout is reached, we simply break out of the loop
                eprintln!("Timeout reached, no more messages received.");
                break;
            }
        }
    }
    // Check if any responses were received, if not, assume leadership
    if received_responses.is_empty() {
        println!(
            "No responses received. This server (Process ID: {}) assumes leadership.",
            server_id
        );
        return Ok(true);
    }

    // Add own server's data to the list
    received_responses.push((cpu_usage, server_id as u32));

    // Determine the leader by sorting
    received_responses.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap().then_with(|| b.1.cmp(&a.1)));

    let leader = received_responses[0].1 == server_id as u32;
    if leader {
        println!(
            "This server (Process ID: {}) is elected as the leader.",
            server_id
        );
    } else {
        println!("Server (Process ID: {}) is not the leader.", server_id);
    }

    // Confirm the leader to peers
    if leader {
        for peer_address in &peers {
            socket
                .lock()
                .await
                .send_to(b"Leader Confirmed", peer_address)
                .await?;
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
