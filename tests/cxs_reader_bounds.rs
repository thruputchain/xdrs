use xdrs::cxs::buf_reader::BufReader as CxsReader;
use xdrs::serialize::reader::{ReadError, Reader};

fn assert_source_empty(error: ReadError, requested: usize, remaining: usize) {
    #[cfg(feature = "detailed-errors")]
    assert!(
        matches!(error, ReadError::SourceEmpty { requested: n, remaining: m }
        if n == requested && m == remaining)
    );
    #[cfg(not(feature = "detailed-errors"))]
    let _ = (error, requested, remaining);
}

#[test]
fn cxs_failed_reads_preserve_the_cursor_and_remaining_input() {
    let data = [1, 2, 3, 4];
    let mut reader = CxsReader::new(&data);
    assert_eq!(reader.read_opaque_fixed(1).unwrap(), &data[..1]);

    // The maximum wire length also exercises addition overflow on 32-bit
    // targets without constructing a large buffer.
    for requested in [4, u32::MAX as usize] {
        assert_source_empty(
            reader.read_opaque_fixed(requested).unwrap_err(),
            requested,
            3,
        );
        assert_eq!(reader.position(), 1);
        assert_eq!(reader.remaining_slice(), &data[1..]);
    }

    assert_eq!(reader.read_opaque_fixed(3).unwrap(), &data[1..]);
    assert!(reader.read_opaque_fixed(0).unwrap().is_empty());
    assert_source_empty(reader.read_u64().unwrap_err(), 1, 0);
    assert_eq!(reader.position(), data.len());
    assert_eq!(reader.remaining(), 0);
}

#[test]
fn cxs_truncated_integer_payloads_consume_only_the_header() {
    for (header, width) in [(0x3f, 1), (0x7f, 2), (0xbf, 4), (0xff, 8)] {
        // All-ones payloads are minimally encoded at each width.
        let mut data = [0xff; 10];
        data[0] = 7;
        data[1] = header;
        for available in 0..=width {
            let mut reader = CxsReader::new(&data[..2 + available]);
            assert_eq!(reader.read_u64().unwrap(), 7);
            if available < width {
                assert_source_empty(reader.read_u64().unwrap_err(), width, available);
                assert_eq!(reader.position(), 2);
                assert_eq!(reader.remaining(), available);
            } else {
                let expected = u64::MAX >> (64 - width * 8);
                assert_eq!(reader.read_u64().unwrap(), expected);
                assert_eq!(reader.position(), 2 + width);
            }
        }
    }
}

#[test]
fn cxs_unsigned_single_byte_values_and_reserved_headers() {
    for header in 0..=u8::MAX {
        if header & 0x3f == 0x3f {
            continue; // Payload encodings are exercised above.
        }
        let data = [header, 7];
        let mut reader = CxsReader::new(&data);
        if header < 63 {
            assert_eq!(reader.read_u64().unwrap(), header as u64);
        } else {
            let error = reader.read_u64().unwrap_err();
            #[cfg(feature = "detailed-errors")]
            assert!(matches!(error, ReadError::InvalidHeaderByte { h } if h == header));
            #[cfg(not(feature = "detailed-errors"))]
            let _ = error;
        }
        assert_eq!(reader.position(), 1);
        assert_eq!(reader.read_u64().unwrap(), 7);
    }
}
