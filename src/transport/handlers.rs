use std::{
    cell::RefCell,
    io::{Error, ErrorKind},
    rc::Rc,
    sync::mpsc::Sender,
};

use bytes::Bytes;
use sfu::{RTCSessionDescription, ServerStates};
use tracing::info;

pub enum SignalingProtocolMessage {
    Ok {
        session_id: u64,
        endpoint_id: u64,
    },
    Err {
        session_id: u64,
        endpoint_id: u64,
        reason: Bytes,
    },
    Offer {
        session_id: u64,
        endpoint_id: u64,
        offer_sdp: Bytes,
    },
    Answer {
        session_id: u64,
        endpoint_id: u64,
        answer_sdp: Bytes,
    },
    Leave {
        session_id: u64,
        endpoint_id: u64,
    },
}

pub struct SignalingMessage {
    pub request: SignalingProtocolMessage,
    pub response_tx: Sender<SignalingProtocolMessage>,
}

pub fn handle_signaling_message(
    server_states: &Rc<RefCell<ServerStates>>,
    signaling_msg: SignalingMessage,
) -> std::io::Result<()> {
    match signaling_msg.request {
        SignalingProtocolMessage::Offer {
            session_id,
            endpoint_id,
            offer_sdp,
        } => handle_offer_message(
            server_states,
            session_id,
            endpoint_id,
            offer_sdp,
            signaling_msg.response_tx,
        ),
        SignalingProtocolMessage::Leave {
            session_id,
            endpoint_id,
        } => handle_leave_message(
            server_states,
            session_id,
            endpoint_id,
            signaling_msg.response_tx,
        ),
        SignalingProtocolMessage::Ok {
            session_id,
            endpoint_id,
        }
        | SignalingProtocolMessage::Err {
            session_id,
            endpoint_id,
            reason: _,
        }
        | SignalingProtocolMessage::Answer {
            session_id,
            endpoint_id,
            answer_sdp: _,
        } => Ok(signaling_msg
            .response_tx
            .send(SignalingProtocolMessage::Err {
                session_id,
                endpoint_id,
                reason: Bytes::from("Invalid Request"),
            })
            .map_err(|_| {
                Error::new(
                    ErrorKind::Other,
                    "failed to send back signaling message response".to_string(),
                )
            })?),
    }
}

fn handle_offer_message(
    server_states: &Rc<RefCell<ServerStates>>,
    session_id: u64,
    endpoint_id: u64,
    offer: Bytes,
    response_tx: Sender<SignalingProtocolMessage>,
) -> std::io::Result<()> {
    let try_handle = || -> std::io::Result<Bytes> {
        let offer_str = match String::from_utf8(offer.to_vec()) {
            Ok(offer_str) => offer_str,
            Err(err) => {
                return Err(Error::new(
                    ErrorKind::Other,
                    format!("failed to parse offer: {}", err),
                ))
            }
        };
        info!(
            "handle_offer_message: {}/{}/{}",
            session_id, endpoint_id, offer_str,
        );
        let mut server_states = server_states.borrow_mut();

        let offer_sdp = serde_json::from_str::<RTCSessionDescription>(&offer_str)?;
        let answer = match server_states.accept_offer(session_id, endpoint_id, None, offer_sdp) {
            Ok(answer) => answer,
            Err(err) => {
                return Err(Error::new(
                    ErrorKind::Other,
                    format!("failed to accept offer: {}", err),
                ))
            }
        };
        let answer_str = serde_json::to_string(&answer)?;
        info!("generate answer sdp: {}", answer_str);
        Ok(Bytes::from(answer_str))
    };

    match try_handle() {
        Ok(answer_sdp) => Ok(response_tx
            .send(SignalingProtocolMessage::Answer {
                session_id,
                endpoint_id,
                answer_sdp,
            })
            .map_err(|_| {
                Error::new(
                    ErrorKind::Other,
                    "failed to send back signaling message response".to_string(),
                )
            })?),
        Err(err) => Ok(response_tx
            .send(SignalingProtocolMessage::Err {
                session_id,
                endpoint_id,
                reason: Bytes::from(err.to_string()),
            })
            .map_err(|_| {
                Error::new(
                    ErrorKind::Other,
                    "failed to send back signaling message response".to_string(),
                )
            })?),
    }
}

fn handle_leave_message(
    _server_states: &Rc<RefCell<ServerStates>>,
    session_id: u64,
    endpoint_id: u64,
    response_tx: Sender<SignalingProtocolMessage>,
) -> std::io::Result<()> {
    let try_handle = || -> std::io::Result<()> {
        info!("handle_leave_message: {}/{}", session_id, endpoint_id,);
        Ok(())
    };

    match try_handle() {
        Ok(_) => Ok(response_tx
            .send(SignalingProtocolMessage::Ok {
                session_id,
                endpoint_id,
            })
            .map_err(|_| {
                Error::new(
                    ErrorKind::Other,
                    "failed to send back signaling message response".to_string(),
                )
            })?),
        Err(err) => Ok(response_tx
            .send(SignalingProtocolMessage::Err {
                session_id,
                endpoint_id,
                reason: Bytes::from(err.to_string()),
            })
            .map_err(|_| {
                Error::new(
                    ErrorKind::Other,
                    "failed to send back signaling message response".to_string(),
                )
            })?),
    }
}
