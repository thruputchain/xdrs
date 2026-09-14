//! TCP connection handling for RPC record streams.

use std::net::SocketAddr;
use std::time::Duration;

use socket2::{SockRef, TcpKeepalive};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, ToSocketAddrs};

/// Write half of a TCP connection.
#[derive(Debug)]
pub struct WriteHalf {
    write_half: tokio::net::tcp::OwnedWriteHalf,
}

/// Read half of a TCP connection with record framing.
///
/// Uses an internal read buffer to amortize syscalls: a single `read()`
/// may return multiple records, which are then parsed without further
/// syscalls until the buffer is depleted.
#[derive(Debug)]
pub struct ReadHalf {
    read_half: tokio::net::tcp::OwnedReadHalf,
    /// Read buffer. Data lives in `buf[pos..len]`.
    buf: Vec<u8>,
    /// Start of unconsumed data.
    pos: usize,
    /// End of valid data.
    len: usize,
    /// Scratch space for records that span two reads.
    record: Vec<u8>,
    max_message_size: u32,
}

fn configure_stream(stream: &TcpStream) -> std::io::Result<()> {
    configure_stream_with_keepalive(stream, |stream| {
        // Send probes after 60s idle to preserve connection tracking through
        // middleboxes with longer idle timeouts. This is not an RPC deadline.
        SockRef::from(stream)
            .set_tcp_keepalive(&TcpKeepalive::new().with_time(Duration::from_secs(60)))
    })
}

fn configure_stream_with_keepalive(
    stream: &TcpStream,
    configure_keepalive: impl FnOnce(&TcpStream) -> std::io::Result<()>,
) -> std::io::Result<()> {
    // RPC latency is more important than Nagle aggregation; batching happens
    // explicitly in the connection writer.
    stream.set_nodelay(true)?;
    if let Err(error) = configure_keepalive(stream) {
        tracing::warn!(%error, "could not fully configure TCP keepalive; continuing with connection");
    }
    Ok(())
}

pub async fn connect<A: ToSocketAddrs>(
    addr: A,
    max_message_size: u32,
    read_buffer_size: usize,
) -> Result<(ReadHalf, WriteHalf), std::io::Error> {
    let stream = TcpStream::connect(addr).await?;
    configure_stream(&stream)?;
    let (read_half, write_half) = stream.into_split();
    Ok((
        ReadHalf::new(read_half, max_message_size, read_buffer_size),
        WriteHalf { write_half },
    ))
}

pub async fn accept(
    listener: &TcpListener,
    max_message_size: u32,
    read_buffer_size: usize,
) -> Result<(ReadHalf, WriteHalf, SocketAddr), std::io::Error> {
    let (stream, addr) = listener.accept().await?;
    configure_stream(&stream)?;
    let (read_half, write_half) = stream.into_split();
    Ok((
        ReadHalf::new(read_half, max_message_size, read_buffer_size),
        WriteHalf { write_half },
        addr,
    ))
}

impl WriteHalf {
    pub async fn write_data(&mut self, data: &[u8]) -> Result<(), std::io::Error> {
        self.write_half.write_all(data).await
    }

    pub async fn write_vectored<'a>(
        &mut self,
        bufs: &'a [std::io::IoSlice<'a>],
    ) -> Result<usize, std::io::Error> {
        // Try non-blocking first to skip the async poll machinery when the
        // socket is already writable (common for small batches on loopback).
        match self.write_half.try_write_vectored(bufs) {
            Ok(n) => return Ok(n),
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(e) => return Err(e),
        }
        self.write_half.writable().await?;
        match self.write_half.try_write_vectored(bufs) {
            Ok(n) => Ok(n),
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                // Spurious readiness — fall back to async write.
                self.write_half.write_vectored(bufs).await
            }
            Err(e) => Err(e),
        }
    }
}

impl ReadHalf {
    fn new(
        read_half: tokio::net::tcp::OwnedReadHalf,
        max_message_size: u32,
        read_buffer_size: usize,
    ) -> Self {
        Self {
            read_half,
            buf: vec![0u8; read_buffer_size],
            pos: 0,
            len: 0,
            record: Vec::with_capacity(8192),
            max_message_size,
        }
    }

    /// Number of unconsumed bytes in the read buffer.
    #[inline]
    fn buffered(&self) -> usize {
        self.len - self.pos
    }

