use std::{
    collections::HashMap,
    sync::mpsc::{self, Sender},
};

use actix_web::{
    get, post,
    web::{self, Data},
    HttpResponse, Responder,
};
use bytes::Bytes;
use tracing::{error, info};

use crate::transport::handlers::{SignalingMessage, SignalingProtocolMessage};

#[derive(Debug, serde::Deserialize, serde::Serialize)]
struct RTCSessionDescriptionSerializable {
    sdp: String,
    #[serde(rename = "type")]
    sdp_type: String,
}

#[get("/health")]
pub async fn health() -> impl Responder {
    HttpResponse::Ok().body("OK")
}

#[post("/offer/{session}/{endpoint}")]
pub async fn handle_offer(
    path: web::Path<(String, String)>,
    offer_sdp: web::Json<RTCSessionDescriptionSerializable>,
    media_port_thread_map: Data<HashMap<u16, Sender<SignalingMessage>>>,
) -> impl Responder {
    let path = path.into_inner();
    let session_id = u64::from_str_radix(&path.0, 10).unwrap();
    let endpoint_id = u64::from_str_radix(&path.1, 10).unwrap();
    let sorted_ports: Vec<u16> = media_port_thread_map.keys().map(|x| *x).collect();
    let port = sorted_ports[(session_id as usize) % sorted_ports.len()];
    let (response_tx, response_rx) = mpsc::channel();
    let payload_to_string = serde_json::to_string(&offer_sdp).map_err(|e| {
        error!("Error serializing offer: {}", e);
        HttpResponse::InternalServerError().body("Error serializing offer")
    });

    let offer_sdp = match payload_to_string {
        Ok(s) => {
            info!("Received offer: {}", s);
            Bytes::from(s)
        }
        Err(r) => return r,
    };
    match media_port_thread_map.get(&port) {
        Some(tx) => {
            tx.send(SignalingMessage {
                request: SignalingProtocolMessage::Offer {
                    session_id,
                    endpoint_id,
                    offer_sdp: offer_sdp,
                },
                response_tx,
            })
            .unwrap();
        }
        None => {
            return HttpResponse::InternalServerError().body("No media port available");
        }
    };

    let response = response_rx.recv().expect("receive answer offer");
    let to_send = match response {
        SignalingProtocolMessage::Answer {
            session_id: _,
            endpoint_id: _,
            answer_sdp,
        } => HttpResponse::Ok()
            .content_type("application/json")
            .body(answer_sdp),
        SignalingProtocolMessage::Err {
            session_id,
            endpoint_id,
            reason,
        } => {
            let reason_str = std::str::from_utf8(&reason).unwrap();
            error!(
                "Error for session {} endpoint {}: {}",
                session_id, endpoint_id, reason_str
            );
            HttpResponse::InternalServerError().body(format!(
                "Error for session {} endpoint {}: {}",
                session_id, endpoint_id, reason_str
            ))
        }
        SignalingProtocolMessage::Leave {
            session_id,
            endpoint_id,
        } => {
            error!("Session {} endpoint {} left", session_id, endpoint_id);
            return HttpResponse::InternalServerError().body("Session left");
        }
        SignalingProtocolMessage::Offer {
            session_id,
            endpoint_id,
            offer_sdp,
        } => {
            let offer_sdp_str = std::str::from_utf8(&offer_sdp).unwrap();
            error!(
                "Received offer for session {} endpoint {} while expecting answer with sdp {}",
                session_id, endpoint_id, offer_sdp_str
            );
            return HttpResponse::InternalServerError()
                .body("Received offer for session endpoint while expecting answer");
        }

        SignalingProtocolMessage::Ok {
            session_id,
            endpoint_id,
        } => {
            error!(
                "Received Ok for session {} endpoint {} while expecting answer",
                session_id, endpoint_id
            );
            return HttpResponse::InternalServerError()
                .body("Received Ok for session endpoint while expecting answer");
        }
    };
    to_send
}

#[post("/leave/{session}/{endpoint}")]
pub async fn leave() -> impl Responder {
    HttpResponse::Ok() //idk what to do here
}
