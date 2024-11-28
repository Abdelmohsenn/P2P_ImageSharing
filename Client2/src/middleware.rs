use image::ImageFormat;
use std::fs;
use std::path::Path;
use tokio::net::UdpSocket;

use std::collections::HashMap;
use tokio::time::{sleep, timeout, Duration};

use steganography::decoder::Decoder;
use steganography::encoder::Encoder;
use steganography::util::bytes_to_str;
use steganography::util::file_as_dynamic_image;
use steganography::util::file_as_image_buffer;
use steganography::util::save_image_buffer;
use steganography::util::str_to_bytes;
use std::fs::File;
use std::fs::OpenOptions;
use std::io::{self, Cursor, Write};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio::signal;
use tokio::time;


struct ImageStats {
    client_id: String, // Unique identifier
    img_id: String,    // Image identifier
    num_of_views: u8,  // Number of views (using unsigned 8-bit integer)
}
pub async fn middleware(socket: &UdpSocket, socket6: &UdpSocket, image_id: &str) -> io::Result<String> {
    let mut buffer = [0u8; 2048];
    let mut leader_address = String::new();

    loop {
        // Set a timeout for receiving the leader address and acknowledgment

        let receive_result = timeout(Duration::from_secs(5), socket.recv_from(&mut buffer)).await;
        match receive_result {
            Ok(Ok((size, _))) => {
                let message = String::from_utf8_lossy(&buffer[..size]);
                println!("{}",message);

                if message.starts_with("LEADER_ACK:") {
                    leader_address = message.replace("LEADER_ACK:", "").trim().to_string();
                    println!(
                        "Leader identified at {}. Proceeding to connect...",
                        leader_address
                    );
                    socket.connect(&leader_address).await?; // make this the leader address after the ack is sent back from the server
                    break;
                } else {
                    println!("Received message without Leader_Ack, retrying...");
                    println!("Received: {}", message);
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

    send_image(&socket, image_id).await?;

    // Allows user to input image path (one-by-one)

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

    // // // // mn hena, example lel view encryption / / / / // / / /
    let stats = ImageStats {
        client_id: String::from("5"),
        img_id: String::from("596132"),
        num_of_views: 10,
    };

    // Define a secret message to hide in our picture
    let views = stats.num_of_views.to_string();
    println!("Encoding message: {}", views);
    // Convert our string to bytes
    let payload = str_to_bytes(&views);
    println!("Payload bytes: {:?}", payload);
    // Load the image where we want to embed our secret message
    let destination_image = file_as_dynamic_image("encrypted_image_from_server.png".to_string());
    // Create an encoder
    let enc = Encoder::new(payload, destination_image);
    // Encode our message into the alpha channel of the image
    let result = enc.encode_alpha();
    // Save the new image
    save_image_buffer(result, "hidden_message.png".to_string());
    // Load the image with the secret message
    let encoded_image = file_as_image_buffer("hidden_message.png".to_string());
    // Create a decoder
    let dec = Decoder::new(encoded_image);
    // Decode the image by reading the alpha channel
    let out_buffer = dec.decode_alpha();
    // Filter out padding bytes and null bytes
    let clean_buffer: Vec<u8> = out_buffer
        .into_iter()
        .filter(|&b| b != 0xff && b != 0x00)
        .take(payload.len()) // Only take as many bytes as we originally encoded
        .collect();
    println!("Decoded bytes: {:?}", clean_buffer);
    // Convert bytes to string with proper error handling
    let message = String::from_utf8_lossy(&clean_buffer).into_owned();
    println!("Decoded message: {}", message);
    // / / // /  / leghayt hena / nehayt el view encryption / / // / / /
    // decrypt the image
    let encrypted_image = file_as_dynamic_image(encrypted_image_path.to_string()).to_rgba();
    let decoder = Decoder::new(encrypted_image);
    let decrypted_data = decoder.decode_alpha();
    let output_path = "decrypted_image.png";
    std::fs::write(output_path, &decrypted_data)?;
    println!("Decrypted image saved successfully as PNG!");
    Ok(leader_address)
}
pub async fn send_samples(
    socket: &UdpSocket,
    client_id: &str,
    server_address: &str,
) -> io::Result<()> {
    let samples_dir = "samples";

    // Check if the samples folder exists
    if !Path::new(samples_dir).exists() {
        println!("No 'samples/' folder found. Skipping sample upload.");
        return Ok(());
    }

    for entry in fs::read_dir(samples_dir).expect("Failed to read samples directory") {
        if let Ok(file) = entry {
            let file_path = file.path();
            if file_path.is_file() {
                // Extract the original image name without extension
                let original_name = file
                    .file_name()
                    .to_string_lossy()
                    .strip_suffix(".jpg")
                    .unwrap_or(&file.file_name().to_string_lossy())
                    .to_string();

                // Send metadata
                let metadata_message = format!("SAMPLE_UPLOAD:{}:{}", client_id, original_name);
                socket
                    .send_to(metadata_message.as_bytes(), server_address)
                    .await?;
                println!("Sent metadata for sample: {}", original_name);

                // Send file data
                let file_data = fs::read(&file_path).expect("Failed to read file");
                socket.send_to(&file_data, server_address).await?;
                println!("Sent file data for sample: {}", original_name);
            }
        }
    }

    // Notify the server that all samples are sent
    socket.send_to(b"END_SAMPLES", server_address).await?;
    println!("Notified server that all samples are sent.");
    Ok(())
}

pub async fn send_image(socket: &UdpSocket, image_id:&str) -> io::Result<()> {
    // Prompt the user for the image path
    // let mut input = String::new();
    // println!("Enter your Image Path to send to the server: ");
    // io::stdin()
    //     .read_line(&mut input)
    //     .expect("Failed to read line");
    let image_path = format!("{}/{}.jpg", "images", image_id); // images/5

    // Load the image from the given path
    let format =
        image::guess_format(&std::fs::read(image_path.clone()).expect("Failed to read the image file"))
            .unwrap_or(ImageFormat::Jpeg);
    let img = image::open(image_path).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    let mut buf = Cursor::new(Vec::new());
    img.write_to(&mut buf, format)
        .expect("Failed to convert image to bytes");
    let image_bytes = buf.into_inner();

    // Image chunking setup
    let chunk_size = 2044;
    let total_chunks = (image_bytes.len() as f64 / chunk_size as f64).ceil() as usize;

    let mut ack_buffer = [0u8; 1024];
    let max_retries = 5;
    let mut sequence_num: u32 = 0;

    // Send image in chunks
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
            // Send "END" message
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
                            resend_last_batch(i, chunk_size, &image_bytes, socket).await?;
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
                        resend_last_batch(i, chunk_size, &image_bytes, socket).await?;
                    }
                }
            }
        }
    }
    Ok(())
}

