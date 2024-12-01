use image::ImageFormat;
use std::fs;
use std::path::Path;
use tokio::net::UdpSocket;

use std::collections::HashMap;
use tokio::time::{sleep, timeout, Duration};

use image::{ImageBuffer, Rgba, RgbaImage};
use std::collections::HashSet;
use std::convert::TryInto;
use std::fs::File;
use std::fs::OpenOptions;
use std::io::{self, Cursor, Write};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use steganography::decoder::Decoder;
use steganography::encoder::Encoder;
use steganography::util::bytes_to_str;
use steganography::util::file_as_dynamic_image;
use steganography::util::file_as_image_buffer;
use steganography::util::save_image_buffer;
use steganography::util::str_to_bytes;
use tokio::signal;
use tokio::time;

struct ImageStats {
    client_id: String, // Unique identifier
    num_of_views: u8,  // Number of views (using unsigned 8-bit integer)
}

fn embed_data_in_image(
    image_path: &str,
    views: u32,
    client_id: u32,
    output_path: &str,
) -> io::Result<()> {
    // Load the image
    let original_image = image::open(image_path)
        .expect("Failed to open image")
        .into_rgba8();

    let (width, height) = original_image.dimensions();

    // Create a new image with an extra row
    let mut new_image: RgbaImage = ImageBuffer::new(width, height + 1);

    // Copy original image data
    for y in 0..height {
        for x in 0..width {
            new_image.put_pixel(x, y, *original_image.get_pixel(x, y));
        }
    }

    // Convert views and client_id to bytes
    let views_bytes = views.to_be_bytes();
    let client_id_bytes = client_id.to_be_bytes();

    // Embed metadata in the extra row
    // First 4 pixels store views, next 4 pixels store client_id
    for i in 0..4u32 {
        new_image.put_pixel(i, height, Rgba([views_bytes[i as usize], 0, 0, 255]));

        new_image.put_pixel(
            i + 4,
            height,
            Rgba([client_id_bytes[i as usize], 0, 0, 255]),
        );
    }

    // Save the modified image
    new_image
        .save(output_path)
        .expect("Failed to save the image");

    println!("Data embedded in image and saved to {}", output_path);
    Ok(())
}

fn extract_data_from_image(image_path: &str) -> (u32, u32) {
    // Load the image
    let image = image::open(image_path)
        .expect("Failed to open image")
        .into_rgba8();

    let (_, height) = image.dimensions();
    let metadata_row = height - 1;

    // Extract views bytes
    let mut views_bytes = [0u8; 4];
    for i in 0..4usize {
        views_bytes[i] = image.get_pixel(i.try_into().unwrap(), metadata_row)[0];
    }

    // Extract client_id bytes
    let mut client_id_bytes = [0u8; 4];
    for i in 0..4usize {
        let x: u32 = (i + 4).try_into().unwrap();
        client_id_bytes[i] = image.get_pixel(x, metadata_row)[0];
    }

    let views = u32::from_be_bytes(views_bytes);
    let client_id = u32::from_be_bytes(client_id_bytes);

    (views, client_id)
}

fn strip_metadata_row(image_path: &str, output_path: &str) -> io::Result<()> {
    // Load the image
    let image = image::open(image_path)
        .expect("Failed to open image")
        .into_rgba8();

    let (width, height) = image.dimensions();

    // Create new image without the metadata row
    let mut stripped_image: RgbaImage = ImageBuffer::new(width, height - 1);

    // Copy all rows except the last one
    for y in 0..height - 1 {
        for x in 0..width {
            stripped_image.put_pixel(x, y, *image.get_pixel(x, y));
        }
    }

    // Save the stripped image
    stripped_image
        .save(output_path)
        .expect("Failed to save the image");
    println!(
        "Stripped metadata row from image and saved to {}",
        output_path
    );

    Ok(())
}