    /// Ensure at least `need` bytes are available in `buf[pos..]`.
    /// Compacts the buffer first if necessary to make room, then reads
    /// from the socket until we have enough.
    async fn fill(&mut self, need: usize) -> Result<(), std::io::Error> {
        // Compact: move unconsumed data to the front if we can't fit
        // `need` more bytes at the end of the buffer.
        if self.pos > 0 && self.buf.len() - self.len < need.saturating_sub(self.buffered()) {
            self.buf.copy_within(self.pos..self.len, 0);
            self.len -= self.pos;
            self.pos = 0;
        }

        // Grow if even the full buffer can't hold `need` bytes.
        if need > self.buf.len() {
            self.buf.resize(need, 0);
        }

        while self.buffered() < need {
            let n = self.read_half.read(&mut self.buf[self.len..]).await?;
            if n == 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "connection closed",
                ));
            }
            self.len += n;
        }
        Ok(())
    }

    /// Read a single RPC record from the connection.
    ///
    /// Returns `None` if the record is too large or requests a multi-part message.
    pub async fn read_record(&mut self) -> Result<Option<&[u8]>, std::io::Error> {
        // Read the 4-byte record header.
        self.fill(4).await?;
        let header = u32::from_be_bytes(self.buf[self.pos..self.pos + 4].try_into().unwrap());
        let size = (header & 0x7FFF_FFFF) as usize;
        let last_fragment = header & 0x8000_0000 != 0;

        if !last_fragment {
            tracing::warn!(header, "multi-part messages unsupported");
            return Ok(None);
        }

        if size as u32 > self.max_message_size {
            tracing::warn!(header, "message size too large");
            return Ok(None);
        }

        // Consume header.
        self.pos += 4;

        // Fast path: entire record already in the read buffer.
        if self.buffered() >= size {
            let start = self.pos;
            self.pos += size;
            return Ok(Some(&self.buf[start..start + size]));
        }

        // Slow path: record spans a buffer boundary. Copy what we have
        // into `self.record`, then read the rest directly into it.
        self.record.clear();
        self.record.reserve(size);
        let available = self.buffered();
        self.record.extend_from_slice(&self.buf[self.pos..self.len]);
        self.pos = 0;
        self.len = 0;

        // Extend the length so read_exact can fill the rest.
        self.record.resize(size, 0);
        self.read_half
            .read_exact(&mut self.record[available..])
            .await?;

        Ok(Some(&self.record[..size]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn configure_stream_sets_nodelay_and_keepalive_on_both_ends() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let (client, server) = tokio::join!(
            connect(listener.local_addr().unwrap(), 1024, 65536),
            accept(&listener, 1024, 65536),
        );
        let (client_read, _client_write) = client.unwrap();
        let (server_read, _server_write, _) = server.unwrap();
        for read in [&client_read, &server_read] {
            let stream = read.read_half.as_ref();
            assert!(stream.nodelay().unwrap());
            let socket = SockRef::from(stream);
            assert!(socket.keepalive().unwrap());
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            assert_eq!(
                socket.tcp_keepalive_time().unwrap(),
                Duration::from_secs(60)
            );
        }
    }

    #[tokio::test]
    async fn keepalive_configuration_failure_preserves_stream_io() {
        for kind in [
            std::io::ErrorKind::PermissionDenied,
            std::io::ErrorKind::Unsupported,
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let (client, server) = tokio::join!(
                TcpStream::connect(listener.local_addr().unwrap()),
                listener.accept(),
            );
            let mut client = client.unwrap();
            let (mut server, _) = server.unwrap();
            for stream in [&client, &server] {
                configure_stream_with_keepalive(stream, |_| Err(std::io::Error::from(kind)))
                    .unwrap();
                assert!(stream.nodelay().unwrap());
            }
            tokio::time::timeout(Duration::from_secs(5), async {
                client.write_all(b"request").await.unwrap();
                let mut request = [0; 7];
                server.read_exact(&mut request).await.unwrap();
                assert_eq!(&request, b"request");
                server.write_all(b"reply").await.unwrap();
                let mut reply = [0; 5];
                client.read_exact(&mut reply).await.unwrap();
                assert_eq!(&reply, b"reply");
            })
            .await
            .unwrap();
        }
    }

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
        let (mut read, mut write) = connect(addr, 1024, 65536).await.unwrap();

        write.write_data(b"test message").await.unwrap();
        let mut buf = vec![0u8; (b"test message").len()];

        read.read_half.read_exact(&mut buf).await.unwrap();

        assert_eq!(buf, b"test message");
    }
}