// Helper function to resend the last 10 chunks or fewer if near the start
pub async fn resend_last_batch(
    current_index: usize,
    chunk_size: usize,
    image_bytes: &[u8],
    socket: &UdpSocket,
) -> io::Result<()> {
    let start_chunk = (current_index + 1 - 10).max(0);
    for j in start_chunk..=current_index {
        let start = j * chunk_size;
        let end = std::cmp::min(start + chunk_size, image_bytes.len());
        let chunk_data = &image_bytes[start..end];

        let mut chunk = Vec::with_capacity(4 + chunk_data.len());
        chunk.extend_from_slice(&(j as u32).to_be_bytes());
        chunk.extend_from_slice(chunk_data);

        socket.send(&chunk).await?;
        println!("Resent chunk {} with sequence number {}", j + 1, j);
        sleep(Duration::from_millis(5)).await;
    }
    Ok(())
}
// Checks if a file has an image extension.
pub fn is_image_file(file_name: &str) -> bool {
    let image_extensions = ["png", "jpg", "jpeg", "gif"];
    if let Some(extension) = Path::new(file_name).extension() {
        return image_extensions
            .iter()
            .any(|&ext| ext.eq_ignore_ascii_case(extension.to_str().unwrap_or("")));
    }
    false
}

/// Retrieves all image file paths in a directory.
pub fn get_image_paths(dir: &str) -> Result<Vec<String>, std::io::Error> {
    let mut image_paths = Vec::new();

    // Read the directory
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        // If the entry is a file and has an image extension, add to the list
        if path.is_file() {
            if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
                if is_image_file(file_name) {
                    image_paths.push(path.to_string_lossy().to_string());
                }
            }
        }
    }

    Ok(image_paths)
}