pub async fn middleware(
    socket6: &UdpSocket,
    image_id: &str,
    reinitiated: &str,
    peer_id:&str
) -> io::Result<String> {
    let mut buffer = [0u8; 2048];
    let mut leader_address = String::new();

    loop {
        // Set a timeout for receiving the leader address and acknowledgment
        let socket = UdpSocket::bind(reinitiated).await?;

        let receive_result = timeout(Duration::from_secs(5), socket.recv_from(&mut buffer)).await;
        match receive_result {
            Ok(Ok((size, _))) => {
                let message = String::from_utf8_lossy(&buffer[..size]);
                println!("{}", message);

                if message.starts_with("LEADER_ACK:") {
                    leader_address = message.replace("LEADER_ACK:", "").trim().to_string();
                    println!(
                        "Leader identified at {}. Proceeding to connect...",
                        leader_address
                    );
                    socket.connect(&leader_address).await?; // make this the leader address after the ack is sent back from the server
                    send_image(&socket, image_id).await?;
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

    let views = 5;
    let peer_id = u32::from_str_radix(peer_id, 10).unwrap();
    embed_data_in_image(encrypted_image_path, views, peer_id, encrypted_image_path)?;
    // Extract the data back

    // strip_metadata_row(encrypted_image_path, encrypted_image_path)?;

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

pub async fn send_image(socket: &UdpSocket, image_id: &str) -> io::Result<()> {
    // Prompt the user for the image path
    // let mut input = String::new();
    // println!("Enter your Image Path to send to the server: ");
    // io::stdin()
    // .read_line(&mut input)
    // .expect("Failed to read line");
    let image_path = format!("{}/{}.jpg", "images", image_id); // images/5

    // Load the image from the given path
    let format = image::guess_format(
        &std::fs::read(image_path.clone()).expect("Failed to read the image file"),
    )
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

pub async fn start_p2p_listener(client_address: &str, client: &str) -> io::Result<()> {
    let server1 = "127.0.0.1";
    let server2 = "127.0.0.1";
    let server3 = "127.0.0.1";
    /////////////////////////////////////////////////////////////////////////////
    let server1_address = format!("{}:{}", server1, "8083");
    let server2_addres = format!("{}:{}", server2, "8084");
    let server3_address = format!("{}:{}", server3, "2010");
    /////////////////////////////////////////////////////////////////////////////
    println!("P2P Listener running on {}", client_address);

    let servers: Vec<SocketAddr> = vec![
        server1_address.parse().unwrap(),
        server2_addres.parse().unwrap(),
        server3_address.parse().unwrap(),
    ];

    let client_election_and_image = format!("{}:{}", client, "7005"); // client address to send image for encryption
    let client_encyrpted_image_back = format!("{}:{}", client, "7001"); // client address to send image for encryption
    let socket = UdpSocket::bind(client_address).await?;
    let socket6 = UdpSocket::bind(client_encyrpted_image_back).await?; // socket for encrypted image recieving

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
            if received_message.starts_with("REQUEST_IMAGE_FROM") {
                // Extract the part after "REQUEST_IMAGE_FROM" and remove the prefix
                let after_prefix = received_message
                    .strip_prefix("REQUEST_IMAGE_FROM")
                    .unwrap_or("")
                    .trim();

                // Split the remaining part by ':' to separate the IP from the image ID
                let parts: Vec<&str> = after_prefix.split(':').collect();

                if parts.len() == 2 {
                    let requester_ip = parts[0]; // The IP address of the requester
                    let full_image_id = parts[1]; // The full image ID requested

                    // Remove the leading part before the underscore (e.g., "6_" part)
                    let image_id = full_image_id.split('_').nth(1).unwrap_or(full_image_id);

                    println!(
                        "Received request for image '{}' from requester IP: {}",
                        image_id, requester_ip
                    );

                    // Send the ELECT message to the servers
                    for addr in &servers {
                        socket6.send_to(b"ELECT", addr).await.unwrap_or_else(|e| {
                            eprintln!("Failed to send ELECT message to {}", addr);
                            0
                        });
                        println!("Sent ELECT message to {}", addr);
                    }
                    middleware(&socket6, image_id, &client_election_and_image, &requester_ip).await;

                    // Check if the image exists in the samples directory
                    let image_path = format!("encrypted_image_from_server.png"); // images/5
                    if Path::new(&image_path).exists() {
                        match fs::read(&image_path) {
                            Ok(image_data) => {
                                let chunk_size = 1024; // Chunk size for transmission
                                let total_chunks = (image_data.len() + chunk_size - 1) / chunk_size;

                                // Send the total number of chunks first
                                let total_chunks_message = format!("TOTAL_CHUNKS:{}", total_chunks);
                                socket
                                    .send_to(total_chunks_message.as_bytes(), peer_addr)
                                    .await
                                    .unwrap_or_else(|e| {
                                        eprintln!(
                                        "Failed to send total chunks message for image '{}': {:?}",
                                        image_id, e
                                    );
                                        0
                                    });

                                // Send the image data in chunks with ACK/NACK handling
                                for (i, chunk) in image_data.chunks(chunk_size).enumerate() {
                                    let mut message = Vec::new();
                                    message.extend_from_slice(&(i as u32).to_be_bytes()); // Add chunk number
                                    message.extend_from_slice(chunk); // Add chunk data

                                    loop {
                                        // Send the chunk
                                        socket.send_to(&message, peer_addr).await.unwrap_or_else(
                                            |e| {
                                                eprintln!(
                                                    "Failed to send chunk {} for image '{}': {:?}",
                                                    i, image_id, e
                                                );
                                                0
                                            },
                                        );

                                        println!(
                                            "Sent chunk {}/{} of image '{}' to {}",
                                            i + 1,
                                            total_chunks,
                                            image_id,
                                            peer_addr
                                        );

                                        // Wait for ACK/NACK or timeout
                                        let mut ack_buffer = [0u8; 128];
                                        match tokio::time::timeout(
                                            std::time::Duration::from_secs(2),
                                            socket.recv_from(&mut ack_buffer),
                                        )
                                        .await
                                        {
                                            Ok(Ok((amt, _))) => {
                                                let response =
                                                    String::from_utf8_lossy(&ack_buffer[..amt]);
                                                if response == format!("ACK:{}", i) {
                                                    println!("Received ACK for chunk {}", i);
                                                    break; // Move to the next chunk
                                                } else if response == format!("NACK:{}", i) {
                                                    println!(
                                                        "Received NACK for chunk {}, resending...",
                                                        i
                                                    );
                                                    continue; // Resend the chunk
                                                } else {
                                                    eprintln!("Unexpected response: {}", response);
                                                }
                                            }
                                            Ok(Err(e)) => {
                                                eprintln!("Error receiving ACK/NACK: {:?}", e);
                                                continue; // Retry sending the chunk
                                            }
                                            Err(_) => {
                                                println!("Timeout waiting for ACK/NACK for chunk {}, resending...", i);
                                                continue; // Retry sending the chunk
                                            }
                                        }
                                    }
                                }

                                println!("Completed sending image '{}' to {}", image_id, peer_addr);
                            }
                            Err(e) => {
                                eprintln!("Failed to read image '{}': {:?}", image_id, e);
                            }
                        }
                    } else {
                        // Notify the peer that the image doesn't exist
                        let error_message = format!("IMAGE_NOT_FOUND:{}", image_id);
                        socket
                            .send_to(error_message.as_bytes(), peer_addr)
                            .await
                            .unwrap_or_else(|e| {
                                eprintln!(
                                    "Failed to notify peer about missing image '{}': {:?}",
                                    image_id, e
                                );
                                0
                            });
                        println!("Image '{}' not found. Notified {}", image_id, peer_addr);
                    }
                } else {
                    println!("Invalid request format: {}", received_message);
                }
            }

            else if received_message.starts_with("CONTROL_UPDATE") {

                let update_data = received_message.strip_prefix("CONTROL_UPDATE:").unwrap_or("").to_string();
                let parts: Vec<&str> = update_data.split(':').collect();
                
                if parts.len() == 3 {
                    let client_id = parts[0].to_string();
                    let image_id = parts[1].to_string();
                    let views = parts[2].to_string(); 
                    
                    println!("Received control update - Client ID: {}, Image ID: {}, New Views: {}", client_id, image_id, views);
            
                    let stored_client_id = client_id;
                    let stored_image_id = image_id;
                    let stored_views = views;
            
                    // access received_images folder and update the views count
                    let views_dir = "views_count";
                    let views_file = format!("{}/{}_views.txt", views_dir, stored_image_id);
                    
                    // Open the file for writing and truncate (overwrite) its contents
                    let mut file = OpenOptions::new()
                        .write(true)
                        .truncate(true)  // This ensures the file is overwritten, not appended
                        .open(views_file)
                        .expect("Failed to open file");
                    
                    // Write the new view count to the file
                    file.write_all(stored_views.as_bytes())
                        .expect("Failed to write to file");
            
                    // For now, we print them or store them for future use
                    println!("Stored client_id: {}, image_id: {}, views: {}", stored_client_id, stored_image_id, stored_views);
                }
                 
                else {
                    println!("Received invalid CONTROL_UPDATE format.");
                }
            }   
        }

    });
    Ok(())
}

pub async fn request_image_by_id(
    socket: &UdpSocket,
    image_id: &str,
    client_map: &HashMap<String, String>,
    my_ip: &str,
) -> io::Result<()> {
    // Determine the client_id (folder name)
    let client_id = image_id.split('_').next().unwrap_or("").to_string();
    if let Some(peer_address) = client_map.get(&client_id) {
        // Send the request to the correct peer
        let request_message = format!("REQUEST_IMAGE_FROM{}:{}", my_ip, image_id);
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

            // Prepare to collect chunks with ACK/NACK
            let mut received_chunks = vec![None; total_chunks];
            let mut missing_chunks: HashSet<usize> = (0..total_chunks).collect();

            while !missing_chunks.is_empty() {
                let (amt, src_addr) = socket.recv_from(&mut buffer).await?;
                let chunk_number = u32::from_be_bytes(buffer[0..4].try_into().unwrap()) as usize;
                let chunk_data = &buffer[4..amt];

                if chunk_number < total_chunks && missing_chunks.contains(&chunk_number) {
                    received_chunks[chunk_number] = Some(chunk_data.to_vec());
                    missing_chunks.remove(&chunk_number);

                    println!(
                        "Received chunk {}/{} for image '{}'",
                        chunk_number + 1,
                        total_chunks,
                        image_id
                    );

                    // Send an ACK for the received chunk
                    let ack_message = format!("ACK:{}", chunk_number);
                    socket.send_to(ack_message.as_bytes(), src_addr).await?;
                } else {
                    // Send a NACK if the chunk is invalid or duplicate
                    let nack_message = format!("NACK:{}", chunk_number);
                    socket.send_to(nack_message.as_bytes(), src_addr).await?;
                }
            }

            println!("All chunks received for image '{}'", image_id);

            // Reassemble the image
            let mut image_data = Vec::new();
            for chunk in received_chunks.into_iter().flatten() {
                image_data.extend_from_slice(&chunk);
            }

            // Save the reassembled image
            let received_images_dir = "received_images";
            std::fs::create_dir_all(received_images_dir)
                .expect("Failed to create 'received_images' directory");

            let image_path = format!("{}/{}.png", received_images_dir, image_id);
            std::fs::write(&image_path, &image_data).expect("Failed to save received image");
            println!("Received and saved image '{}' from peer.", image_id);

            // Decrypting the image
            let encrypted_image = file_as_dynamic_image(image_path.to_string()).to_rgba();
            ////////////////////////////////////////////////////////////////////////////////////////////////////
            let (extracted_views, extracted_client_id) =
                extract_data_from_image(&image_path.to_string());
            println!(
                "Extracted Views: {}, Extracted Client ID: {}",
                extracted_views, extracted_client_id
            );
            ////////////////////////////////////////////////////////////////////////////////////////////////////

            // view file

            let views_dir = "views_count";
            std::fs::create_dir_all(views_dir).expect("Failed to create 'views' directory");

            let views_file = format!("{}/{}_views.txt", views_dir, image_id);
            let mut file = File::create(views_file).expect("Failed to create file");
            file.write_all(extracted_views.to_string().as_bytes())
                .expect("Failed to write to file");

            strip_metadata_row(&image_path, &image_path).expect("Failed to strip metadata row");

            let decoder = Decoder::new(encrypted_image);
            let decrypted_data = decoder.decode_alpha();
            let output_path = "decrypted_image.png";
            std::fs::write(output_path, &decrypted_data)?;
            println!("Decrypted image saved successfully as PNG!");
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

pub async fn decrypt(image_path: String) -> io::Result<()> {
    let encrypted_image = file_as_dynamic_image(image_path.to_string()).to_rgba();
    let decoder = Decoder::new(encrypted_image);
    let decrypted_data = decoder.decode_alpha();
    let output_path = "decrypted_image.png";
    std::fs::write(output_path, &decrypted_data)?;
    println!("Decrypted image saved successfully as PNG!");
    Ok(())
}
