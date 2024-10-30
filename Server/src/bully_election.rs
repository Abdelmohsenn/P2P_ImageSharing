use tokio::net::{UdpSocket};
use std::io::{self};
use std::process;
use std::time::Duration;
use tokio::time::sleep;
use tokio::time::timeout;
use sysinfo::{System, SystemExt, ProcessExt};
use std::sync::Arc;
use tokio::sync::Mutex;


// this function is used to detect who is a leader from two servers. To use it, just duplicate 
// a new server with the same code and change ports(look at teh main.rs port part)
pub async fn server_election(socket: &Arc<Mutex<UdpSocket>>, peer_address: &str) -> io::Result<bool> {

    let server_id = process::id();
    println!("Server Process ID: {}", server_id);

    let mut initial_response_received = false;
    let mut buffer = [0u8; 32];


    while !initial_response_received {
        let result = timeout(Duration::from_secs(5), socket.lock().await.recv_from(&mut buffer)).await;
        socket.lock().await.send_to(&server_id.to_be_bytes(), peer_address).await?;

        match result {
            Ok(Ok((size, _))) => {
                println!("Initial response received from peer.");
                initial_response_received = true;
                let message = &buffer[..size];
                let confirmation = String::from_utf8_lossy(message);
                println!("Received initial message from peer: {}", confirmation);
            }
            Ok(Err(e)) => {
                eprintln!("Error receiving initial message: {:?}", e);
                return Err(e);
            }
            Err(_) => {
                println!("Waiting for initial response from peer...");
                sleep(Duration::from_secs(2)).await;
            }
        }
    }
    let mut leader = false;

    socket.lock().await.send_to(&server_id.to_be_bytes(), peer_address).await?;
    println!("Sent Process ID to peer at {}", peer_address);

    sleep(Duration::from_millis(500)).await;// delay

    let mut buffer = [0u8; 32];
    println!("Waiting for peer's Process ID or election result...");

    // let (size, _) = socket.recv_from(&mut buffer).await?;
    // let (size, _) = socket.lock().await.recv_from(&mut buffer).await?;
    let result = timeout(Duration::from_secs(60), socket.lock().await.recv_from(&mut buffer)).await;
    match result {
        Ok(Ok((size, _))) => {
            let message = &buffer[..size];
            // let message = &buffer[..size];
             // 4 bytes  checking
            if message.len() == 4 {
                let peer_id = u32::from_be_bytes(message.try_into().unwrap());
                println!("Received peer's Process ID: {}", peer_id);

                if server_id > peer_id {
                    // there is something wrong here, there is no printing regardless of the case

                    println!("This server (Process ID: {}) is the leader.", server_id);
                    leader = true;

                } else if peer_id > server_id {
                // checking who is the highest
                    println!("Peer server (Process ID: {}) is the leader.", peer_id);
                    // socket.send_to(b"You are the Leader", peer_address).await?;
                    socket.lock().await.send_to(b"You are the Leader", peer_address).await?;
                    leader = false;

                } 
            } else {

                let confirmation = String::from_utf8_lossy(message);
                println!("Received election confirmation from peer: {}", confirmation);
                leader = true;
            }
        }
        Ok(Err(e)) => {
            eprintln!("Error receiving message: {:?}", e);
            return Err(e); // Return the error here
        }
        Err(_) => {
            eprintln!("Timeout reached, no message received.");
            return Err(io::Error::new(io::ErrorKind::TimedOut, "No message received before timeout.")); // Return timeout error
        }
    }
            // debugging for loop
            if leader == true {
                println!("This server is the leader.");
               
            } else {

                println!("I am not the leader.");
                
            }

        
        match get_cpu_usage() {
            Some(cpu_usage) => println!("CPU Usage: {:.2}%", cpu_usage),
            None => println!("Failed to retrieve CPU usage."),
        }

        // println!("{}",leader);


        Ok(leader)
    
}
fn get_cpu_usage() -> Option<f32> {
    let mut sys = System::new_all();
    sys.refresh_all();

    // Get the current process PID
    let pid = sysinfo::get_current_pid().ok()?;

    // Fetch the process and retrieve CPU usage
    sys.process(pid).map(|process| process.cpu_usage())
}
