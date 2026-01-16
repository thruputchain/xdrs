use std::sync::Arc;

use tokio::sync::mpsc;

use crate::{
    client_state::ClientHandle,
    rpc_error::DEFAULT_OPAQUE_AUTH,
    rpc_msg,
    server_state::ServiceMap,
    tcp_connection::{ReadHalf, WriteHalf},
    ProgVers,
};

#[derive(Debug, thiserror::Error)]
pub enum ReadTaskError {
    #[error("Shutdown")]
    Shutdown,
    #[error("Read error: {0}")]
    ReadError(#[from] std::io::Error),
    #[error("Write error: {0}")]
    WriteError(#[from] mpsc::error::SendError<Vec<u8>>),
}

#[derive(Debug, thiserror::Error)]
pub enum WriteTaskError {
    #[error("Shutdown")]
    Shutdown,
    #[error("Write error: {0}")]
    WriteError(#[from] std::io::Error),
    #[error("Recv error")]
    RecvError,
}

pub async fn launch_read_task(
    service_map: Arc<ServiceMap>,
    client_cbs: Arc<ClientHandle>,
    mut read_half: ReadHalf,
) -> Result<(), ReadTaskError> {
    loop {
        let record = read_half.read_record().await?;
        if record.is_none() {
            continue;
        }

        let mut write_buffer = Vec::new();

        let mut reader = xdrs::txs::buf_reader::BufReader::new(record.unwrap());

        let msg: rpc_msg::rpc_msg = match xdrs::read_xdr(&mut reader) {
            Ok(msg) => msg,
            Err(_e) => {
                // if we get a bad msg record,
                // the intent is to close the connection
                return Err(ReadTaskError::Shutdown);
            }
        };

        let remaining_bytes: &[u8] = &record.unwrap()[reader.position()..];

        match msg.body {
            rpc_msg::rpc_msg_body::CALL { cbody } => {
                let vers = ProgVers {
                    prog: cbody.prog,
                    vers: cbody.vers,
                };
                let res = service_map
                    .dispatch(cbody.rpcvers, vers, cbody.proc, remaining_bytes)
                    .await;
                match res {
                    Ok(response) => {
                        prepare_success_response(msg.xid, &response, &mut write_buffer);
                        client_cbs
                            .send_msg(&write_buffer)
                            .map_err(ReadTaskError::WriteError)?;
                    }
                    Err(e) => {
                        let out_bytes = e.to_error_record(msg.xid);

                        client_cbs
                            .send_msg(&out_bytes)
                            .map_err(ReadTaskError::WriteError)?;
                    }
                }
            }
            rpc_msg::rpc_msg_body::REPLY { rbody } => match rbody {
                rpc_msg::reply_body::MSG_ACCEPTED { areply } => {
                    // Handle_reply only returns an error if the client xid is not found,
                    // which signals bizarre error and is handled by shutting down the connection.
                    if let rpc_msg::accepted_reply_reply_data::SUCCESS { results: _results } =
                        areply.reply_data
                    {
                        client_cbs
                            .client_state
                            .write()
                            .await
                            .handle_reply(msg.xid, Ok(remaining_bytes.to_vec()));
                    } else {
                        client_cbs
                            .client_state
                            .write()
                            .await
                            .handle_reply(msg.xid, Err(areply.reply_data.into()));
                    }
                }
                rpc_msg::reply_body::MSG_DENIED { rreply } => {
                    client_cbs
                        .client_state
                        .write()
                        .await
                        .handle_reply(msg.xid, Err(rreply.into()));
                }
            },
        }
    }
}

fn get_success_header(xid: u32) -> rpc_msg::rpc_msg {
    return rpc_msg::rpc_msg {
        xid,
        body: rpc_msg::rpc_msg_body::REPLY {
            rbody: rpc_msg::reply_body::MSG_ACCEPTED {
                areply: rpc_msg::accepted_reply {
                    verf: DEFAULT_OPAQUE_AUTH,
                    reply_data: rpc_msg::accepted_reply_reply_data::SUCCESS { results: [0u8; 0] },
                },
            },
        },
    };
}

fn prepare_success_response(xid: u32, results: &[u8], buf: &mut Vec<u8>) {
    buf.clear();
    let header = get_success_header(xid);

    let header_bytes = xdrs::txs::encode(&header).unwrap();
    let msg_len: u32 = (header_bytes.len() as u32 + results.len() as u32) | 0x8000_0000;

    buf.extend_from_slice(&msg_len.to_be_bytes());
    buf.extend_from_slice(&header_bytes);
    buf.extend_from_slice(results);
}

pub async fn launch_write_task(
    mut receive_channel: mpsc::UnboundedReceiver<Vec<u8>>,
    mut write_half: WriteHalf,
) -> Result<(), WriteTaskError> {
    loop {
        let msg = receive_channel
            .recv()
            .await
            .ok_or(WriteTaskError::RecvError)?;
        write_half
            .write_data(&msg)
            .await
            .map_err(WriteTaskError::WriteError)?;
    }
}
