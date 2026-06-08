# Peer to Peer Image Sharing System

## System Overview
A **Cloud Peer-to-Peer (P2P) system** for controlled image sharing with dual-layer encryption:
- **Cloud servers** handle image encryption using steganography
- **Directory of services** connects clients for direct P2P communication
- **Access control** with embedded view counts and recipient IDs

## Technical Stack
- **Language**: Rust (rustc 1.82.0)
- **Async Runtime**: Tokio 1.40.0 with full features
- **Encryption**: Steganography crate (1.0.2) + custom metadata embedding
- **Communication**: UDP sockets for all client-server and P2P interactions
- **Image Processing**: Image crate (0.24.6)

## Key Features
1. **Server-to-Server Communication** with synchronized directory of services
2. **Load Balancing** using Modified Bully Algorithm (CPU utilization-based)
3. **Dual Encryption**: Server-side steganography + client-side access rights embedding
4. **Fault Tolerance** with automatic leader re-election and failure simulation
5. **Offline Operations** with push notifications and history table
6. **Chunked Transmission** with ACK/NACK mechanism for reliability

## Concurrency Architecture
**Server Side (4 threads):**
- Message transmission thread (main communication handler)
- Failure simulation thread (30-second random shutdowns)
- Image receiving thread (chunked data assembly)
- Image sending thread (post-encryption transmission)

**Client Side:**
- P2P listener thread for direct peer communication

## Performance Results

### Load Balancing Performance (1000 images, 7 hours)
| Metric | Server1 | Server2 | Server3 | Total |
|--------|---------|---------|---------|-------|
| Total Requests | 241 | 328 | 431 | 1000 |
| Success Rate | 100% | 100% | 100% | 100% |
| Avg Turnaround Time | 15.96s | 15.87s | 16.01s | 15.95s |
| Avg Encryption Time | 1.99s | 1.98s | 1.97s | 1.98s |

### With Failure Simulation (1000 images, 9 hours)
| Metric | Result |
|--------|--------|
| Success Rate | 74.3% (743/1000) |
| Retransmission Rate | 25.68% |
| Avg Turnaround Time | 26.56s |
| Avg Encryption Time | 1.87s |

## Technical Highlights

### Load Balancing Algorithm
- **Modified Bully Algorithm** with CPU utilization comparison
- Dynamic leader election based on lowest CPU usage
- Handles 1, 2, or 3 server scenarios with automatic failover

### Encryption Implementation
- **Steganography**: Server-side image hiding using mask image
- **Metadata Embedding**: Additional row added to image containing:
  - Access rights (view count)
  - Recipient ID
  - Double decryption process on recipient side

#### Steganography result can be found below:
Image Before Encryption:
![2](https://github.com/user-attachments/assets/87b9919d-131b-4dba-a470-75aa0df20d0a)
Image After Encryption:
![22](https://github.com/user-attachments/assets/e56b4c71-5540-4a2e-987a-9f0277b4fe1b)

**We use the second image as the mask image**

### Reliability Features
- **Chunked Transmission**: Large images split with sequence numbers
- **ACK/NACK Protocol**: Automatic retransmission for failed/out-of-order chunks
- **Timeout Handling**: 3-timeout tolerance before transmission abandonment
- **History Table**: Automatic retry for failed requests when clients reconnect

### Transparency Implementation
- **Access Transparency**: Clients unaware of resource locations
- **Location Transparency**: Random server selection abstracted from clients
- **Failure Transparency**: Automatic leader re-election without client awareness
- **Performance Transparency**: Dynamic load balancing maintains consistent performance

## Architecture Benefits
- **Scalability**: Decentralized P2P communication reduces server load
- **Security**: Dual-layer encryption with controlled access rights
- **Fault Tolerance**: Survives server failures with 25% retransmission rate
- **Efficiency**: Direct peer communication minimizes server dependency
