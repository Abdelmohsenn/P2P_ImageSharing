use std::env;
use std::io;
use std::io::Write;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use std::path::Path;
use image::{RgbaImage};
use std::time::Instant;
use std::io::Cursor;
use std::net::Ipv4Addr;


mod encryption;
mod bully_election;
use bully_election::server_election;


#[tokio::main]
async fn main() -> io::Result<()> {
    let my_address = "127.0.0.1:8082"; // da address el Server
    let peer_address = "127.0.0.1:8084"; // da address el peer for election
    let client_address = "127.0.0.1:8080"; // da for sending back my socket
    let socket_client = Arc::new(tokio::sync::Mutex::new(UdpSocket::bind(my_address).await?));

    let socket_election = Arc::new(tokio::sync::Mutex::new(UdpSocket::bind("127.0.0.1:8083").await?)); // server-server socket
    let socketsendipback = Arc::new(tokio::sync::Mutex::new(UdpSocket::bind("127.0.0.1:8086").await?)); // my socket for sending back my socket

    let mut leader = false;
    
    // Set up a task to listen for incoming messages (including "ELECT")
    let socket_clone = Arc::clone(&socket_client);
    tokio::spawn(async move {
    loop {
        let mut buffer = [0u8; 2048];
        let (size, addr) = socket_election.lock().await.recv_from(&mut buffer).await.unwrap();
        let message = String::from_utf8_lossy(&buffer[..size]);

        if message == "ELECT" {
            println!("Election request received from {}. Initiating election...", addr);

            match server_election(&socket_election, peer_address).await {
                Ok(is_leader) => {
                    if is_leader {
                        println!("This server is the leader.");
                        leader = true;

                        // Send "Leader_Ack" with the server address
                        let message_to_client = format!("LEADER_ACK:{}", my_address);
                        socketsendipback.lock().await.send_to(message_to_client.as_bytes(), client_address).await.unwrap();
                    } else {
                        println!("This server is not the leader.");
                        leader = false;
                    }
                }
                Err(e) => eprintln!("Election failed: {:?}", e),
            }
        }
    }
});

    
    
   let (tx, mut rx): (mpsc::Sender<(Vec<u8>, std::net::SocketAddr)>, mpsc::Receiver<(Vec<u8>, std::net::SocketAddr)>) = mpsc::channel(32);// sender/receiving vars
   let socket_clone_client = Arc::clone(&socket_client);

   tokio::spawn(async move {
   let mut buffer = [0u8; 2048];
   let mut image_data = Vec::new();
   let mut received_chunks = 0;
   let mut expected_sequence_num = 0;
   println!("Server listening on {}", my_address);
   

   loop {
    let (size, addr) = socket_clone_client.lock().await.recv_from(&mut buffer).await.unwrap();
   if size < 4 {
    continue;
   }

   if &buffer[4..size] == b"END" {
       let sequence_num = u32::from_be_bytes(buffer[0..4].try_into().unwrap());


       if sequence_num == expected_sequence_num {
               let mut file = match std::fs::File::create("received_image.png") {
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

           tx.send((image_data.clone(), addr)).await.unwrap(); // Send to encryption thread
           image_data.clear();
           received_chunks = 0;
           expected_sequence_num = 0;
           socket_clone_client.lock().await.send_to(b"END", addr).await.unwrap();
           println!("Final ACK sent.");
       }
       else {
           socket_clone_client.lock().await.send_to(format!("NACK {}", expected_sequence_num).as_bytes(), addr).await.unwrap();
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
           socket_clone_client.lock().await.send_to(format!("NACK {}", expected_sequence_num).as_bytes(), addr).await.unwrap();
           println!("NACK sent for sequence number {}", expected_sequence_num);
           continue;
       }


if received_chunks % 10 == 0 {
    socket_clone_client.lock().await.send_to(format!("ACK {}", expected_sequence_num - 1).as_bytes(), addr).await.unwrap();
    println!("ACK {} sent.", expected_sequence_num - 1);
}
}
});

tokio::spawn(async move {
    while let Some((image_data, _addr)) = rx.recv().await {
        // Guess the format from the raw image data slice
        let format = image::guess_format(&image_data).expect("Failed to guess image format");
        let original_img = image::load(Cursor::new(image_data.clone()), format).expect("Failed to load image");

        let default_img_path = Path::new("images/sunflower-0quality.jpg");
        let default_img = image::open(default_img_path).expect("Failed to open default image");

        let start = Instant::now();
        let encrypted_img: RgbaImage = encryption::encrypt(default_img, original_img.clone());
        println!("Encryption Time: {:?}", start.elapsed());

        let _ = encrypted_img.save("images/encrypted-image.jpg").expect("Failed to save encrypted image");
    }
}).await.expect("Task failed");

Ok(())

}

    