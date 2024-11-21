use csv::Writer;
use image::{DynamicImage, ImageFormat, RgbaImage};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs;
use std::fs::read_to_string;
use std::fs::File;
use std::fs::OpenOptions;
use std::io::{self, Cursor, Write};
use std::net::SocketAddr;
use std::path::Path;
use std::time::Instant;
use steganography::decoder::Decoder;
use steganography::encoder::Encoder;
use steganography::util::bytes_to_str;
use steganography::util::file_as_dynamic_image;
use steganography::util::file_as_image_buffer;
use steganography::util::save_image_buffer;
use steganography::util::str_to_bytes;
use tokio::net::UdpSocket;
use tokio::signal;
use tokio::time::{sleep, timeout, Duration};

// struct for image stats
struct ImageStats {
    client_id: String, // Unique identifier
    img_id: String,    // Image identifier
    num_of_views: u8,  // Number of views (using unsigned 8-bit integer)
}
// struct for online status
#[derive(Serialize, Deserialize)]
struct OnlineStatus {
    status: bool,
    client_id: String,
}

async fn send_image(socket: &UdpSocket) -> io::Result<()> {
    // Prompt the user for the image path
    let mut input = String::new();
    println!("Enter your Image Path to send to the server: ");
    io::stdin()
        .read_line(&mut input)
        .expect("Failed to read line");
    let image_path = input.trim(); // Trim to remove any extraneous whitespace or newlines

    // Load the image from the given path
    let format =
        image::guess_format(&std::fs::read(image_path).expect("Failed to read the image file"))
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
async fn resend_last_batch(
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
fn is_image_file(file_name: &str) -> bool {
    let image_extensions = ["png", "jpg", "jpeg", "gif"];
    if let Some(extension) = Path::new(file_name).extension() {
        return image_extensions
            .iter()
            .any(|&ext| ext.eq_ignore_ascii_case(extension.to_str().unwrap_or("")));
    }
    false
}

/// Retrieves all image file paths in a directory.
fn get_image_paths(dir: &str) -> Result<Vec<String>, std::io::Error> {
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

// The middleware function containing the logic for the client such as communicating with the servers and sending messages to the servers
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

    let image_path = "/Users/ramygad/Desktop/11_19 dist/P2P_ImageSharing/P2P_ImageSharing/Server/images/mask.jpg";
    send_image(&socket).await?;

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
    Ok(())
}

fn handle_auth() -> io::Result<bool> {
    // get UID from directory of service from server instead (just a placeholder)
    let mut uid = "127.0.0.1_1".to_string();
    let path = Path::new("uid.txt");

    if path.exists() {
        println!("File exists!");
        let contents = read_to_string("uid.txt")?;
        if contents == uid {
            return Ok(true);
        }
    } else {
        println!("File does not exist!");
    }

    println!("Register or login? (r/l):");
    let mut choice = String::new();
    io::stdin()
        .read_line(&mut choice)
        .expect("Failed to read input!");

    match choice.trim().to_lowercase().as_str() {
        "r" => {
            let mut file = File::create("uid.txt")?;
            file.write_all(uid.as_bytes())?;
            println!("UID: {}", uid);
            Ok(true)
        }
        "l" => {
            println!("Enter your UID:");
            io::stdin()
                .read_line(&mut uid)
                .expect("Failed to read UID!");
            // if UID matches what is in the directory of service, set flag to true and enter the loop
            // otherwise terminate!
            Ok(true)
        }
        _ => Ok(false),
    }
}

#[tokio::main]
async fn main() -> io::Result<()> {
    let mut count = 0;
    let authenticated = handle_auth()?;
    if authenticated {
        println!("Authentication successful!");
    } else {
        println!("Authentication failed!");
    }
    
    if authenticated {
        loop {
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

            // struct for directory of service filling
            let info = OnlineStatus {
                status: true,
                client_id: "5".to_string(),
            };

            let info_bytes_online = serde_json::to_vec(&info).unwrap();

            // multicast to all servers
            let servers: Vec<SocketAddr> = vec![
                "127.0.0.1:8083".parse().unwrap(),
                "127.0.0.1:8084".parse().unwrap(),
                "127.0.0.1:2010".parse().unwrap(),
            ];
            let clientaddress = "127.0.0.1:8080"; // my client server address
            let socket = UdpSocket::bind(clientaddress).await?;
            let socket6 = UdpSocket::bind("127.0.0.1:2005").await?; // socket for encrypted image recieving

            // sending the client status and info to a single server
            if count == 0 {
                socket.send_to(&info_bytes_online, servers[0]).await?;
                count += 1;
            }
            socket.send_to(&info_bytes_online, servers[0]).await?;

            println!("
        |---------------------------------------------------------------|
        |    1) If you want to send an image, please enter (y) or (Y)   |
        |                                                               |
        |    2) If you want to exit,             please enter (E) or (e)|   
        |---------------------------------------------------------------|
 "
        );

            let mut input = String::new();
            io::stdin()
                .read_line(&mut input)
                .expect("Failed to read input");

            // if yes, send ELECT message to all servers
            if input.trim().eq_ignore_ascii_case("y") || input.trim().eq_ignore_ascii_case("Y") {
                for addr in &servers {
                    socket.send_to(b"ELECT", addr).await?;
                    println!("message sent to {}", addr);
                }
            } else if input.trim().eq_ignore_ascii_case("e") || input.trim().eq_ignore_ascii_case("E") {
                println!("Exiting...");

                let info = OnlineStatus {
                    status: false,
                    client_id: "5".to_string(),
                };
                let info_bytes_offline= serde_json::to_vec(&info).unwrap();
                socket.send_to(&info_bytes_offline, servers[0]).await?;
                break;
            } 
            else {
                println!("Invalid input. Please try again.");
            }
            // Call the middleware function that handles everything
            middleware(&socket, &socket6).await?;
        }
    }
    Ok(())
}