pub async fn start_p2p_listener(client_address: &str, samples_dir: &str) -> io::Result<()> {
    let socket = UdpSocket::bind(client_address).await?;
    println!("P2P Listener running on {}", client_address);
    let servers: Vec<SocketAddr> = vec![
        "127.0.0.1:8083".parse().unwrap(),
        "127.0.0.1:8084".parse().unwrap(),
        "127.0.0.1:2010".parse().unwrap(),
    ];

    let clientaddress2 = "127.0.0.1:7005"; //  client address to send image for encryption
    let socket6 = UdpSocket::bind("127.0.0.1:7001").await?; // socket for encrypted image recieving
    let socket2 = UdpSocket::bind(clientaddress2).await?;


    let samples_dir = "images";
    tokio::spawn(async move {
        let mut buffer = [0u8; 4096];
        loop {
            let (amt, peer_addr) = match socket.recv_from(&mut buffer).await {
                Ok((amt, addr)) => (amt, addr),
                Err(e) => {
                    eprintln!("Error receiving P2P request: {:?}", e);
                    continue;
                }
            };

            let received_message = String::from_utf8_lossy(&buffer[..amt]);
            if received_message.starts_with("REQUEST_IMAGE:") {
                // Extract the requested image ID
                let image_id = received_message
                    .strip_prefix("REQUEST_IMAGE:")
                    .unwrap_or("")
                    .trim()
                    .split('_')
                    .nth(1)
                    .unwrap_or("");

                println!(
                    "Received request for image '{}' from {}",
                    image_id, peer_addr
                );
                // Send the ELECT message to the servers
                for addr in &servers {
                    socket6.send_to(b"ELECT", addr).await;
                    println!("Sent ELECT message to {}", addr);
                }
                middleware(&socket2, &socket6, image_id).await;
                
                // Check if the image exists in the samples directory
                // let image_path = format!("{}/{}.jpg", samples_dir, image_id); // images/5
                // if Path::new(&image_path).exists() {
                //     match fs::read(&image_path) {
                //         Ok(image_data) => {
                //             let chunk_size = 1024; // Chunk size for transmission
                //             let total_chunks = (image_data.len() + chunk_size - 1) / chunk_size;

                //             // Send the total number of chunks first
                //             let total_chunks_message = format!("TOTAL_CHUNKS:{}", total_chunks);
                //             socket
                //                 .send_to(total_chunks_message.as_bytes(), peer_addr)
                //                 .await
                //                 .unwrap_or_else(|e| {
                //                     eprintln!(
                //                         "Failed to send total chunks message for image '{}': {:?}",
                //                         image_id, e
                //                     );
                //                     0
                //                 });

                //             // Send the image data in chunks
                //             for (i, chunk) in image_data.chunks(chunk_size).enumerate() {
                //                 let mut message = Vec::new();
                //                 message.extend_from_slice(&(i as u32).to_be_bytes()); // Add chunk number
                //                 message.extend_from_slice(chunk); // Add chunk data

                //                 socket
                //                     .send_to(&message, peer_addr)
                //                     .await
                //                     .unwrap_or_else(|e| {
                //                         eprintln!(
                //                             "Failed to send chunk {} for image '{}': {:?}",
                //                             i, image_id, e
                //                         );
                //                         0
                //                     });
                //                 println!(
                //                     "Sent chunk {}/{} of image '{}' to {}",
                //                     i + 1,
                //                     total_chunks,
                //                     image_id,
                //                     peer_addr
                //                 );
                //             }
                //             println!("Completed sending image '{}' to {}", image_id, peer_addr);
                //         }
                //         Err(e) => {
                //             eprintln!("Failed to read image '{}': {:?}", image_id, e);
                //         }
                //     }
                // } else {
                //     // Notify the peer that the image doesn't exist
                //     let error_message = format!("IMAGE_NOT_FOUND:{}", image_id);
                //     socket
                //         .send_to(error_message.as_bytes(), peer_addr)
                //         .await
                //         .unwrap_or_else(|e| {
                //             eprintln!(
                //                 "Failed to notify peer about missing image '{}': {:?}",
                //                 image_id, e
                //             );
                //             0
                //         });
                //     println!("Image '{}' not found. Notified {}", image_id, peer_addr);
                // }
            }
        }
    });

    Ok(())
}

