use tokio::sync::{mpsc, RwLock};

use crate::{
    client_state::ClientState, rpc_error::DEFAULT_OPAQUE_AUTH, rpc_msg, server_state::ServiceMap,
    tcp_connection::ReadHalf, ProcVers,
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

pub async fn launch_read_task(
    service_map: ServiceMap,
    client_cbs: &RwLock<ClientState>,
    mut read_half: ReadHalf,
    write_queue: mpsc::UnboundedSender<Vec<u8>>,
) -> Result<(), ReadTaskError> {
    loop {
        let record = read_half.read_record().await?;

        let mut write_buffer = Vec::new();

        let mut reader = xdrs::serialize_txs::buf_reader::BufReader::new(record);

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
                let vers = ProcVers {
                    proc: cbody.proc,
                    vers: cbody.vers,
                };
                let res = service_map
                    .dispatch(cbody.rpcvers, vers, msg.xid, remaining_bytes)
                    .await;
                match res {
                    Ok(response) => {
                        prepare_success_response(msg.xid, &response, &mut write_buffer);
                        write_queue
                            .send(write_buffer)
                            .map_err(ReadTaskError::WriteError)?;
                    }
                    Err(e) => {
                        write_queue
                            .send(xdrs::serialize_txs::encode(&e.to_rpc_msg(msg.xid)).unwrap())
                            .map_err(ReadTaskError::WriteError)?;
                    }
                }
            }
            rpc_msg::rpc_msg_body::REPLY { rbody } => match rbody {
                rpc_msg::reply_body::MSG_ACCEPTED { areply } => {
                    if let rpc_msg::accepted_reply_reply_data::SUCCESS { results: _results } =
                        areply.reply_data
                    {
                        client_cbs
                            .write()
                            .await
                            .handle_reply(msg.xid, Ok(remaining_bytes.to_vec()));
                    } else {
                        client_cbs
                            .write()
                            .await
                            .handle_reply(msg.xid, Err(areply.reply_data.into()));
                    }
                }
                rpc_msg::reply_body::MSG_DENIED { rreply } => {
                    client_cbs
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

    let header_bytes = xdrs::serialize_txs::encode(&header).unwrap();
    let msg_len: u32 = header_bytes.len() as u32 + results.len() as u32;

    buf.extend_from_slice(&msg_len.to_be_bytes());
    buf.extend_from_slice(&header_bytes);
    buf.extend_from_slice(results);
}
