use csv::Writer;
use image::{DynamicImage, ImageFormat, RgbaImage};
use rand::prelude::SliceRandom;
use rand::thread_rng;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::fs::read_to_string;
use std::fs::File;
use std::fs::OpenOptions;
use std::io::{self, Cursor, Write};
use std::net::SocketAddr;
use std::path::Path;
use std::sync::{Arc, Mutex};
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
use tokio::time;
use tokio::time::{sleep, timeout, Duration};

mod communication;
use communication::request_image_by_id;
use communication::send_image;
use communication::send_samples;
use communication::start_p2p_listener;

// struct for image stats
struct ImageStats {
    client_id: String, // Unique identifier
    img_id: String,    // Image identifier
    num_of_views: u8,  // Number of views (using unsigned 8-bit integer)
}
// struct for online status
#[derive(Serialize, Deserialize)]
struct OnlineStatus {
    ip: String,
    status: bool,
    client_id: String,
}

async fn parse_and_store_dos(dos_content: &str) -> HashMap<String, String> {
    let mut client_map = HashMap::new();

    for line in dos_content.lines().skip(1) {
        // Skip the header
        let parts: Vec<&str> = line.split(',').collect();
        if parts.len() == 3 {
            let ip_port = parts[0].trim().to_string();
            let client_id = parts[1].trim().to_string();
            let status = parts[2].trim();
            if status == "true" {
                client_map.insert(client_id, ip_port);
            }
        }
    }

    println!("Parsed DoS: {:?}", client_map);
    client_map
}

