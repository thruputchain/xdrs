//! TCP connection handling for RPC record streams.

use std::net::SocketAddr;

use bytes::{Buf, Bytes, BytesMut};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, ToSocketAddrs};

/// Write half of a TCP connection.
#[derive(Debug)]
pub struct WriteHalf {
    write_half: tokio::net::tcp::OwnedWriteHalf,
}

/// Read half of a TCP connection with record framing.
///
/// Uses a [`BytesMut`] read buffer to amortize syscalls: a single `read()`
/// may return multiple records, which are then extracted via
/// [`split_to`](BytesMut::split_to) with zero copy.
#[derive(Debug)]
pub struct ReadHalf {
    read_half: tokio::net::tcp::OwnedReadHalf,
    buf: BytesMut,
    max_message_size: u32,
}

pub async fn connect<A: ToSocketAddrs>(
    addr: A,
    max_message_size: u32,
    read_buffer_size: usize,
) -> Result<(ReadHalf, WriteHalf), std::io::Error> {
    let stream = TcpStream::connect(addr).await?;
    // This doesn't noticeably affect performance on local loopback tests
    // TODO(geoff): test on real network
    stream.set_nodelay(true)?;
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
    // This doesn't noticeably affect performance on local loopback tests
    // TODO(geoff): test on real network
    stream.set_nodelay(true)?;
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
            buf: BytesMut::with_capacity(read_buffer_size),
            max_message_size,
        }
    }

    /// Ensure at least `need` bytes are available in the buffer.
    ///
    /// **Note on allocation:** After `split_to().freeze()` in `read_record`,
    /// the returned `Bytes` and this `BytesMut` share the same backing
    /// allocation.  The next `reserve` call here will detect the shared
    /// refcount and copy remaining bytes into a fresh allocation.  This is
    /// the standard `bytes`-crate trade-off: zero-copy on the read side in
    /// exchange for a possible realloc on the next fill.  Callers that drop
    /// their `Bytes` handles promptly allow the `BytesMut` to reclaim the
    /// buffer and grow in-place.
    async fn fill(&mut self, need: usize) -> Result<(), std::io::Error> {
        while self.buf.len() < need {
            self.buf.reserve(need - self.buf.len());
            let n = self.read_half.read_buf(&mut self.buf).await?;
            if n == 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "connection closed",
                ));
            }
        }
        Ok(())
    }

    /// Read a single RPC record from the connection.
    ///
    /// Returns an owned, reference-counted [`Bytes`] handle — zero-copy
    /// from the internal read buffer.
    ///
    /// Returns `None` if the record is too large or requests a multi-part message.
    pub async fn read_record(&mut self) -> Result<Option<Bytes>, std::io::Error> {
        // Read the 4-byte record header.
        self.fill(4).await?;
        let header = u32::from_be_bytes(self.buf[..4].try_into().unwrap());
        let size = (header & 0x7FFF_FFFF) as usize;
        let last_fragment = header & 0x8000_0000 != 0;

        // Discard the 4-byte record-length header unconditionally so the
        // buffer is always positioned past the header on return (whether
        // we return data, None, or an error from fill).
        self.buf.advance(4);

        if !last_fragment {
            tracing::warn!(header, "multi-part messages unsupported");
            return Ok(None);
        }

        if size as u32 > self.max_message_size {
            tracing::warn!(header, "message size too large");
            return Ok(None);
        }

        // Ensure the full record body is available.
        self.fill(size).await?;

        // Split the record out of the buffer — O(1), zero-copy.
        Ok(Some(self.buf.split_to(size).freeze()))
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
        let (mut read, mut write) = connect(addr, 1024, 65536).await.unwrap();

        write.write_data(b"test message").await.unwrap();
        let mut buf = vec![0u8; (b"test message").len()];

        read.read_half.read_exact(&mut buf).await.unwrap();

        assert_eq!(buf, b"test message");
    }
}
