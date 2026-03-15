//! TCP connection handling for RPC record streams.

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, ToSocketAddrs};

/// Write half of a TCP connection.
#[derive(Debug)]
pub struct WriteHalf {
    write_half: tokio::net::tcp::OwnedWriteHalf,
}

const READ_BUF_SIZE: usize = 65536;

/// Read half of a TCP connection with record framing.
///
/// Uses an internal read buffer to amortize syscalls: a single `read()`
/// may return multiple records, which are then parsed without further
/// syscalls until the buffer is depleted.
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

pub async fn connect<A: ToSocketAddrs>(
    addr: A,
    max_message_size: u32,
) -> Result<(ReadHalf, WriteHalf), std::io::Error> {
    let stream = TcpStream::connect(addr).await?;
    // This doesn't noticeably affect performance on local loopback tests
    // TODO(geoff): test on real network
    stream.set_nodelay(true)?;
    let (read_half, write_half) = stream.into_split();
    Ok((
        ReadHalf::new(read_half, max_message_size),
        WriteHalf { write_half },
    ))
}

pub async fn accept(
    listener: &TcpListener,
    max_message_size: u32,
) -> Result<(ReadHalf, WriteHalf), std::io::Error> {
    let (stream, _) = listener.accept().await?;
    // This doesn't noticeably affect performance on local loopback tests
    // TODO(geoff): test on real network
    stream.set_nodelay(true)?;
    let (read_half, write_half) = stream.into_split();
    Ok((
        ReadHalf::new(read_half, max_message_size),
        WriteHalf { write_half },
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
        self.write_half.write_vectored(bufs).await
    }
}

impl ReadHalf {
    fn new(read_half: tokio::net::tcp::OwnedReadHalf, max_message_size: u32) -> Self {
        Self {
            read_half,
            buf: vec![0u8; READ_BUF_SIZE],
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
        // `need` bytes without growing.
        if self.pos > 0 && self.pos + need > self.buf.len() {
            self.buf.copy_within(self.pos..self.len, 0);
            self.len -= self.pos;
            self.pos = 0;
        }

        // Grow if the record is larger than our buffer.
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
            eprintln!("Multi-part messages unsupported: n={header:X}");
            return Ok(None);
        }

        if size as u32 > self.max_message_size {
            eprintln!("Message size too large: n={header:X}");
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
        let (mut read, mut write) = connect(addr, 1024).await.unwrap();

        write.write_data(b"test message").await.unwrap();
        let mut buf = vec![0u8; (b"test message").len()];

        read.read_half.read_exact(&mut buf).await.unwrap();

        assert_eq!(buf, b"test message");
    }
}
