use image::{DynamicImage, ImageFormat, RgbaImage};
use serde::{Serialize, Deserialize};
use std::fs::File;
use std::io::{self, Cursor, Write};
use std::net::SocketAddr;
use std::time::Instant;
use steganography::decoder::Decoder;
use steganography::util::file_as_dynamic_image;
use tokio::net::UdpSocket;
use tokio::time::{sleep, timeout, Duration};


// The middleware function containibg the logic for the client such as communicating with the servers and sending messages to the servers   
async fn middleware(socket: &UdpSocket, socket6: &UdpSocket) -> io::Result<()> {
    let mut buffer = [0u8; 2048];
    let mut leader_address = String::new();

    loop {
        
     // Set a timeout for receiving the leader address and acknowledgment

        let receive_result = timeout(Duration::from_secs(5), socket.recv_from(&mut buffer)).await;

        match receive_result {
            Ok(Ok((size, _))) => {
                let message = String::from_utf8_lossy(&buffer[..size]);

                // Leader acknowledgment received with the address

                if message.starts_with("LEADER_ACK") {
                    leader_address = message.replace("LEADER_ACK:", "").trim().to_string();
                    println!(
                        "Leader identified at {}. Proceeding to connect...",
                        leader_address
                    );
                    socket.connect(&leader_address).await?; // make this the leader address after the ack is sent back from the server
                    break;
                } else {
                    println!("Received message without Leader_Ack, retrying...");
                }
            }
            Ok(Err(e)) => {
                eprintln!("Error receiving data: {:?}", e);
            }
            Err(_) => {
                // Timeout occurred
                println!("Timeout waiting for Leader_Ack. Retrying...");
            }
        }
    }

    let mut input = String::new();
    println!("Enter your Image Path to send to the server: ");
    io::stdin()
        .read_line(&mut input)
        .expect("Failed to read line");
    let input = input.trim();
    let format = image::guess_format(&std::fs::read(input).expect("Failed to read the image file"))
        .unwrap_or(ImageFormat::Jpeg);
    let img = image::open(input).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    let mut buf = Cursor::new(Vec::new());
    img.write_to(&mut buf, format)
        .expect("Failed to convert image to bytes");
    let image_bytes = buf.into_inner();

    let chunk_size = 2044;
    let total_chunks = (image_bytes.len() as f64 / chunk_size as f64).ceil() as usize;

    let mut cursed = false;
    if total_chunks % 10 == 2 {
        cursed = true;
    }

    let mut ack_buffer = [0u8; 1024];
    let max_retries = 5;
    let mut sequence_num: u32 = 0;

    for i in 0..total_chunks {
        let start = i * chunk_size;
        let end = std::cmp::min(start + chunk_size, image_bytes.len());
        let chunk_data = &image_bytes[start..end];

        // Prepare chunk with sequence number
        let mut chunk = Vec::with_capacity(4 + chunk_data.len());
        chunk.extend_from_slice(&sequence_num.to_be_bytes());
        chunk.extend_from_slice(chunk_data);

        socket.send(&chunk).await?;
        println!(
            "Sent chunk {} of {} with sequence number {}",
            i + 1,
            total_chunks,
            sequence_num
        );

        if i == total_chunks - 1 {
            let mut end_message = Vec::with_capacity(4 + 3);
            println!(
                "Sending END message... with sequence number {}",
                sequence_num
            );
            end_message.extend_from_slice(&(sequence_num + 1).to_be_bytes());
            end_message.extend_from_slice(b"END");
            socket.send(&end_message).await?;

             // Wait for ACK END
            match timeout(Duration::from_secs(5), socket.recv(&mut ack_buffer)).await {
                Ok(Ok(ack_size)) => {
                    let ack_message = String::from_utf8_lossy(&ack_buffer[..ack_size]);
                    if ack_message == "END" {
                        println!("Final ACK received. Image transmission completed.");
                        break;
                    }
                }
                Ok(Err(e)) => println!("Failed to receive final ACK: {}", e),
                Err(_) => println!("Timeout waiting for final ACK."),
            }
            break;
        }

        sequence_num += 1;
        // Check for ACK or NACK every 10 chunks or at the end of transmission
        if (i + 1) % 10 == 0 || i == total_chunks - 1 {
            let mut retries = 0;
            loop {
                match timeout(Duration::from_secs(5), socket.recv(&mut ack_buffer)).await {
                    Ok(Ok(ack_size)) => {
                        let ack_message = String::from_utf8_lossy(&ack_buffer[..ack_size]);
                        if ack_message.starts_with("ACK") {
                            let ack_num: u32 = ack_message[4..].parse().unwrap();
                            println!("ACK received for sequence number {}.", ack_num);
                            if ack_num == sequence_num - 1 {
                                break; // Successfully received ACK for this chunk
                            }
                        } else if ack_message.starts_with("NACK") {
                            println!("NACK received. Resending last batch...");
                            retries += 1;
                            if retries >= max_retries {
                                println!("Max retries reached for last batch. Aborting.");
                                break;
                            }
                    // Resend the last 10 chunks or all chunks if fewer than 10
                            let start_chunk = (i + 1 - 10).max(0);
                            for j in start_chunk..=i {
                                let start = j * chunk_size;
                                let end = std::cmp::min(start + chunk_size, image_bytes.len());
                                let chunk_data = &image_bytes[start..end];

                                let mut chunk = Vec::with_capacity(4 + chunk_data.len());
                                chunk.extend_from_slice(&(j as u32).to_be_bytes());
                                chunk.extend_from_slice(chunk_data);

                                socket.send(&chunk).await?;
                                println!(
                                    "Resent chunk {} of {} with sequence number {}",
                                    j + 1,
                                    total_chunks,
                                    j
                                );
                                sleep(Duration::from_millis(5)).await;
                            }
                        }
                    }
                    Ok(Err(e)) => {
                        println!("Failed to receive ACK: {}", e);
                        break;
                    }
                    Err(_) => {
                        println!("Timeout waiting for ACK. Retrying last batch.");
                        retries += 1;
                        if retries >= max_retries {
                            println!("Max retries reached for timeout on batch. Aborting.");
                            break;
                        }
                    // Resend the last 10 chunks or all chunks if fewer than 10
                        let start_chunk = (i + 1 - 10).max(0);
                        for j in start_chunk..=i {
                            let start = j * chunk_size;
                            let end = std::cmp::min(start + chunk_size, image_bytes.len());
                            let chunk_data = &image_bytes[start..end];

                            let mut chunk = Vec::with_capacity(4 + chunk_data.len());
                            chunk.extend_from_slice(&(j as u32).to_be_bytes());
                            chunk.extend_from_slice(chunk_data);

                            socket.send(&chunk).await?;
                            println!(
                                "Resent chunk {} of {} with sequence number {}",
                                j + 1,
                                total_chunks,
                                j
                            );
                            sleep(Duration::from_millis(5)).await;
                        }
                    }
                }
            }
        }
    }
    println!("Waiting for encrypted image from server...");
    let mut encrypted_image_data = Vec::new();
    let mut buffer = [0u8; 2048];
    let mut expected_sequence_num: u32 = 0;

    loop {
        let (len, addr) = socket6.recv_from(&mut buffer).await?;

        if &buffer[..len] == b"END" {
            println!("Encrypted image received completely from server.");
            break;
        }
   // Extract the sequence number and data
        let sequence_num = u32::from_be_bytes(buffer[0..4].try_into().unwrap());
        let chunk_data = &buffer[4..len];

        if sequence_num == expected_sequence_num {
            encrypted_image_data.extend_from_slice(chunk_data);
            println!(
                "Received encrypted chunk with sequence number {}",
                sequence_num
            );
                // Send ACK for the received chunk

            let ack_message = format!("ACK {}", sequence_num);
            socket6.send_to(ack_message.as_bytes(), addr).await?;
            expected_sequence_num += 1;
        } else {
                // Send NACK if the sequence number is not as expected

            let nack_message = format!("NACK {}", expected_sequence_num);
            socket6.send_to(nack_message.as_bytes(), addr).await?;
            println!("NACK sent for sequence number {}", expected_sequence_num);
        }
    }
        // Save the received encrypted image as a PNG file
    let encrypted_image_path = "encrypted_image_from_server.png";
    let mut encrypted_image_file = File::create(encrypted_image_path)?;
    encrypted_image_file.write_all(&encrypted_image_data)?;
    println!("Encrypted image saved as PNG at {}", encrypted_image_path);
        // decrypt the image
    let encrypted_image = file_as_dynamic_image(encrypted_image_path.to_string()).to_rgba();
    let decoder = Decoder::new(encrypted_image);
    let decrypted_data = decoder.decode_alpha();
    let output_path = "decrypted_image.png";
    std::fs::write(output_path, &decrypted_data)?;

    println!("Decrypted image saved successfully as PNG!");
    Ok(())
}

