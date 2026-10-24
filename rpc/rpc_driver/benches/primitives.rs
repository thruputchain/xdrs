//! x23-style component timings. Each sample times a batch, not an individual op.
use std::hint::black_box;
use std::time::Instant;

fn measure(name: &str, iterations: usize, mut operation: impl FnMut()) -> serde_json::Value {
    for _ in 0..1000 {
        operation();
    }
    let mut trials = Vec::new();
    for _ in 0..7 {
        let start = Instant::now();
        for _ in 0..iterations {
            operation();
        }
        trials.push(start.elapsed().as_secs_f64() * 1e9 / iterations as f64);
    }
    trials.sort_by(f64::total_cmp);
    serde_json::json!({"name": name, "unit": "ns/op", "median": trials[3],
        "trials": trials, "iterations_per_trial": iterations})
}

fn main() {
    let iterations = if std::env::args().any(|a| a == "--quick") {
        1000
    } else {
        100_000
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let _guard = runtime.enter();
    let mut results = Vec::new();
    results.push(measure("empty_function", iterations, || black_box(|| {})()));
    results.push(measure("ready_future", iterations, || {
        let mut future = std::pin::pin!(std::future::ready(black_box(0u32)));
        let mut context = std::task::Context::from_waker(std::task::Waker::noop());
        let _ = black_box(std::future::Future::poll(future.as_mut(), &mut context));
    }));
    // Same wire sizes as x23's fix_4 and fix_4_100. Includes buffer allocation.
    for count in [0usize, 1, 100] {
        results.push(measure(
            &format!("serialize_xdr/{count}_u32"),
            iterations,
            || {
                let mut writer = xdrs::txs::buf_writer::BufWriter::with_capacity(count * 4);
                for _ in 0..count {
                    xdrs::write_xdr(&black_box(42u32), &mut writer).unwrap();
                }
                black_box(writer);
            },
        ));
        let bytes = [0, 0, 0, 42].repeat(count);
        results.push(measure(
            &format!("deserialize_xdr/{count}_u32"),
            iterations,
            || {
                let mut reader = xdrs::txs::buf_reader::BufReader::new(black_box(&bytes));
                for _ in 0..count {
                    let value: u32 = xdrs::read_xdr(&mut reader).unwrap();
                    black_box(value);
                }
            },
        ));
    }
    // Measure a scheduler round trip separately from ready-future polling.
    results.push(runtime.block_on(async {
        let start = Instant::now();
        for _ in 0..iterations {
            tokio::task::yield_now().await;
        }
        serde_json::json!({"name": "yield_current_thread", "unit": "ns/op",
            "mean": start.elapsed().as_secs_f64() * 1e9 / iterations as f64,
            "iterations": iterations})
    }));
    for payload_size in [4usize, 400, 4096, 65536] {
        results.push(runtime.block_on(records(
            payload_size,
            if iterations == 1000 { 100 } else { 10_000 },
        )));
    }
    println!("{}", serde_json::to_string_pretty(&results).unwrap());
}

async fn records(payload_size: usize, count: usize) -> serde_json::Value {
    use thruput_rpc_driver::tcp_connection::{accept, connect};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (client, server) = tokio::join!(
        connect(listener.local_addr().unwrap(), 1 << 20, 65536),
        accept(&listener, 1 << 20, 65536),
    );
    let (mut reader, _client_writer) = client.unwrap();
    let (_server_reader, mut writer, _) = server.unwrap();
    let mut record = (0x8000_0000 | payload_size as u32).to_be_bytes().to_vec();
    record.resize(payload_size + 4, 42);
    // Warm the receive buffer before timing (including growth for large records).
    let (written, read) = tokio::join!(writer.write_data(&record), reader.read_record());
    written.unwrap();
    assert_eq!(read.unwrap().unwrap().len(), payload_size);
    let start_gate = std::sync::Arc::new(tokio::sync::Barrier::new(2));
    let sender_gate = start_gate.clone();
    let sender = tokio::spawn(async move {
        sender_gate.wait().await;
        for _ in 0..count {
            writer.write_data(&record).await.unwrap();
        }
    });
    let start = Instant::now();
    start_gate.wait().await;
    for _ in 0..count {
        let record = reader.read_record().await.unwrap().unwrap();
        assert_eq!(record.len(), payload_size);
        black_box(record);
    }
    let elapsed = start.elapsed();
    sender.await.unwrap();
    serde_json::json!({"name": format!("tcp_records/{payload_size}"), "unit": "ns/record",
        "mean": elapsed.as_secs_f64() * 1e9 / count as f64, "records": count,
        "read_buffer_bytes": 65536, "payload_bytes": payload_size})
}
