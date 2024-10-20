use tokio::net::UdpSocket;
use std::io::{self};
use image::{ImageFormat, DynamicImage, RgbaImage}; // Ensure you import ImageFormat
use std::io::Cursor;
use std::path::Path;

mod encryption;

#[tokio::main]
async fn main() -> io::Result<()> {
    let address = "127.0.0.1";
    let port = "8080";
    let together = format!("{}:{}", address, port);
    let socket = UdpSocket::bind(&together).await?;
    println!("Server listening on {}", together);

    let mut buffer = [0u8; 2048];
    let mut img_data = Vec::new(); 
    let mut chunk_count = 0;

    loop {
        let (size, addr) = socket.recv_from(&mut buffer).await?;
        println!("Received chunk of size: {} from {}", size, addr);

        // Add data to current image
        img_data.extend_from_slice(&buffer[..size]);
        chunk_count += 1;

        // Acknowledge every 500 chunks received
        if chunk_count % 500 == 0 {
            socket.send_to(b"ACK", addr).await?;
            println!("Acknowledged 500 chunks.");
        }

        // If the end of the image is detected, process the images
        if &buffer[..size] == b"END" {
            let original_img = image::load(Cursor::new(img_data.clone()), ImageFormat::Jpeg).unwrap();
        
            let default_img_path = Path::new("images/pexels-sohi-807598.jpg");
            let default_img = image::open(default_img_path).unwrap();
        
            let encrypted_img: RgbaImage = encryption::encrypt(default_img, original_img);
            
            encrypted_img.save("images/encrypted-image.jpg");

            println!("Encrypted image saved successfully!");

            let (w, h) = encrypted_img.dimensions();
            let decrypted_img: DynamicImage = encryption::decrypt(DynamicImage::from(encrypted_img.clone()), w, h);

            decrypted_img.save("images/decrypted-image.jpg");

            println!("Decrypted image saved successfully!");

            img_data.clear(); // Clear for the next batch
            chunk_count = 0;
            socket.send_to(b"ACK", addr).await?; // Acknowledge final chunk and END signal
        }        
    }
}