#[tokio::main]
async fn main() -> io::Result<()> {
loop{ // loop to keep the client running

    // Check if the IP addresses and port numbers of the three servers are provided

    // if args.len() != N+1 {
    // eprintln!("Usage: {} <ClientIP:ClientPORT> <Server1_IP:Server1_PORT> <Server2_IP:Server2_PORT> <Server3_IP:Server3_PORT>", args[0]);
    // return Ok(());
    // }

    // let addresses_ports = [
    // // args[args.len() - 4].clone(),
    // // args[args.len() - 3].clone(),
    // args[args.len() - 2].clone(),
    // args[args.len() - 1].clone(),
    // ];

        // multicast to all servers
        let servers: Vec<SocketAddr> = vec![

        "127.0.0.1:8083".parse().unwrap(),
        "127.0.0.1:8084".parse().unwrap(),
        "127.0.0.1:2010".parse().unwrap(),
    ];

    let clientaddress = "127.0.0.1:8080"; // my client server address
    let socket = UdpSocket::bind(clientaddress).await?;
    let socket6 = UdpSocket::bind("127.0.0.1:2005").await?; // socket for encrypted image recieving

    let mut input = String::new();
    println!("Do you want to start Sending Message? (y/n): ");
    io::stdin()
        .read_line(&mut input)
        .expect("Failed to read input");

     // if yes, send ELECT message to all servers
    if input.trim().eq_ignore_ascii_case("y") {
        for addr in &servers {
            socket.send_to(b"ELECT", addr).await?;
            println!("message sent to {}", addr);
        }
    }

    // Call the middleware function that handles everything
    middleware(&socket, &socket6).await?;
}
    Ok(())
}
