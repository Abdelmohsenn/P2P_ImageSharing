use std::env;
use tokio::net::UdpSocket;
use image::{ImageFormat};
use std::io::{self, Cursor};

// tokio usage line for async tasks
#[tokio::main]
async fn main() -> io::Result<()> {
    // Extract the command-line arguments
    let args: Vec<String> = env::args().collect();

    // Check if the IP addresses and port numbers of the three servers are provided
    if args.len() != 3 {
        eprintln!("Usage: {} <ClientIP:ClientPORT> <IP1:PORT1>", args[0]);
        return Ok(());
    }

    let addresses_ports = [
        args[args.len() - 2].clone(),
        args[args.len() - 1].clone(),
    ];

    let mut num = 0;
    let socket = UdpSocket::bind(addresses_ports[0].clone()).await?;
    socket.connect(addresses_ports[1].clone()).await?;

    while num < 3 {
        let mut input = String::new(); 

        println!("Enter your Image Path to send to the server: ");
        io::stdin().read_line(&mut input).expect("Failed to read line");
        let input = input.trim();

        match image::open(input) {
            Ok(img) => {
                
                let mut buf = Cursor::new(Vec::new());
                
                // handling all img formats
                let format = image::guess_format(&std::fs::read(input).expect("Failed to read the image file")).unwrap_or(ImageFormat::Png);

                // Converting the Image to Bytes
                img.write_to(&mut buf, format).expect("Failed to convert image to bytes");
                let image_bytes = buf.into_inner();

                // smaller chunks division
                let chunk_size = 2048;
                let total_chunks = (image_bytes.len() as f64 / chunk_size as f64).ceil() as usize;
                let batch_size = 500;
                let mut ack_buffer = [0u8; 8192];

                for i in 0..total_chunks {

                    // Calculate start and end index of chunk
                    let start = i * chunk_size;
                    let end = std::cmp::min(start + chunk_size, image_bytes.len());
                    let chunk = &image_bytes[start..end];

                    socket.send(chunk).await?;
                    println!("Sent chunk {}/{}", i + 1, total_chunks);

                    // Wait for server ACK. 
                    if (i + 1) % batch_size == 0 {
                        let ack_size = socket.recv(&mut ack_buffer).await?;
                        let ack_message = String::from_utf8_lossy(&ack_buffer[..ack_size]);

                        if ack_message != "ACK" {
                            println!("Error: Did not receive ACK from the server. Aborting.");
                            break;
                        }
                        println!("ACK received from server, continuing...");
                    }
                }

                // Send END to indicate the end of the image transmission
                socket.send(b"END").await?;
                println!("Image sent successfully.");

                // Wait for the final ACK after sending the "END" signal
                let ack_size = socket.recv(&mut ack_buffer).await?;
                let ack_message = String::from_utf8_lossy(&ack_buffer[..ack_size]);

                if ack_message == "ACK" {
                    println!("Final ACK received from server. Image sent");
                }

            },
            Err(e) => {
                println!("Failed to open image: {}", e);
                continue;
            }
        }
        num += 1;
    }

    Ok(())
}
