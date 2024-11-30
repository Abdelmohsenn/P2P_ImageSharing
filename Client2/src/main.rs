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
use tokio::net::UdpSocket;
use tokio::signal;
use tokio::time;
use tokio::time::{sleep, timeout, Duration};
use std::process::Command;
mod middleware;
use middleware::middleware;
use middleware::request_image_by_id;
use middleware::send_samples;
use middleware::start_p2p_listener;

// struct for image stats

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
pub async fn main() -> io::Result<()> {
    /////////////////////////////////////////////////////////////////////////////
    let client = "127.0.0.1"; // to be changed to the real client address

    let server1 = "127.0.0.1";
    let server2 = "127.0.0.1";
    let server3 = "127.0.0.1";
    /////////////////////////////////////////////////////////////////////////////
    let server1_address = format!("{}:{}", server1, "8083");
    let server2_addres = format!("{}:{}", server2, "8084");
    let server3_address = format!("{}:{}", server3, "2010");
    /////////////////////////////////////////////////////////////////////////////
    let mut count = 0;
    let authenticated = handle_auth()?;
    let mut leader_address = String::new();
    let servers: Vec<SocketAddr> = vec![
        server1_address.parse().unwrap(),
        server2_addres.parse().unwrap(),
        server3_address.parse().unwrap(),
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
        let p2p_listener = format!("{}:{}", client, "6999");
        let mut samples_sent = false;
        start_p2p_listener(&p2p_listener, &client).await?;
        loop {
            let clientaddress = format!("{}:{}", client, "7000");
            let socket = UdpSocket::bind(clientaddress).await?;

            let info = OnlineStatus {
                ip: p2p_listener.to_string(),
                status: true,
                client_id: "6".to_string(),
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
    |  5) If you want to Control access rights,   enter (C) or (c) |   
     -------------------------------------------------------------- "
            );
            let mut input = String::new();
            io::stdin()
                .read_line(&mut input)
                .expect("Failed to read input");

            // if input.trim().eq_ignore_ascii_case("y") || input.trim().eq_ignore_ascii_case("Y") {
            //     for addr in &servers {
            //         socket.send_to(b"ELECT", addr).await?;
            //         println!("message sent to {}", addr);
            //     }
            //     // middleware(&socket2, &socket6, "5").await?;
            // }
            if input.trim().eq_ignore_ascii_case("r") {
                // Request image by ID
                println!("Enter image ID to request (e.g., 5_0): ");
                let mut image_id = String::new();
                io::stdin()
                    .read_line(&mut image_id)
                    .expect("Failed to read image ID");

                let client_map_locked = client_map.lock().unwrap();
                request_image_by_id(
                    &socket,
                    image_id.trim(),
                    &*client_map_locked,
                    &info.client_id,
                )
                .await?;
            } 
            
            else if input.trim().eq_ignore_ascii_case("e")
                || input.trim().eq_ignore_ascii_case("E")
            {
                println!("Exiting...");

                let info = OnlineStatus {
                    ip: p2p_listener.to_string(),
                    status: false,
                    client_id: "6".to_string(),
                };
                let serialized_info = serde_json::to_string(&info).unwrap();
                let message_to_send = format!("STATUS:{}", serialized_info);
                socket
                    .send_to(&message_to_send.as_bytes(), assistant)
                    .await?;
                break;
            } 
            else if input.trim().eq_ignore_ascii_case("d")
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
            } 

         else if (input.trim().eq_ignore_ascii_case("v") || input.trim().eq_ignore_ascii_case("V")){
                
                // receive input for image id
                println!("Enter image ID to view: ");
                let mut image_id = String::new();
                io::stdin()
                    .read_line(&mut image_id)
                    .expect("Failed to read image ID");

                // go into received_images directory

                // Open the image
                println!("Opening image...");
                let views_path = "views_count";
                let file_path = format!("{}/{}_views.txt", views_path, image_id.trim());
                let decrypted_path  = "decrypted_image.png";
                let views = fs::read_to_string(&file_path
                ).expect("Failed to read views file");

                // Check for platform and run the appropriate command
                if views == "0" {
                    eprintln!("You have no views left for this image.");
                    continue;
                } else {
                    let received_images_dir = "received_images";

                // if no received_images directory exists, raise an error

                if !Path::new(received_images_dir).exists() {
                    eprintln!("No received images directory found. Please request images first.");
                    continue;
                }

                // get the image path
                let image_path = format!("{}/{}.png", received_images_dir, image_id.trim());

                // check if the image exists
                if !Path::new(&image_path).exists() {
                    eprintln!("Image not found. Please request the image first.");
                    continue;
                }

                // Decrypt the image
                if let Err(e) = middleware::decrypt(image_path.clone()).await {
                    eprintln!("Failed to decrypt image: {}", e);
                    continue;
                }
                if cfg!(target_os = "windows") {
                    Command::new("cmd")
                        .arg("/C")
                        .arg(format!("start {}", decrypted_path))
                        .spawn()
                        .expect("Failed to open image");
                } 
                // MacOS uses 'open'
                else if cfg!(target_os = "macos") {
                    Command::new("open")
                        .arg(decrypted_path)
                        .spawn()
                        .expect("Failed to open image");
                } 
                // Linux typically uses 'xdg-open'
                else if cfg!(target_os = "linux") {
                    Command::new("xdg-open")
                        .arg(decrypted_path)
                        .spawn()
                        .expect("Failed to open image");
                }                
                 else {
                    eprintln!("Unsupported platform for image viewer");
                }

                // Check if the views file exists if it doesn't, Raise an error
                if !Path::new(&file_path).exists() {
                    eprintln!("Views count file not found. Please request the image first.");
                    continue;
                }
               

                // Decrement the views count

                let views: i32 = views.trim().parse().expect("Failed to parse views count");
                let new_views = views - 1;

                // Write the new views count
                fs::write(&file_path, new_views.to_string()).expect("Failed to write new views count");

                // delete temporary decrypted image
                // fs::remove_file(decrypted_path).expect("Failed to delete decrypted image");
            }
        }

        else if (input.trim().eq_ignore_ascii_case("c") || input.trim().eq_ignore_ascii_case("C")) {
            // Request for control access
            println!("Enter the client ID, image ID (in two parts), and new view count (format: client_id_image_id_part1_image_id_part2_new_views), e.g., 5_6_23_2:");
            let mut access_input = String::new();
            io::stdin()
                .read_line(&mut access_input)
                .expect("Failed to read access input");
        
            // Clean up the input (remove any leading/trailing whitespace)
            let access_input = access_input.trim();
        
            // Send the formatted message to the server
            let message = format!("Access_Control:{}", access_input);
            socket.send_to(message.as_bytes(), assistant).await?;
            println!("Sent access control request: {}", access_input);
        }

            else {
                println!("Invalid input. Please try again.");
            }
        }
    }
    Ok(())
}