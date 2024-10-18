use tokio::net::UdpSocket;
use tokio::io::{self};

#[tokio::main]
async fn main() -> io::Result<()> {
    let address = "127.0.0.1";
    let port = "8080";
    let together = format!("{}:{}", address, port);
    let socket = UdpSocket::bind(&together).await?;
    println!("Server listening on {}", together);

    let mut buffer = [0u8; 1024];

    loop { // will always listen
        let (size, addr) = socket.recv_from(&mut buffer).await?;
        let message = String::from_utf8_lossy(&buffer[..size]);
        println!("Received message from {}: {}", addr, message);

        // send the message back to the client to check
        socket.send_to(&buffer[..size], addr).await?;
    }
}
