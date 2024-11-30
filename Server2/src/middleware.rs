use crate::bully_election::server_election;
use csv::Writer;
use csv::WriterBuilder;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs;
use std::fs::OpenOptions;
use std::io::{Cursor, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;
use steganography::encoder::*;
use steganography::util::file_as_dynamic_image;
use tokio::fs::File;
use std::net::SocketAddr;
use tokio::io;
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tokio::time::{sleep, timeout, Duration};
#[derive(Serialize, Deserialize, Debug)]
struct OnlineStatus {
    ip : String,
    status: bool,
    client_id: String,
}

pub async fn receive_samples(
    socket: &Arc<tokio::sync::Mutex<UdpSocket>>,
    client_id: &str,
    client_address: &SocketAddr,
    peers: &[String], // List of peer addresses
) -> io::Result<()> {
    println!("Waiting for samples from client: {}", client_id);

    let samples_dir = format!("samples/{}", client_id);
    std::fs::create_dir_all(&samples_dir).expect("Failed to create client-specific 'samples/' directory");

    let mut buffer = [0u8; 4096]; // Buffer for receiving data
    let timeout_duration = Duration::from_secs(30); // Timeout duration
    let mut received_files = Vec::new(); // Track received files for syncing

    loop {
        let recv_result = timeout(timeout_duration, socket.lock().await.recv_from(&mut buffer)).await;

        match recv_result {
            Ok(Ok((size, addr))) => {
                if addr != *client_address {
                    println!("Unexpected message from {}, ignoring.", addr);
                    continue;
                }

                if size == 0 {
                    continue;
                }

                let message = String::from_utf8_lossy(&buffer[..size]);

                if message.starts_with("SAMPLE_UPLOAD:") {
                    // Parse metadata
                    let parts: Vec<&str> = message.split(':').collect();
                    if parts.len() < 3 {
                        eprintln!("Invalid SAMPLE_UPLOAD message: {}", message);
                        continue;
                    }
                    let image_id = parts[2]; // Use the provided image ID

                    // Await the image data
                    let mut buffer = [0u8; 4096];
                    let recv_image_result = timeout(timeout_duration, socket.lock().await.recv_from(&mut buffer)).await;

                    match recv_image_result {
                        Ok(Ok((image_size, image_addr))) => {
                            if image_addr != *client_address {
                                println!("Unexpected image data from {}, ignoring.", image_addr);
                                continue;
                            }

                            // Save the image
                            let image_path = format!("{}/{}.jpg", samples_dir, image_id); // Use image_id without extension
                            std::fs::write(&image_path, &buffer[..image_size])?;
                            println!("Saved sample image: {}", image_path);

                            received_files.push((image_id.to_string(), buffer[..image_size].to_vec()));

                            // Send acknowledgment
                            // socket.lock().await.send_to(b"ACK", client_address).await?;
                            println!("ACK sent for sample: {}", image_id);
                        }
                        Ok(Err(e)) => {
                            eprintln!("Failed to receive image data: {:?}", e);
                            continue;
                        }
                        Err(_) => {
                            eprintln!("Timeout while waiting for image data from client: {}", client_id);
                            break;
                        }
                    }
                } else if message == "END_SAMPLES" {
                    println!("All samples received from client: {}", client_id);

                    // Distribute samples to peers
                    distribute_samples_to_peers(socket, peers, client_id, &received_files).await?;
                    break;
                } else {
                    println!("Unknown message: {}", message);
                }
            }
            Ok(Err(e)) => {
                eprintln!("Failed to receive data: {:?}", e);
                continue;
            }
            Err(_) => {
                eprintln!("Timeout while waiting for samples from client: {}", client_id);
                break;
            }
        }
    }

    println!("Stopped waiting for samples from client: {}", client_id);
    Ok(())
}


// Function to distribute samples to peers
async fn distribute_samples_to_peers(
    socket: &Arc<tokio::sync::Mutex<UdpSocket>>,
    peers: &[String],
    client_id: &str,
    received_files: &[(String, Vec<u8>)], // File names (image_id) and their data
) -> io::Result<()> {
    for (image_id, image_data) in received_files {
        for peer in peers {
            let metadata_message = format!("SAMPLE_SYNC:{}:{}", client_id, image_id);
            socket.lock().await.send_to(metadata_message.as_bytes(), peer).await?;
            println!("Sent SAMPLE_SYNC metadata to {}", peer);

            socket.lock().await.send_to(image_data, peer).await?;
            println!("Sent file {} to {}", image_id, peer);
        }
    }

    Ok(())
}


pub async fn middleware() -> io::Result<()> {
    let my_address = "127.0.0.1";
    let client1 = "127.0.0.1";
    let client2 = "127.0.0.1";
    
    let mysocket = format!("{}:8081", my_address);
    let peers = vec!["127.0.0.1:8083", "127.0.0.1:2010"];

    
    let client_address1 = format!("{}:2005", client1);
    let client_address2 = format!("{}:7001", client2);
    
    let socket_client = Arc::new(tokio::sync::Mutex::new(
        UdpSocket::bind(mysocket.clone()).await?,
    ));
    
    let socket_election = Arc::new(tokio::sync::Mutex::new(
        UdpSocket::bind(format!("{}:8084", my_address)).await?,
    ));
    
    let socketsendipback = Arc::new(tokio::sync::Mutex::new(
        UdpSocket::bind(format!("{}:8085", my_address)).await?,
    ));
    
    let socket6 = Arc::new(tokio::sync::Mutex::new(
        UdpSocket::bind(format!("{}:2003", my_address)).await?,
    ));
    
    let failure_socket = Arc::new(tokio::sync::Mutex::new(
        UdpSocket::bind(format!("{}:9001", my_address)).await?,
    ));
    
    let fail_flag = Arc::new(Mutex::new(false));
    let fail_flag_clone = Arc::clone(&fail_flag);
    

    let mut leader = false;
    let fail_flag_clone_for_failure = Arc::clone(&fail_flag);

    let socket_clone = Arc::clone(&socket_client);
    let socket_clone = Arc::clone(&socket_client);
    
    tokio::spawn(async move {
        loop {
            let mut buffer = [0u8; 2048];
            let (size, addr) = socket_election
                .lock()
                .await
                .recv_from(&mut buffer)
                .await
                .unwrap();
            let message = String::from_utf8_lossy(&buffer[..size]);
            let fail_flag_value = *fail_flag_clone.lock().unwrap();
        
            if message == "ELECT" && !fail_flag_value {
                println!(
                    "Election request received from {}. Initiating election...",
                    addr
                );

                match server_election(&socket_election, peers.clone()).await {
                    Ok(is_leader) => {
                        if is_leader {
                            println!("This server is the leader.");
                            leader = true;

                            let message_to_client = format!("LEADER_ACK:{}", mysocket);
                            if addr.to_string() == client_address1{
                                println!("Client 1 wesel");
                                socketsendipback
                                    .lock()
                                    .await
                                    .send_to(message_to_client.as_bytes(), "127.0.0.1:9080")
                                    .await
                                    .unwrap();
                                } else if addr.to_string() == client_address2{
                                    println!("Client 2 wesel");
                                    socketsendipback
                                    .lock()
                                    .await
                                    .send_to(message_to_client.as_bytes(), "127.0.0.1:7005")
                                    .await
                                    .unwrap();
                                }
                        } else {
                            println!("This server is not the leader.");
                            leader = false;
                        }
                    }
                    Err(e) => eprintln!("Election failed: {:?}", e),
                }
            } else if message.starts_with("DIR_OF_SERV") {
                // Extract the JSON payload from the message
                if let Some(json_payload) = message.strip_prefix("DIR_OF_SERV:") {
                    match serde_json::from_str::<OnlineStatus>(json_payload) {
                        Ok(online_status) => {
                            println!("Received OnlineStatus from peer: {:?}", online_status);
            
                            let file_path = "directory_of_service.csv";
                            let mut is_duplicate = false;
                            let mut needs_update = false;
                            let mut updated_records = Vec::new();
            
                            // Check if the file exists and process existing records
                            if Path::new(file_path).exists() {
                                if let Ok(content) = fs::read_to_string(file_path) {
                                    for line in content.lines() {
                                        // Skip the header
                                        if line.starts_with("uid,client_id,status") {
                                            updated_records.push(line.to_string());
                                            continue;
                                        }
            
                                        let mut fields: Vec<&str> = line.split(',').collect();
                                        if fields.len() == 3 {
                                            if fields[0] == online_status.ip {
                                                is_duplicate = true;
            
                                                // Check if the status has changed
                                                let existing_status = fields[2] == "true";
                                                if existing_status != online_status.status {
                                                    fields[2] = if online_status.status {
                                                        "true"
                                                    } else {
                                                        "false"
                                                    };
                                                    needs_update = true;
                                                    println!(
                                                        "Updating record for UID/IP: {} from {} to {}",
                                                        online_status.ip,
                                                        existing_status,
                                                        online_status.status
                                                    );
                                                }
                                            }
                                        }
            
                                        // Add either the updated or original record to the list
                                        updated_records.push(fields.join(","));
                                    }
                                }
                            }
            
                            // Write updated data back to the CSV if needed
                            if is_duplicate && needs_update {
                                let mut file = match OpenOptions::new()
                                    .write(true)
                                    .truncate(true)
                                    .open(file_path)
                                {
                                    Ok(file) => file,
                                    Err(e) => {
                                        eprintln!("Failed to open file for updating: {}", e);
                                        return;
                                    }
                                };
            
                                for record in &updated_records {
                                    if let Err(e) = writeln!(file, "{}", record) {
                                        eprintln!("Failed to write record: {}", e);
                                    }
                                }
                                println!("CSV file updated successfully.");
                            } else if !is_duplicate {
                                // Append the new record if it's not a duplicate
                                let record = format!(
                                    "{},{},{}\n",
                                    online_status.ip,
                                    online_status.client_id,
                                    online_status.status
                                );
            
                                let file_exists = Path::new(file_path).exists();
                                let mut wtr = match OpenOptions::new()
                                    .create(true)
                                    .append(true)
                                    .open(file_path)
                                {
                                    Ok(file) => file,
                                    Err(e) => {
                                        eprintln!("Failed to open file: {}", e);
                                        return;
                                    }
                                };
            
                                if !file_exists {
                                    if let Err(e) = wtr.write_all(b"uid,client_id,status\n") {
                                        eprintln!("Failed to write header to file: {}", e);
                                        return;
                                    }
                                }
            
                                if let Err(e) = wtr.write_all(record.as_bytes()) {
                                    eprintln!("Failed to write record to file: {}", e);
                                } else {
                                    println!("New record appended to CSV: {:?}", online_status);
                                }
                            } else {
                                println!(
                                    "Duplicate record found for UID/IP: {}. No changes needed.",
                                    online_status.ip
                                );
                            }
            
                        }
                        Err(e) => eprintln!("Failed to parse OnlineStatus: {}", e),
                    }
                } else {
                    eprintln!("Malformed DIR_OF_SERV message: {}", message);
                }
            }
             else if message.starts_with("STATUS:") {
                if let Some(json_payload) = message.strip_prefix("STATUS:") {
                    match serde_json::from_str::<OnlineStatus>(json_payload) {
                        Ok(online_status) => {
                            println!("Received OnlineStatus: {:?}", online_status);
            
                            let file_path = "directory_of_service.csv";
                            let mut is_duplicate = false;
                            let mut needs_update = false;
                            let mut updated_records = Vec::new();
            
                            // Check if the file exists and process existing records
                            if Path::new(file_path).exists() {
                                if let Ok(content) = fs::read_to_string(file_path) {
                                    for line in content.lines() {
                                        // Skip the header
                                        if line.starts_with("uid,client_id,status") {
                                            updated_records.push(line.to_string());
                                            continue;
                                        }
            
                                        let mut fields: Vec<&str> = line.split(',').collect();
                                        if fields.len() == 3 {
                                            if fields[0] == online_status.ip {
                                                is_duplicate = true;
            
                                                // Check if the status has changed
                                                let existing_status = fields[2] == "true";
                                                if existing_status != online_status.status {
                                                    fields[2] = if online_status.status {
                                                        "true"
                                                    } else {
                                                        "false"
                                                    };
                                                    needs_update = true;
                                                    println!(
                                                        "Updating record for UID/IP: {} from {} to {}",
                                                        online_status.ip,
                                                        existing_status,
                                                        online_status.status
                                                    );
                                                }
                                            }
                                        }
            
                                        // Add either the updated or original record to the list
                                        updated_records.push(fields.join(","));
                                    }
                                }
                            }
            
                            // Write updated data back to the CSV if needed
                            if is_duplicate && needs_update {
                                let mut file = match OpenOptions::new()
                                    .write(true)
                                    .truncate(true)
                                    .open(file_path)
                                {
                                    Ok(file) => file,
                                    Err(e) => {
                                        eprintln!("Failed to open file for updating: {}", e);
                                        return;
                                    }
                                };
            
                                for record in &updated_records {
                                    if let Err(e) = writeln!(file, "{}", record) {
                                        eprintln!("Failed to write record: {}", e);
                                    }
                                }
                                println!("CSV file updated successfully.");
                            } else if !is_duplicate {
                                // Append the new record if it's not a duplicate
                                let record = format!(
                                    "{},{},{}\n",
                                    online_status.ip,
                                    online_status.client_id,
                                    online_status.status
                                );
            
                                let file_exists = Path::new(file_path).exists();
                                let mut wtr = match OpenOptions::new()
                                    .create(true)
                                    .append(true)
                                    .open(file_path)
                                {
                                    Ok(file) => file,
                                    Err(e) => {
                                        eprintln!("Failed to open file: {}", e);
                                        return;
                                    }
                                };
            
                                if !file_exists {
                                    if let Err(e) = wtr.write_all(b"uid,client_id,status\n") {
                                        eprintln!("Failed to write header to file: {}", e);
                                        return;
                                    }
                                }
            
                                if let Err(e) = wtr.write_all(record.as_bytes()) {
                                    eprintln!("Failed to write record to file: {}", e);
                                } else {
                                    println!("New record appended to CSV: {:?}", online_status);
                                }
                            } else {
                                println!(
                                    "Duplicate record found for UID/IP: {}. No changes needed.",
                                    online_status.ip
                                );
                            }
            
                            // Acknowledge the message
                            let message_to_client = format!("STATUS_ACK:{}", mysocket);
                            socket_election
                                .lock()
                                .await
                                .send_to(message_to_client.as_bytes(), addr)
                                .await
                                .unwrap();
                            println!("Ack sent to {}", addr);
            
                            // Let the homies know
                            if let Ok(online_status_json) = serde_json::to_string(&online_status) {
                                for peer in &peers {
                                    let message_to_send = format!("DIR_OF_SERV:{}", online_status_json);
                                    socket_election
                                        .lock()
                                        .await
                                        .send_to(message_to_send.as_bytes(), peer)
                                        .await
                                        .unwrap();
                                }
                            }

                            else {
                                eprintln!("Failed to serialize OnlineStatus for broadcasting.");
                            }
            
                            // Receive samples from client only if client is online
                            if online_status.status {
                                // Convert peers to Vec<String> for the function call
                                let peers_for_samples: Vec<String> = peers.iter().map(|&peer| peer.to_string()).collect();
                            
                                if let Err(e) = receive_samples(&socket_election, &online_status.client_id, &addr, &peers_for_samples).await {
                                    eprintln!("Failed to receive samples: {:?}", e);
                                }
                            }

                        }
                        Err(e) => eprintln!("Failed to parse OnlineStatus: {}", e),
                    }
                } else {
                    eprintln!("Malformed STATUS message: {}", message);
                }
            }
            else if message.starts_with("Request_DOS") {
                println!("Received DOS message from {}", addr);
            
                // Parse the directory of service.csv file and send the info to the client
                let file = fs::read_to_string("directory_of_service.csv").expect("Unable to read file");
                let dos_message = format!("DOS:{}", file);
                socket_election.lock().await.send_to(dos_message.as_bytes(), addr).await.unwrap();
            
                // Now send all available samples
                let samples_dir = "samples";
                if !Path::new(samples_dir).exists() {
                    // No samples directory exists
                    let no_samples_message = "NO_SAMPLES";
                    socket_election.lock().await.send_to(no_samples_message.as_bytes(), addr).await.unwrap();
                    println!("Notified client that no samples are available.");
                } else {
                    // Iterate through all folders and files in `samples/`
                    for entry in fs::read_dir(samples_dir).expect("Failed to read samples directory") {
                        if let Ok(client_folder) = entry {
                            let client_folder_path = client_folder.path();
                            if client_folder_path.is_dir() {
                                let client_id = client_folder.file_name().to_string_lossy().to_string();
            
                                for sample in fs::read_dir(client_folder_path).expect("Failed to read client folder") {
                                    if let Ok(sample_file) = sample {
                                        let sample_path = sample_file.path();
                                        if sample_path.is_file() {
                                            let sample_name = sample_file.file_name().to_string_lossy().to_string();
            
                                            // Send metadata first
                                            let metadata_message = format!("SAMPLE:{}:{}", client_id, sample_name);
                                            socket_election.lock().await.send_to(metadata_message.as_bytes(), addr).await.unwrap();
                                            println!("Sent metadata for sample {}:{}", client_id, sample_name);
            
                                            // Send the file contents
                                            let sample_data = fs::read(&sample_path).expect("Failed to read sample file");
                                            socket_election.lock().await.send_to(&sample_data, addr).await.unwrap();
                                            println!("Sent file {} to {}", sample_name, addr);
                                        }
                                    }
                                }
                            }
                        }
                    }
            
                    // Notify the client that all samples are sent
                    let done_message = "SAMPLES_DONE";
                    socket_election.lock().await.send_to(done_message.as_bytes(), addr).await.unwrap();
                    println!("Notified client that all samples are sent.");
                }
            }

            else if message.starts_with("SAMPLE_SYNC:") {
                if let Some(metadata) = message.strip_prefix("SAMPLE_SYNC:") {
                    let parts: Vec<&str> = metadata.split(':').collect();
                    if parts.len() < 2 {
                        eprintln!("Invalid SAMPLE_SYNC message: {}", metadata);
                        continue;
                    }
            
                    let client_id = parts[0];
                    let image_name = parts[1];
            
                    // Create the folder for this client
                    let client_samples_dir = format!("samples/{}", client_id);
                    std::fs::create_dir_all(&client_samples_dir).expect("Failed to create client samples directory");
            
                    // Receive the image data
                    let mut buffer = [0u8; 4096];
                    let (size, addr) = match socket_election.lock().await.recv_from(&mut buffer).await {
                        Ok(result) => result,
                        Err(e) => {
                            eprintln!("Failed to receive image data: {:?}", e);
                            continue;
                        }
                    };
                    println!("Received image data from {}", addr);
            
                    let image_path = format!("{}/{}.jpg", client_samples_dir, image_name);
                    if let Err(e) = std::fs::write(&image_path, &buffer[..size]) {
                        eprintln!("Failed to write image data: {:?}", e);
                        continue;
                    }
                    println!("Stored image: {}", image_path);
                }
            }
            
                      
        }
    });

    let failure_socket_clone = Arc::clone(&failure_socket);
    tokio::spawn(async move {
        let mut buffer = [0u8; 2048];
        loop {
            let (size, addr) = failure_socket_clone
                .lock()
                .await
                .recv_from(&mut buffer)
                .await
                .expect("Failed to receive message");

            let message = String::from_utf8_lossy(&buffer[..size]);
            println!("Message received from {}: {}", addr, message);

            if message.trim() == "FAIL" {
                *fail_flag_clone_for_failure.lock().unwrap() = true;
                println!("This server is down!");
                thread::sleep(Duration::from_secs(30));
                println!("Server restored!");
                *fail_flag_clone_for_failure.lock().unwrap() = false;
            }
        }
    });

    let (tx, mut rx): (
        mpsc::Sender<(Vec<u8>, std::net::SocketAddr)>,
        mpsc::Receiver<(Vec<u8>, std::net::SocketAddr)>,
    ) = mpsc::channel(32);
    let socket_clone_client = Arc::clone(&socket_client);

    tokio::spawn(async move {
        let mut buffer = [0u8; 2048];
        let mut image_data = Vec::new();
        let mut received_chunks = 0;
        let mut expected_sequence_num = 0;
        println!("Server listening on {}", my_address);

        loop {
            let (size, addr) = socket_clone_client
                .lock()
                .await
                .recv_from(&mut buffer)
                .await
                .unwrap();
            if size < 4 {
                continue;
            }

            if &buffer[4..size] == b"END" {
                let sequence_num = u32::from_be_bytes(buffer[0..4].try_into().unwrap());

                if sequence_num == expected_sequence_num {
                    let mut file = match std::fs::File::create("received_image.jpg") {
                        Ok(f) => f,
                        Err(e) => {
                            eprintln!("Failed to create file: {:?}", e);
                            continue;
                        }
                    };
                    if let Err(e) = file.write_all(&image_data) {
                        eprintln!("Failed to write to file: {:?}", e);
                        continue;
                    }
                    println!("Image saved successfully!");

                    tx.send((image_data.clone(), addr)).await.unwrap();
                    image_data.clear();
                    received_chunks = 0;
                    expected_sequence_num = 0;
                    socket_clone_client
                        .lock()
                        .await
                        .send_to(b"END", addr)
                        .await
                        .unwrap();
                    println!("Final ACK sent.");
                } else {
                    socket_clone_client
                        .lock()
                        .await
                        .send_to(format!("NACK {}", expected_sequence_num).as_bytes(), addr)
                        .await
                        .unwrap();
                    println!("NACK sent for sequence number {}", expected_sequence_num);
                }
                continue;
            }
            let sequence_num = u32::from_be_bytes(buffer[0..4].try_into().unwrap());

            if sequence_num == expected_sequence_num {
                image_data.extend_from_slice(&buffer[4..size]);
                received_chunks += 1;
                expected_sequence_num += 1;
            } else {
                socket_clone_client
                    .lock()
                    .await
                    .send_to(format!("NACK {}", expected_sequence_num).as_bytes(), addr)
                    .await
                    .unwrap();
                println!("NACK sent for sequence number {}", expected_sequence_num);

                continue;
                
            }

            if received_chunks % 10 == 0 {
                socket_clone_client
                    .lock()
                    .await
                    .send_to(
                        format!("ACK {}", expected_sequence_num - 1).as_bytes(),
                        addr,
                    )
                    .await
                    .unwrap();
                println!("ACK {} sent.", expected_sequence_num - 1);
            }
        }
    });

    tokio::spawn(async move {
        while let Some((image_data, client_addr)) = rx.recv().await {
            // Guess the format and load the image for encryption
            let format = image::guess_format(&image_data).expect("Failed to guess image format");
            let original_img =
                image::load(Cursor::new(image_data.clone()), format).expect("Failed to load image");

            let default_img_path = Path::new("images/mask.jpg");
            let default_img = image::open(default_img_path).expect("Failed to open default image");

            // Encrypt the image using the steganography crate
            let start = Instant::now();
            let mask = file_as_dynamic_image(default_img_path.to_str().unwrap().to_string());

            let encoder = Encoder::new(&image_data, mask);
            let encrypted_img = encoder.encode_alpha();
            let duration = start.elapsed();
            println!("Encryption Time: {:?}", duration);

            let filename = "server2_encryption_times.csv";
            let file_exists = std::path::Path::new(filename).exists();
            match OpenOptions::new().append(true).create(true).open(filename) {
                Ok(mut file) => {
                    if !file_exists {
                        if let Err(e) = writeln!(file, "encryption_time") {
                            eprintln!("Failed to write header: {}", e);
                            return;
                        }
                    }

                    let mut wtr = Writer::from_writer(file);
                    let duration_in_seconds = format!("{:.2}", duration.as_secs_f64());

                    if let Err(e) = wtr.write_record(&[duration_in_seconds.to_string()]) {
                        eprintln!("Failed to write record: {}", e);
                        return;
                    }

                    if let Err(e) = wtr.flush() {
                        eprintln!("Failed to flush writer: {}", e);
                        return;
                    }
                }
                Err(e) => {
                    eprintln!("Failed to open file: {}", e);
                }
            }

            // Save the encrypted image as PNG to avoid lossy compression
            let encrypted_image_path = "encrypted-image.png";
            encrypted_img
                .save(encrypted_image_path)
                .expect("Failed to save encoded image as PNG");
            println!("Server: Encrypted image saved as {}", encrypted_image_path);

            // Read the PNG image bytes
            let encrypted_data =
                std::fs::read(encrypted_image_path).expect("Failed to read encrypted PNG image");

            // Send encrypted data in chunks
            let chunk_size = 2044;
            let total_chunks = (encrypted_data.len() as f64 / chunk_size as f64).ceil() as usize;
            let mut sequence_num: u32 = 0;
            let mut ack_buffer = [0u8; 1024];
            let max_retries = 5;

            let mut client = String::new();
            
            if client_addr.to_string() == format!("{}:9080",client1){
                client = format!("{}:2005",client1);
            } else if client_addr.to_string()==format!("{}:7005",client2){
                client = format!("{}:7001", client2);
            }

            for i in 0..total_chunks {
                let start = i * chunk_size;
                let end = std::cmp::min(start + chunk_size, encrypted_data.len());
                let chunk_data = &encrypted_data[start..end];

                let mut chunk = Vec::with_capacity(4 + chunk_data.len());
                chunk.extend_from_slice(&sequence_num.to_be_bytes());
                chunk.extend_from_slice(chunk_data);

                socket6
                    .lock()
                    .await
                    .send_to(&chunk, &client)
                    .await
                    .expect("Failed to send encrypted image chunk");
                println!("Sent encrypted chunk {} of {}", i + 1, total_chunks);

                let mut retries = 0;
                loop {
                    match timeout(
                        Duration::from_secs(5),
                        socket6.lock().await.recv_from(&mut ack_buffer),
                    )
                    .await
                    {
                        Ok(Ok((ack_size, _))) => {
                            let ack_message = String::from_utf8_lossy(&ack_buffer[..ack_size]);
                            if ack_message.starts_with("ACK") {
                                let ack_num: u32 = ack_message[4..].parse().unwrap();
                                if ack_num == sequence_num {
                                    println!("ACK received for sequence number {}", ack_num);
                                    sequence_num += 1;
                                    break;
                                }
                            } else if ack_message.starts_with("NACK") {
                                println!("NACK received for sequence number {}", sequence_num);
                            }
                        }
                        Ok(Err(e)) => {
                            eprintln!("Failed to receive ACK/NACK: {:?}", e);
                            retries += 1;
                            if retries >= max_retries {
                                eprintln!(
                                    "Max retries reached for chunk {}. Aborting.",
                                    sequence_num
                                );
                                break;
                            }
                            println!("Retrying chunk {} after recv_from error", sequence_num);
                            socket6
                                .lock()
                                .await
                                .send_to(&chunk, &client)
                                .await
                                .expect("Failed to resend chunk");
                        }
                        Err(_) => {
                            retries += 1;
                            if retries >= max_retries {
                                eprintln!(
                                    "Max retries reached for chunk {}. Aborting.",
                                    sequence_num
                                );
                                break;
                            }
                            println!("Timeout waiting for ACK, resending chunk {}", sequence_num);
                            socket6
                                .lock()
                                .await
                                .send_to(&chunk, &client)
                                .await
                                .expect("Failed to resend chunk");
                        }
                    }
                }
            }

            // Send "END" message after the last chunk
            socket6
                .lock()
                .await
                .send_to(b"END", &client)
                .await
                .expect("Failed to send END message");
            println!("Encrypted image transmission completed.");
        }
    });

    loop {
        sleep(Duration::from_secs(10)).await;
    }
}
