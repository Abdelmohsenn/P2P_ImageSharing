use tokio::net::UdpSocket;
use std::fs::File;
use std::io::Write;
use tokio::io::{self};

#[tokio::main]
async fn main() -> io::Result<()> {
    let address = "127.0.0.1";
    let port = "8080";
    let together = format!("{}:{}", address, port);
    let socket = UdpSocket::bind(&together).await?;
    println!("Server listening on {}", together);

    let mut buffer = [0u8; 2048];
    let mut image_data = Vec::new(); 
    let mut chunk_count = 0;

    loop {
        let (size, addr) = socket.recv_from(&mut buffer).await?;
        println!("Received chunk of size: {} from {}", size, addr);

        // Add data to current image
        image_data.extend_from_slice(&buffer[..size]);
        chunk_count += 1;

        // Acknowledge every 500 chunks received
        if chunk_count % 500 == 0 {
            socket.send_to(b"ACK", addr).await?;
            println!("Acknowledged 500 chunks.");
        }

        // If the end of the image is detected, save the file
        if &buffer[..size] == b"END" {
            let mut file = File::create("received_image.png")?;
            file.write_all(&image_data)?;
            println!("Image saved successfully!");


            image_data.clear();
            chunk_count = 0;
            socket.send_to(b"ACK", addr).await?; // Acknowledge final chunk and END signal
        }
    }
}
