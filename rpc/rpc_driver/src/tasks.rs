use std::sync::Arc;

use crate::{
    ProgVers,
    connection::{ConnectionHandle, RpcSender, log_rpc_timestamp},
    context::with_peer,
    handler::ServiceMap,
    rpc_error::DEFAULT_OPAQUE_AUTH,
    rpc_msg,
    tcp_connection::ReadHalf,
};

#[derive(Debug, thiserror::Error)]
pub enum ReadTaskError {
    #[error("Shutdown")]
    Shutdown,
    #[error("Read error: {0}")]
    ReadError(#[from] std::io::Error),
    #[error("Write error: {0}")]
    WriteError(std::io::Error),
}

// Never returns (), just error when the connection is closed
pub async fn launch_read_task(
    service_map: Arc<ServiceMap>,
    client_cbs: Arc<ConnectionHandle>,
    mut read_half: ReadHalf,
) -> Result<(), ReadTaskError> {
    let mut write_buffer = xdrs::txs::buf_writer::BufWriter::default();
    loop {
        let record = read_half.read_record().await?;
        if record.is_none() {
            // Multi-part messages unsupported; if the record fails to read
            // we shut down the connection
            return Err(ReadTaskError::Shutdown);
        }
        let record = record.unwrap();

        let mut reader = xdrs::txs::buf_reader::BufReader::new(record);

        let msg: rpc_msg::rpc_msg = match xdrs::read_xdr(&mut reader) {
            Ok(msg) => msg,
            Err(_e) => {
                // if we get a bad msg record,
                // the intent is to close the connection
                return Err(ReadTaskError::Shutdown);
            }
        };

        let remaining_bytes: &[u8] = &record[reader.position()..];

        match msg.body {
            rpc_msg::rpc_msg_body::CALL { cbody } => {
                if client_cbs.log_timestamps {
                    log_rpc_timestamp(msg.xid, "server_recv");
                }
                let vers = ProgVers {
                    prog: cbody.prog,
                    vers: cbody.vers,
                };
                let sender: Arc<dyn RpcSender> = client_cbs.clone();
                let res = with_peer(sender, async {
                    service_map
                        .dispatch(cbody.rpcvers, vers, cbody.proc, remaining_bytes)
                        .await
                })
                .await;
                match res {
                    Ok(response) => {
                        prepare_success_response(msg.xid, &response, &mut write_buffer)?;
                        if client_cbs.log_timestamps {
                            log_rpc_timestamp(msg.xid, "server_send");
                        }
                        client_cbs
                            .send_msg(write_buffer.as_ref())
                            .await
                            .map_err(ReadTaskError::WriteError)?;
                    }
                    Err(e) => {
                        if client_cbs.log_timestamps {
                            log_rpc_timestamp(msg.xid, "server_send");
                        }
                        let out_bytes = e.to_error_record(msg.xid);
                        client_cbs
                            .send_msg(&out_bytes)
                            .await
                            .map_err(ReadTaskError::WriteError)?;
                    }
                }
            }
            rpc_msg::rpc_msg_body::REPLY { rbody } => match rbody {
                rpc_msg::reply_body::MSG_ACCEPTED { areply } => {
                    // Handle_reply only returns an error if the client xid is not found,
                    // which signals bizarre error and is handled by shutting down the connection.
                    if client_cbs.log_timestamps {
                        log_rpc_timestamp(msg.xid, "client_recv");
                    }
                    if let rpc_msg::accepted_reply_reply_data::SUCCESS { results: _results } =
                        areply.reply_data
                    {
                        client_cbs
                            .connection_state
                            .lock()
                            .unwrap()
                            .handle_reply(msg.xid, Ok(remaining_bytes.to_vec()));
                    } else {
                        client_cbs
                            .connection_state
                            .lock()
                            .unwrap()
                            .handle_reply(msg.xid, Err(areply.reply_data.into()));
                    }
                }
                rpc_msg::reply_body::MSG_DENIED { rreply } => {
                    if client_cbs.log_timestamps {
                        log_rpc_timestamp(msg.xid, "client_recv");
                    }
                    client_cbs
                        .connection_state
                        .lock()
                        .unwrap()
                        .handle_reply(msg.xid, Err(rreply.into()));
                }
            },
        }
    }
}

fn get_success_header(xid: u32) -> rpc_msg::rpc_msg {
    rpc_msg::rpc_msg {
        xid,
        body: rpc_msg::rpc_msg_body::REPLY {
            rbody: rpc_msg::reply_body::MSG_ACCEPTED {
                areply: rpc_msg::accepted_reply {
                    verf: DEFAULT_OPAQUE_AUTH,
                    reply_data: rpc_msg::accepted_reply_reply_data::SUCCESS { results: [0u8; 0] },
                },
            },
        },
    }
}

fn prepare_success_response(
    xid: u32,
    results: &[u8],
    writer: &mut xdrs::txs::buf_writer::BufWriter,
) -> Result<(), ReadTaskError> {
    writer.clear();
    let header = get_success_header(xid);

    // placeholder for message length
    xdrs::write_xdr(&0u32, writer).unwrap();
    // write the header
    xdrs::write_xdr(&header, writer).unwrap();

    let header_len = writer.len() - 4;

    // Check for integer overflow when calculating message length
    // Total length must fit in u32 AND have top bit clear (for ORing with 0x8000_0000)
    let total_len = header_len
        .checked_add(results.len())
        .and_then(|sum| u32::try_from(sum).ok())
        .filter(|&len| len <= 0x7FFF_FFFF)
        .ok_or_else(|| {
            eprintln!(
                "Message too large: header={} + results={} exceeds max (0x7FFF_FFFF)",
                writer.len(),
                results.len()
            );
            ReadTaskError::Shutdown
        })?;

    let msg_len: u32 = total_len | 0x8000_0000;

    {
        let bytes: &mut [u8] = writer.as_mut();
        bytes[..4].copy_from_slice(&msg_len.to_be_bytes());
    }

    writer.as_mut().extend_from_slice(results);

    Ok(())
}
