use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpStream, ToSocketAddrs};

pub struct WriteHalf {
    // We may choose to multiplex this over multiple connections later.
    write_half: tokio::net::tcp::OwnedWriteHalf,
}

pub struct ReadHalf {
    // We may choose to multiplex this over multiple connections later.
    read_half: tokio::net::tcp::OwnedReadHalf,
}

pub async fn connect<A: ToSocketAddrs>(addr: A) -> Result<(ReadHalf, WriteHalf), std::io::Error> {
    let stream = TcpStream::connect(addr).await?;
    let (read_half, write_half) = stream.into_split();
    Ok((ReadHalf { read_half }, WriteHalf { write_half }))
}

impl WriteHalf {
    pub async fn write_data(&mut self, data: &[u8]) -> Result<(), std::io::Error> {
        self.write_half.write_all(data).await
    }
}

impl ReadHalf {
    pub async fn read_u32(&mut self) -> Result<u32, std::io::Error> {
        let mut buffer = [0u8; 4];
        self.read_half.read_exact(&mut buffer).await?;
        Ok(u32::from_be_bytes(buffer))
    }

    pub async fn read_data(&mut self, data: &mut [u8]) -> Result<(), std::io::Error> {
        let n = self.read_half.read_exact(data).await?;
        assert_eq!(n, data.len());
        Ok(())
    }
}

/*
    /// Send a message and register a callback for the response
    pub async fn send_request(&self, data : &[u8]) -> Result<(), TcpConnectionError>
    {
        self.write_half.write_all(data).await
    }

    /// Check if the connection is still active
    pub async fn is_connected(&self) -> bool {
        *self.is_connected.lock().await
    }

    /// Get the current client state (for advanced usage)
    pub fn client_state(&self) -> Arc<Mutex<ClientState>> {
        self.client_state.clone()
    }

    /// Writer task that handles outgoing messages
    async fn writer_task(
        mut write_half: tokio::net::tcp::OwnedWriteHalf,
        mut send_rx: mpsc::UnboundedReceiver<Message>,
        is_connected: Arc<Mutex<bool>>,
    ) {
        while let Some(message) = send_rx.recv().await {
            let bytes = message.to_bytes();
            
            if let Err(e) = write_half.write_all(&bytes).await {
                eprintln!("Failed to write message: {}", e);
                break;
            }
            
            if let Err(e) = write_half.flush().await {
                eprintln!("Failed to flush write buffer: {}", e);
                break;
            }
        }
        
        // Mark connection as closed
        *is_connected.lock().await = false;
    }

    /// Reader task that handles incoming messages
    async fn reader_task(
        mut read_half: tokio::net::tcp::OwnedReadHalf,
        client_state: Arc<Mutex<ClientState>>,
        is_connected: Arc<Mutex<bool>>,
    ) {
        let mut buffer = vec![0u8; 8192];
        let mut message_buffer = Vec::new();
        
        loop {
            match read_half.read(&mut buffer).await {
                Ok(0) => {
                    // Connection closed
                    break;
                }
                Ok(n) => {
                    message_buffer.extend_from_slice(&buffer[0..n]);
                    
                    // Process complete messages
                    while message_buffer.len() >= 8 {
                        let data_len = u32::from_be_bytes([
                            message_buffer[4],
                            message_buffer[5], 
                            message_buffer[6],
                            message_buffer[7]
                        ]) as usize;
                        
                        let total_len = 8 + data_len;
                        if message_buffer.len() >= total_len {
                            // We have a complete message
                            let message_bytes = message_buffer.drain(0..total_len).collect::<Vec<_>>();
                            
                            match Message::from_bytes(&message_bytes) {
                                Ok(message) => {
                                    let mut state = client_state.lock().await;
                                    state.handle_reply(message.xid, message.data);
                                }
                                Err(e) => {
                                    eprintln!("Failed to parse message: {}", e);
                                }
                            }
                        } else {
                            // Wait for more data
                            break;
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Failed to read from connection: {}", e);
                    break;
                }
            }
        }
        
        // Mark connection as closed
        *is_connected.lock().await = false;
    }
} */

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn test_tcp_connection() {
        // Start a simple echo server
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (mut read_half, mut write_half) = stream.into_split();
            
            let mut buffer = vec![0u8; 1024];
            while let Ok(n) = read_half.read(&mut buffer).await {
                if n == 0 { break; }
                let _ = write_half.write_all(&buffer[0..n]).await;
            }
        });

        // Connect and test
        let (mut read, mut write) = connect(addr).await.unwrap();
        
        write.write_data(b"test message").await.unwrap();
        let mut buf = vec![0u8; (b"test message").len()];

        read.read_data(&mut buf).await.unwrap();

        assert_eq!(buf, b"test message");
    }
}
