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

    #[cfg(test)]
    pub(crate) fn from_inner(write_half: tokio::net::tcp::OwnedWriteHalf) -> Self {
        Self { write_half }
    }

    pub(crate) fn into_inner(self) -> tokio::net::tcp::OwnedWriteHalf {
        self.write_half
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
            max_message_size,
        }
    }

    /// A reader over `read_half` with test-sized limits.
    #[cfg(test)]
    pub(crate) fn new_for_test(read_half: tokio::net::tcp::OwnedReadHalf) -> Self {
        Self::new(read_half, 1 << 20, 4096)
    }

    /// Number of unconsumed bytes in the read buffer.
    #[inline]
    fn buffered(&self) -> usize {
        self.len - self.pos
    }

    /// Make room for `need` bytes of record in `buf[pos..]`: compact if the
    /// unconsumed data cannot be extended in place, grow if the whole buffer
    /// is too small.
    fn make_room(&mut self, need: usize) {
        if self.pos > 0 && self.buf.len() - self.len < need.saturating_sub(self.buffered()) {
            self.buf.copy_within(self.pos..self.len, 0);
            self.len -= self.pos;
            self.pos = 0;
        }
        if need > self.buf.len() {
            self.buf.resize(need, 0);
        }
    }

    /// Ensure at least `need` bytes are available in `buf[pos..]`.
    /// Compacts the buffer first if necessary to make room, then reads
    /// from the socket until we have enough.
    async fn fill(&mut self, need: usize) -> Result<(), std::io::Error> {
        self.make_room(need);
        while self.buffered() < need {
            let n = self.read_half.read(&mut self.buf[self.len..]).await?;
            if n == 0 {
                return Err(eof());
            }
            self.len += n;
        }
        Ok(())
    }

    /// Like [`fill`](Self::fill) but never waits: returns `Ok(false)` when
    /// the socket has no more data right now.
    fn try_fill(&mut self, need: usize) -> Result<bool, std::io::Error> {
        self.make_room(need);
        while self.buffered() < need {
            let result = self.read_half.try_read(&mut self.buf[self.len..]);
            match result {
                Ok(0) => return Err(eof()),
                Ok(n) => self.len += n,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => return Ok(false),
                Err(e) => return Err(e),
            }
        }
        Ok(true)
    }

    /// Validate the record mark at `pos`; `Some(size)` is the body length of
    /// a single-fragment record within the size limit.
    fn record_size(&self) -> Option<usize> {
        let header = u32::from_be_bytes(self.buf[self.pos..self.pos + 4].try_into().unwrap());
        let size = (header & 0x7FFF_FFFF) as usize;
        if header & 0x8000_0000 == 0 {
            tracing::warn!(header, "multi-part messages unsupported");
            return None;
        }
        if size as u32 > self.max_message_size {
            tracing::warn!(header, "message size too large");
            return None;
        }
        Some(size)
    }

    /// Consume the record whose `size`-byte body follows the mark at `pos`.
    fn take_record(&mut self, size: usize) -> &[u8] {
        let start = self.pos + 4;
        self.pos = start + size;
        &self.buf[start..self.pos]
    }

    /// Read a single RPC record from the connection.
    ///
    /// Returns `None` if the record is too large or requests a multi-part message.
    pub async fn read_record(&mut self) -> Result<Option<&[u8]>, std::io::Error> {
        // Read the 4-byte record header.
        self.fill(4).await?;
        let Some(size) = self.record_size() else {
            return Ok(None);
        };
        // Keep the header until the whole record is buffered. Besides avoiding
        // a second allocation/copy for split records, fill reads ahead into any
        // spare capacity. Its progress lives in self, so cancelling this future
        // never loses a partial header or body.
        if self.buffered() < size + 4 {
            self.fill(size + 4).await?;
        }
        Ok(Some(self.take_record(size)))
    }

    /// Read a single RPC record if one is already available or can be read
    /// without waiting.
    ///
    /// `WouldBlock` means the socket has no more data right now; the caller
    /// should await [`readable`](Self::readable) and try again. Buffered
    /// partial records are kept, so this never loses input.
    pub fn try_read_record(&mut self) -> Result<TryRecord<'_>, std::io::Error> {
        if !self.try_fill(4)? {
            return Ok(TryRecord::WouldBlock);
        }
        let Some(size) = self.record_size() else {
            return Ok(TryRecord::Invalid);
        };
        if self.buffered() < size + 4 && !self.try_fill(size + 4)? {
            return Ok(TryRecord::WouldBlock);
        }
        Ok(TryRecord::Ready(self.take_record(size)))
    }

    /// Wait until the socket may have data for
    /// [`try_read_record`](Self::try_read_record).
    pub async fn readable(&self) -> Result<(), std::io::Error> {
        self.read_half.readable().await
    }
}

