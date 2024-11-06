use image::imageops::FilterType;
use std::env;
use std::io;
use std::io::Cursor;
use std::io::Write;
use std::net::Ipv4Addr;
use std::path::Path;
use std::time::Instant;
use steganography::encoder::*;
use steganography::util::file_as_dynamic_image;
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use std::sync::{Arc,Mutex};
use tokio::sync::Mutex as AsyncMutex;
use tokio::time::{sleep, timeout, Duration};
use std::thread;

mod bully_election;
mod encryption;
use bully_election::server_election;

#[tokio::main]
async fn main() -> io::Result<()> {
    let my_address = "127.0.0.1:2012";

    let peers = vec![
        "127.0.0.1:8083", // Address of Peer 1
        "127.0.0.1:8084", // Address of Peer 2
    ];
    let client_address = "127.0.0.1:8080";
    let socket_client = Arc::new(tokio::sync::Mutex::new(UdpSocket::bind(my_address).await?));
    let socket_election = Arc::new(tokio::sync::Mutex::new(
        UdpSocket::bind("127.0.0.1:2010").await?,
    )); // server-server socket
    let socketsendipback = Arc::new(tokio::sync::Mutex::new(
        UdpSocket::bind("127.0.0.1:2014").await?,
    ));
    let socket6 = Arc::new(tokio::sync::Mutex::new(
        UdpSocket::bind("127.0.0.1:2004").await?,
    ));

    let failure_socket = Arc::new(tokio::sync::Mutex::new(
        UdpSocket::bind("127.0.0.1:9002").await?,
    ));

    let mut leader = false;

    let fail_flag = Arc::new(Mutex::new(false)); // Shared fail flag
    let fail_flag_clone = Arc::clone(&fail_flag); // Clone the flag for second task

    let fail_flag_clone_for_failure = Arc::clone(&fail_flag);
    // Set up a task to listen for incoming messages (including "ELECT")
    let socket_clone = Arc::clone(&socket_client);
    tokio::spawn(async move {
        loop {
            let mut buffer = [0u8; 2048];
            let (size, addr) = socket_election
                .lock()
                .await
                .recv_from(&mut buffer)
                .await
                .unwrap();
            let message = String::from_utf8_lossy(&buffer[..size]);

            let fail_flag_value = *fail_flag_clone.lock().unwrap();
            if message == "ELECT" && !fail_flag_value {
                println!(
                    "Election request received from {}. Initiating election...",
                    addr
                );

                match server_election(&socket_election, peers.clone()).await {
                    Ok(is_leader) => {
                        if is_leader {
                            println!("This server is the leader.");
                            leader = true;

                            // Send "Leader_Ack" with the server address
                            let message_to_client = format!("LEADER_ACK:{}", my_address);
                            socketsendipback
                                .lock()
                                .await
                                .send_to(message_to_client.as_bytes(), client_address)
                                .await
                                .unwrap();
                        } else {
                            println!("This server is not the leader.");
                            leader = false;
                        }
                    }
                    Err(e) => eprintln!("Election failed: {:?}", e),
                }
            }
        }
    });

    let failure_socket_clone = Arc::clone(&failure_socket);
    tokio::spawn(async move {
        let mut buffer = [0u8; 2048];
        loop {
            let (size, addr) = failure_socket_clone
                .lock()
                .await
                .recv_from(&mut buffer)
                .await
                .expect("Failed to receive message");
    
            let message = String::from_utf8_lossy(&buffer[..size]);
            println!("Message received from {}: {}", addr, message);
    
            if message.trim() == "FAIL" {
                println!("This server is down!");
                *fail_flag_clone_for_failure.lock().unwrap() = true;
                thread::sleep(Duration::from_secs(30));
                *fail_flag_clone_for_failure.lock().unwrap() = false;
                println!("Server restored!");
                break;
            }
        }
    });

    let (tx, mut rx): (
        mpsc::Sender<(Vec<u8>, std::net::SocketAddr)>,
        mpsc::Receiver<(Vec<u8>, std::net::SocketAddr)>,
    ) = mpsc::channel(32); // sender/receiving vars
    let socket_clone_client = Arc::clone(&socket_client);

    tokio::spawn(async move {
        let mut buffer = [0u8; 2048];
        let mut image_data = Vec::new();
        let mut received_chunks = 0;
        let mut expected_sequence_num = 0;
        println!("Server listening on {}", my_address);

        loop {
            let (size, addr) = socket_clone_client
                .lock()
                .await
                .recv_from(&mut buffer)
                .await
                .unwrap();
            if size < 4 {
                continue;
            }

            if &buffer[4..size] == b"END" {
                let sequence_num = u32::from_be_bytes(buffer[0..4].try_into().unwrap());

                if sequence_num == expected_sequence_num {
                    let mut file = match std::fs::File::create("received_image.jpg") {
                        Ok(f) => f,
                        Err(e) => {
                            eprintln!("Failed to create file: {:?}", e);
                            continue;
                        }
                    };
                    if let Err(e) = file.write_all(&image_data) {
                        eprintln!("Failed to write to file: {:?}", e);
                        continue;
                    }
                    println!("Image saved successfully!");

                    tx.send((image_data.clone(), addr)).await.unwrap(); // Send to encryption thread
                    image_data.clear();
                    received_chunks = 0;
                    expected_sequence_num = 0;
                    socket_clone_client
                        .lock()
                        .await
                        .send_to(b"END", addr)
                        .await
                        .unwrap();
                    println!("Final ACK sent.");
                } else {
                    socket_clone_client
                        .lock()
                        .await
                        .send_to(format!("NACK {}", expected_sequence_num).as_bytes(), addr)
                        .await
                        .unwrap();
                    // println!("NACK sent for sequence number {}", expected_sequence_num);
                }
                continue;
            }
            let sequence_num = u32::from_be_bytes(buffer[0..4].try_into().unwrap());

            if sequence_num == expected_sequence_num {
                image_data.extend_from_slice(&buffer[4..size]);
                received_chunks += 1;
                expected_sequence_num += 1;
            } else {
                socket_clone_client
                    .lock()
                    .await
                    .send_to(format!("NACK {}", expected_sequence_num).as_bytes(), addr)
                    .await
                    .unwrap();
                println!("NACK sent for sequence number {}", expected_sequence_num);
                continue;
            }

            if received_chunks % 10 == 0 {
                socket_clone_client
                    .lock()
                    .await
                    .send_to(
                        format!("ACK {}", expected_sequence_num - 1).as_bytes(),
                        addr,
                    )
                    .await
                    .unwrap();
                println!("ACK {} sent.", expected_sequence_num - 1);
            }
        }
    });

    tokio::spawn(async move {
        while let Some((image_data, client_addr)) = rx.recv().await {
            // Guess the format and load the image for encryption
            let format = image::guess_format(&image_data).expect("Failed to guess image format");
            let original_img =
                image::load(Cursor::new(image_data.clone()), format).expect("Failed to load image");

            let default_img_path = Path::new("images/mask.jpg");
            let default_img = image::open(default_img_path).expect("Failed to open default image");

            // Encrypt the image using the steganography crate
            let start = Instant::now();
            let mask = file_as_dynamic_image(default_img_path.to_str().unwrap().to_string());
            let encoder = Encoder::new(&image_data, mask);
            let encrypted_img = encoder.encode_alpha();
            println!("Encryption Time: {:?}", start.elapsed());

            // Save the encrypted image as PNG to avoid lossy compression
            let encrypted_image_path = "encrypted-image.png";
            encrypted_img
                .save(encrypted_image_path)
                .expect("Failed to save encoded image as PNG");
            println!("Server: Encrypted image saved as {}", encrypted_image_path);

            // Read the PNG image bytes
            let encrypted_data =
                std::fs::read(encrypted_image_path).expect("Failed to read encrypted PNG image");

            // Send encrypted data in chunks
            let chunk_size = 2044;
            let total_chunks = (encrypted_data.len() as f64 / chunk_size as f64).ceil() as usize;
            let mut sequence_num: u32 = 0;
            let mut ack_buffer = [0u8; 1024];
            let max_retries = 5;

            for i in 0..total_chunks {
                let start = i * chunk_size;
                let end = std::cmp::min(start + chunk_size, encrypted_data.len());
                let chunk_data = &encrypted_data[start..end];

                let mut chunk = Vec::with_capacity(4 + chunk_data.len());
                chunk.extend_from_slice(&sequence_num.to_be_bytes());
                chunk.extend_from_slice(chunk_data);

                socket6
                    .lock()
                    .await
                    .send_to(&chunk, "127.0.0.1:2005")
                    .await
                    .expect("Failed to send encrypted image chunk");
                println!("Sent encrypted chunk {} of {}", i + 1, total_chunks);

                let mut retries = 0;
                loop {
                    match timeout(
                        Duration::from_secs(5),
                        socket6.lock().await.recv_from(&mut ack_buffer),
                    )
                    .await
                    {
                        Ok(Ok((ack_size, _))) => {
                            let ack_message = String::from_utf8_lossy(&ack_buffer[..ack_size]);
                            if ack_message.starts_with("ACK") {
                                let ack_num: u32 = ack_message[4..].parse().unwrap();
                                if ack_num == sequence_num {
                                    println!("ACK received for sequence number {}", ack_num);
                                    sequence_num += 1;
                                    break;
                                }
                            } else if ack_message.starts_with("NACK") {
                                println!("NACK received for sequence number {}", sequence_num);
                            }
                        }
                        Ok(Err(e)) => {
                            eprintln!("Failed to receive ACK/NACK: {:?}", e);
                            retries += 1;
                            if retries >= max_retries {
                                eprintln!(
                                    "Max retries reached for chunk {}. Aborting.",
                                    sequence_num
                                );
                                break;
                            }
                            println!("Retrying chunk {} after recv_from error", sequence_num);
                            socket6
                                .lock()
                                .await
                                .send_to(&chunk, "127.0.0.1:2005")
                                .await
                                .expect("Failed to resend chunk");
                        }
                        Err(_) => {
                            retries += 1;
                            if retries >= max_retries {
                                eprintln!(
                                    "Max retries reached for chunk {}. Aborting.",
                                    sequence_num
                                );
                                break;
                            }
                            println!("Timeout waiting for ACK, resending chunk {}", sequence_num);
                            socket6
                                .lock()
                                .await
                                .send_to(&chunk, "127.0.0.1:2005")
                                .await
                                .expect("Failed to resend chunk");
                        }
                    }
                }
            }

            // Send "END" message after the last chunk
            socket6
                .lock()
                .await
                .send_to(b"END", "127.0.0.1:2005")
                .await
                .expect("Failed to send END message");
            println!("Encrypted image transmission completed.");
        }
    })
    .await
    .expect("Task failed");

    Ok(())
}