// The middleware function containing the logic for the client such as communicating with the servers and sending messages to the servers
async fn middleware(socket: &UdpSocket, socket6: &UdpSocket) -> io::Result<String> {
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
    Ok(leader_address)
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
    let mut leader_address = String::new();
    let servers: Vec<SocketAddr> = vec![
        "127.0.0.1:8083".parse().unwrap(),
        "127.0.0.1:8084".parse().unwrap(),
        "127.0.0.1:2010".parse().unwrap(),
    ];
    println!("before");
    let mut rng = thread_rng();
    let mut assistant: SocketAddr = *servers.choose(&mut rng).unwrap();
    println!("After");

    if authenticated {
        println!("Authentication successful!");
    } else {
        println!("Authentication failed!");
    }

    let client_map = Arc::new(Mutex::new(HashMap::new()));

    if authenticated {
        let p2p_listener = "127.0.0.1:8079";
        let mut samples_sent = false;
        start_p2p_listener(&p2p_listener, "samples").await?;
        loop {
            let clientaddress = "127.0.0.1:8080"; // my client server address
            let socket = UdpSocket::bind(clientaddress).await?;
            let socket6 = UdpSocket::bind("127.0.0.1:2005").await?; // socket for encrypted image recieving

            let info = OnlineStatus {
                ip: p2p_listener.to_string(),
                status: true,
                client_id: "5".to_string(),
            };
            let serialized_info = serde_json::to_string(&info).unwrap();
            // state a timeout for the client to send the status and receive STATUS_ACK, if not received send again after timeout
            let timeout = Duration::from_secs(1);
            let mut start_time = Instant::now();
            let mut received_acks = false;
            let message_to_send = format!("STATUS:{}", serialized_info);
            if count == 0 {
                socket
                    .send_to(message_to_send.as_bytes(), assistant)
                    .await?;

                let mut received_acks = false;
                

                while !received_acks {
                    let timeout_duration = Duration::from_secs(1);
                    let mut buf = [0; 1024];

                    match time::timeout(timeout_duration, socket.recv_from(&mut buf)).await {
                        Ok(Ok((size, _))) => {
                            let received_message = String::from_utf8_lossy(&buf[..size]);
                            if received_message.contains("STATUS_ACK") {
                                println!("Received STATUS_ACK from server");
                                received_acks = true;

                                // Send samples only if they haven't been sent already
                                if !samples_sent {
                                    send_samples(&socket, &info.client_id, &assistant.to_string())
                                        .await?;
                                    samples_sent = true; // Mark samples as sent
                                }
                            }
                        }
                        _ => {
                            // Timeout occurred
                            assistant = *servers.choose(&mut rng).unwrap(); // Randomize IP
                            println!(
                                "Timeout occurred, resending STATUS message to {}",
                                assistant
                            );
                            socket
                                .send_to(message_to_send.as_bytes(), assistant)
                                .await?;
                        }
                    }
                }
                count += 1;
            }
            // sending the client status and info to a single server
            println!(
                "
        Welcome to our P2P Application :)
     --------------------------------------------------------------
    |  1) If you want to get the DoS,      please enter (D) or (d) |
    |  2) If you want to request an image, please enter (R) or (r) |
    |  3) If you want to exit,             please enter (E) or (e) |   
    |  4) If you want to view your images, please enter (V) or (v) |   
     -------------------------------------------------------------- "
            );
            let mut input = String::new();
            io::stdin()
                .read_line(&mut input)
                .expect("Failed to read input");


                if input.trim().eq_ignore_ascii_case("y") || input.trim().eq_ignore_ascii_case("Y") {
                    for addr in &servers {
                        socket.send_to(b"ELECT", addr).await?;
                        println!("message sent to {}", addr);
                    }
                    middleware(&socket, &socket6).await?;
                } else if input.trim().eq_ignore_ascii_case("r") {
                    // Request image by ID
                    println!("Enter image ID to request (e.g., 5_0): ");
                    let mut image_id = String::new();
                    io::stdin()
                        .read_line(&mut image_id)
                        .expect("Failed to read image ID");
    
                    let client_map_locked = client_map.lock().unwrap();
                    request_image_by_id(&socket, image_id.trim(), &*client_map_locked).await?;
                }  else if input.trim().eq_ignore_ascii_case("e")
                || input.trim().eq_ignore_ascii_case("E")
            {
                println!("Exiting...");

                let info = OnlineStatus {
                    ip: p2p_listener.to_string(),
                    status: false,
                    client_id: "5".to_string(),
                };
                let serialized_info = serde_json::to_string(&info).unwrap();
                let message_to_send = format!("STATUS:{}", serialized_info);
                socket
                    .send_to(&message_to_send.as_bytes(), assistant)
                    .await?;
                break;
            } else if input.trim().eq_ignore_ascii_case("d")
                || input.trim().eq_ignore_ascii_case("D")
            {
                // Request DoS and samples
                let message = "Request_DOS";
                socket.send_to(message.as_bytes(), assistant).await?;
                println!("Requested DOS!");

                // Wait to receive the DoS and samples
                let mut buffer = [0u8; 4096];
                let received_samples_dir = "received_samples";
                std::fs::create_dir_all(received_samples_dir)
                    .expect("Failed to create 'received_samples' directory");

                let mut dos_content = String::new(); // Collect the DoS content
                let mut samples_received = false; // Track if samples were received

                loop {
                    let (amt, _) = socket.recv_from(&mut buffer).await?;
                    let received_message = String::from_utf8_lossy(&buffer[..amt]).to_string();

                    if received_message == "ACK" {
                        // Ignore ACK messages
                        println!("Received ACK, ignoring...");
                        continue;
                    } else if received_message.starts_with("DOS:") {
                        // Process the directory of service
                        dos_content = received_message
                            .strip_prefix("DOS:")
                            .unwrap_or("")
                            .to_string();
                        println!("Received DoS: {}", dos_content);

                        // Parse and store DoS data
                        let mut client_map_locked = client_map.lock().unwrap();
                        *client_map_locked = parse_and_store_dos(&dos_content).await;
                    } else if received_message.starts_with("SAMPLE:") {
                        samples_received = true;

                        // Process sample metadata
                        let parts: Vec<&str> = received_message
                            .strip_prefix("SAMPLE:")
                            .unwrap_or("")
                            .split(':')
                            .collect();
                        if parts.len() == 2 {
                            let client_id = parts[0];
                            let sample_name = parts[1];
                            println!("Receiving sample {} from client {}", sample_name, client_id);

                            // Receive the sample data
                            let (data_size, _) = socket.recv_from(&mut buffer).await?;
                            let sample_path =
                                format!("{}/{}/{}", received_samples_dir, client_id, sample_name);

                            // Ensure client-specific directory exists
                            let client_samples_dir =
                                format!("{}/{}", received_samples_dir, client_id);
                            std::fs::create_dir_all(&client_samples_dir).expect(
                                "Failed to create client-specific received_samples directory",
                            );

                            // Save the received sample
                            std::fs::write(&sample_path, &buffer[..data_size])
                                .expect("Failed to write received sample");
                            println!("Saved received sample: {}", sample_path);
                        }
                    } else if received_message == "NO_SAMPLES" {
                        println!("No samples available on the server.");
                        break;
                    } else if received_message == "SAMPLES_DONE" {
                        println!("All samples have been received.");
                        break;
                    } else {
                        println!("Unknown message: {}", received_message);
                    }
                }

                // If no samples were received, inform the user
                if !samples_received {
                    println!("No samples were transmitted during the DoS request.");
                }
            } else {
                println!("Invalid input. Please try again.");
            }
        }
    }
    Ok(())
}