/// Outcome of [`ReadHalf::try_read_record`].
#[derive(Debug)]
pub enum TryRecord<'a> {
    /// A complete record.
    Ready(&'a [u8]),
    /// The record mark was invalid (multi-part or over the size limit).
    Invalid,
    /// No complete record is available without waiting on the socket.
    WouldBlock,
}

fn eof() -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "connection closed")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    async fn record_pair(buffer_size: usize) -> (ReadHalf, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let (client, server) = tokio::join!(
            TcpStream::connect(listener.local_addr().unwrap()),
            listener.accept(),
        );
        let (read, _) = client.unwrap().into_split();
        (ReadHalf::new(read, 1024, buffer_size), server.unwrap().0)
    }

    fn record(payload: &[u8]) -> Vec<u8> {
        let mut bytes = (0x8000_0000 | payload.len() as u32).to_be_bytes().to_vec();
        bytes.extend_from_slice(payload);
        bytes
    }

    #[tokio::test]
    async fn split_record_survives_cancellation_at_every_byte() {
        let expected = record(b"split payload");
        for split in 1..expected.len() {
            let (mut read, mut peer) = record_pair(8).await;
            peer.write_all(&expected[..split]).await.unwrap();
            assert!(
                tokio::time::timeout(Duration::from_millis(5), read.read_record())
                    .await
                    .is_err()
            );
            peer.write_all(&expected[split..]).await.unwrap();
            peer.write_all(&record(b"next")).await.unwrap();
            peer.shutdown().await.unwrap();
            assert_eq!(read.read_record().await.unwrap().unwrap(), b"split payload");
            assert_eq!(read.read_record().await.unwrap().unwrap(), b"next");
            assert_eq!(
                read.read_record().await.unwrap_err().kind(),
                std::io::ErrorKind::UnexpectedEof
            );
        }
    }

    #[tokio::test]
    async fn records_survive_buffer_compaction_and_growth() {
        for buffer_size in [0, 1, 4, 17, 64, 4096] {
            let (mut read, mut peer) = record_pair(buffer_size).await;
            let payloads = [vec![], vec![1; 11], vec![2; 1024], vec![3; 5]];
            let bytes: Vec<u8> = payloads.iter().flat_map(|p| record(p)).collect();
            peer.write_all(&bytes).await.unwrap();
            peer.shutdown().await.unwrap();
            for payload in &payloads {
                assert_eq!(read.read_record().await.unwrap().unwrap(), payload);
            }
        }
    }

    #[tokio::test]
    async fn try_read_record_reports_would_block_and_keeps_partial_input() {
        let (mut read, mut peer) = record_pair(8).await;
        assert!(matches!(
            read.try_read_record().unwrap(),
            TryRecord::WouldBlock
        ));
        let expected = record(b"split payload");
        peer.write_all(&expected[..7]).await.unwrap();
        peer.flush().await.unwrap();
        read.readable().await.unwrap();
        assert!(matches!(
            read.try_read_record().unwrap(),
            TryRecord::WouldBlock
        ));
        peer.write_all(&expected[7..]).await.unwrap();
        peer.write_all(&record(b"next")).await.unwrap();
        peer.flush().await.unwrap();
        read.readable().await.unwrap();
        loop {
            match read.try_read_record().unwrap() {
                TryRecord::Ready(payload) => {
                    assert_eq!(payload, b"split payload");
                    break;
                }
                TryRecord::WouldBlock => read.readable().await.unwrap(),
                TryRecord::Invalid => panic!("invalid record"),
            }
        }
        loop {
            match read.try_read_record().unwrap() {
                TryRecord::Ready(payload) => {
                    assert_eq!(payload, b"next");
                    break;
                }
                TryRecord::WouldBlock => read.readable().await.unwrap(),
                TryRecord::Invalid => panic!("invalid record"),
            }
        }
        peer.shutdown().await.unwrap();
        loop {
            match read.try_read_record() {
                Ok(TryRecord::WouldBlock) => read.readable().await.unwrap(),
                Err(e) => {
                    assert_eq!(e.kind(), std::io::ErrorKind::UnexpectedEof);
                    break;
                }
                other => panic!("unexpected: {other:?}"),
            }
        }
    }

    #[tokio::test]
    async fn invalid_header_does_not_grow_receive_buffer() {
        for header in [0x8000_0401u32, 0x0000_0001] {
            let (mut read, mut peer) = record_pair(8).await;
            peer.write_all(&header.to_be_bytes()).await.unwrap();
            assert!(read.read_record().await.unwrap().is_none());
            assert_eq!(read.buf.len(), 8);
        }
    }

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