pub async fn request_image_by_id(
    socket: &UdpSocket,
    image_id: &str,
    client_map: &HashMap<String, String>,
) -> io::Result<()> {
    // Determine the client_id (folder name)
    let client_id = image_id.split('_').next().unwrap_or("").to_string();

    if let Some(peer_address) = client_map.get(&client_id) {
        // Send the request to the correct peer
        let request_message = format!("REQUEST_IMAGE:{}", image_id);
        socket
            .send_to(request_message.as_bytes(), peer_address)
            .await?;
        println!("Requested image '{}' from peer {}", image_id, peer_address);

        // Prepare to receive the total number of chunks
        let mut buffer = [0u8; 4096];
        let (amt, _) = socket.recv_from(&mut buffer).await?;

        let total_chunks_message = String::from_utf8_lossy(&buffer[..amt]);
        if total_chunks_message.starts_with("TOTAL_CHUNKS:") {
            let total_chunks: usize = total_chunks_message
                .strip_prefix("TOTAL_CHUNKS:")
                .unwrap_or("0")
                .trim()
                .parse()
                .unwrap_or(0);

            println!("Expecting {} chunks for image '{}'", total_chunks, image_id);

            // Collect all chunks
            let mut received_chunks = vec![None; total_chunks];
            for _ in 0..total_chunks {
                let (amt, _) = socket.recv_from(&mut buffer).await?;
                let chunk_number = u32::from_be_bytes(buffer[0..4].try_into().unwrap()) as usize;
                let chunk_data = &buffer[4..amt];

                if chunk_number < total_chunks {
                    received_chunks[chunk_number] = Some(chunk_data.to_vec());
                    println!(
                        "Received chunk {}/{} for image '{}'",
                        chunk_number + 1,
                        total_chunks,
                        image_id
                    );
                }
            }

            // Reassemble the image
            let mut image_data = Vec::new();
            for chunk in received_chunks.into_iter().flatten() {
                image_data.extend_from_slice(&chunk);
            }

            // Save the reassembled image
            let received_samples_dir = "received_samples";
            std::fs::create_dir_all(received_samples_dir)
                .expect("Failed to create 'received_samples' directory");

            let image_path = format!("{}/{}.jpg", received_samples_dir, image_id);
            std::fs::write(&image_path, &image_data).expect("Failed to save received image");
            println!("Received and saved image '{}' from peer.", image_id);
        } else if total_chunks_message.starts_with("IMAGE_NOT_FOUND:") {
            println!("Peer responded: Image '{}' not found.", image_id);
        } else {
            println!("Unexpected response from peer: {}", total_chunks_message);
        }
    } else {
        println!("No online peer found for client_id '{}'.", client_id);
    }

    Ok(())
}
