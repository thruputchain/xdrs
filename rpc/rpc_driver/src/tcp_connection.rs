//! TCP connection handling for RPC record streams.

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, ToSocketAddrs};

/// Write half of a TCP connection.
pub struct WriteHalf {
    write_half: tokio::net::tcp::OwnedWriteHalf,
}

/// Read half of a TCP connection with record framing.
pub struct ReadHalf {
    read_half: tokio::net::tcp::OwnedReadHalf,
    buffer: Vec<u8>,
    max_message_size: u32,
}

pub async fn connect<A: ToSocketAddrs>(
    addr: A,
    max_message_size: u32,
) -> Result<(ReadHalf, WriteHalf), std::io::Error> {
    let stream = TcpStream::connect(addr).await?;
    let (read_half, write_half) = stream.into_split();
    Ok((
        ReadHalf {
            read_half,
            buffer: Vec::with_capacity(8192),
            max_message_size,
        },
        WriteHalf { write_half },
    ))
}

pub async fn accept(
    listener: &TcpListener,
    max_message_size: u32,
) -> Result<(ReadHalf, WriteHalf), std::io::Error> {
    let (stream, _) = listener.accept().await?;
    let (read_half, write_half) = stream.into_split();
    Ok((
        ReadHalf {
            read_half,
            buffer: Vec::with_capacity(8192),
            max_message_size,
        },
        WriteHalf { write_half },
    ))
}

impl WriteHalf {
    pub async fn write_data(&mut self, data: &[u8]) -> Result<(), std::io::Error> {
        self.write_half.write_all(data).await
    }
}

impl ReadHalf {
    async fn read_u32(&mut self) -> Result<u32, std::io::Error> {
        let mut buffer = [0u8; 4];
        self.read_half.read_exact(&mut buffer).await?;
        Ok(u32::from_be_bytes(buffer))
    }

    /// Read a single RPC record from the connection.
    ///
    /// Returns `None` if the record is too large or requests a multi-part message.
    pub async fn read_record(&mut self) -> Result<Option<&[u8]>, std::io::Error> {
        let n = self.read_u32().await?;
        let size = n & 0x7fffffff;
        let highbit = n & 0x80000000;
        // multi part messages unimplemented
        if highbit == 0 {
            eprintln!("Multi-part messages unsupported: n={:X}", n);
            return Ok(None);
        }

        if size > self.max_message_size {
            eprintln!("Message size too large: n={:X}", n);
            return Ok(None);
        }

        // Only resize buffer if current capacity is insufficient
        // This prevents allocation thrashing from alternating message sizes
        if self.buffer.len() < size as usize {
            self.buffer.resize(size as usize, 0);
        }

        let slice = &mut self.buffer[..size as usize];
        self.read_half.read_exact(slice).await?;
        Ok(Some(slice))
    }
}

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
                if n == 0 {
                    break;
                }
                let _ = write_half.write_all(&buffer[0..n]).await;
            }
        });

        // Connect and test
        let (mut read, mut write) = connect(addr, DEFAULT_MAX_MESSAGE_SIZE).await.unwrap();

        write.write_data(b"test message").await.unwrap();
        let mut buf = vec![0u8; (b"test message").len()];

        read.read_half.read_exact(&mut buf).await.unwrap();

        assert_eq!(buf, b"test message");
    }
}
