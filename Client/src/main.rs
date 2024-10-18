use tokio::net::UdpSocket;
use image::{GenericImageView};
use std::fs::File;
use std::io::{self, Read, Write};

#[tokio::main]
async fn main() -> io::Result<()> {
    let mut num = 0;
    let socket = UdpSocket::bind("127.0.0.1:8081").await?; // Bind to any available port
    socket.connect("127.0.0.1:8080").await?;

    while num < 3 {
        let mut input = String::new(); 

        println!("Enter your Image Path to send to the server: ");

        io::stdin()
            .read_line(&mut input)
            .expect("Failed to read line");

            let input = input.trim();

        match image::open(input) {
            Ok(img) => {
                println!("{:?}", img);
            },
            Err(e) => {
                println!("Failed to open image: {}", e);
                continue;
                
            }
        }

        num += 1;
        let message = input.as_bytes();
        socket.send(message).await?;

        let mut buffer = [0u8; 1024];
        let size = socket.recv(&mut buffer).await?;
        let echoed_message = String::from_utf8_lossy(&buffer[..size]);

        println!("Received echoed message: {}", echoed_message);
    }

    Ok(())
}
