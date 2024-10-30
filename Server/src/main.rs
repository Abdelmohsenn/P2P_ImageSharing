use std::env;
use std::io;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use std::path::Path;
use image::{DynamicImage, RgbaImage};
use std::time::Instant;
use std::io::Cursor;
use sysinfo::{ProcessExt, System, SystemExt};
use std::{thread, time::Duration};
mod steganography;
mod bully_election;
use bully_election::server_election;

#[tokio::main] // using tokio udp socket for communication
async fn main() -> io::Result<()> {
    // Number of servers = 3
    const N: usize = 3;
    let peer_address = "10.40.44.175:8091";

    
    // Extract the command-line arguments
    let args: Vec<String> = env::args().collect();

    // Check if the IP addresses and port numbers of the three servers are provided
    // if args.len() != N+1 {
    //     eprintln!("Usage: {} <IP1:PORT1>, <IP2:PORT2>, <IP3:PORT3>", args[0]);
    //     return Ok(());
    // }

    // The IP address and port number of the same server is the second argument
    // let address = &args[1];
    let addresses_ports = [
        // args[args.len() - 3].clone(),
        // args[args.len() - 2].clone(),
        args[args.len() - 1].clone(),
    ];
    
    // Create a socket bound to the provided address
    let socket = Arc::new(tokio::sync::Mutex::new(UdpSocket::bind(addresses_ports[0].clone()).await?));
    println!("Server listening on {}", addresses_ports[0]);

    let (tx, mut rx) = mpsc::channel(32);
    let socket_clone = Arc::clone(&socket);
    
    // server_election(&socket_clone, peer_address).await?;
    tokio::spawn(async move {
    match server_election(&socket, peer_address).await {
        Ok(is_leader) => {
            if is_leader {
                println!("Main: This server is the leader.");
            } else {
                println!("Main: This server is not the leader.");
            }
        }
        Err(e) => eprintln!("Election failed: {:?}", e),
    }
    });

  
    let socket_client = Arc::new(tokio::sync::Mutex::new(UdpSocket::bind("10.40.34.255:2004").await?));
    let socket_clone_client = Arc::clone(&socket_client);



    // Spawn task for receiving packets
    tokio::spawn(async move {
        let mut buffer = [0u8; 2048];
        let mut chunk_count = 0;
        let mut img_data = Vec::new();

        loop {
            let (size, addr) = socket_clone_client.lock().await.recv_from(&mut buffer).await.unwrap();
            println!("Received chunk of size: {} from {}", size, addr);

            // Add data to current image
            img_data.extend_from_slice(&buffer[..size]);
            chunk_count += 1;

            // Acknowledge every 500 chunks received
            if chunk_count % 500 == 0 {
                socket_clone_client.lock().await.send_to(b"ACK", addr).await.unwrap();
                println!("Acknowledged 500 chunks.");
            }

            // If the end of the image is detected, send data for processing
            if &buffer[..size] == b"END" {
                tx.send((img_data.clone(), addr)).await.unwrap();
                img_data.clear(); // Reset for the next batch
                chunk_count = 0;
            }
        }
    });

    // Spawn task for processing encryption/decryption
    tokio::spawn(async move {
        while let Some((img_data, addr)) = rx.recv().await {
            let original_img = image::load(Cursor::new(img_data.clone()), image::ImageFormat::Jpeg).unwrap();
            let default_img_path = Path::new("images/sunflower-0quality.jpg");
            let default_img = image::open(default_img_path).unwrap();

            // Encryption
            let start = Instant::now();
            let encrypted_img: RgbaImage = steganography::encrypt(default_img, original_img.clone());
            println!("Encryption Time: {:?}", start.elapsed());

            let _ = encrypted_img.save("images/encrypted-image.jpg");

            // Decryption
            let start = Instant::now();
            let decrypted_img: DynamicImage = steganography::decrypt(DynamicImage::from(encrypted_img));
            println!("Decryption Time: {:?}", start.elapsed());

            let _ = decrypted_img.save("images/decrypted-image.jpg");
            println!("Processed image from {}", addr);
        }
    }).await.unwrap(); // Ensure we wait for the task

   
    Ok(())
}
