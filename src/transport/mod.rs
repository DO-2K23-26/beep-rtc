use bytes::BytesMut;
use log::error;
use retty::channel::{InboundPipeline, Pipeline};
use retty::transport::{TaggedBytesMut, TransportContext};
use sfu::{
    DataChannelHandler, DemuxerHandler, DtlsHandler, ExceptionHandler, GatewayHandler,
    InterceptorHandler, SctpHandler, ServerConfig, ServerStates, SrtpHandler, StunHandler,
};
use std::cell::RefCell;
use std::io::ErrorKind;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::rc::Rc;
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::{Duration, Instant};
use std::vec;
use tracing::info;

use crate::transport::handlers::handle_signaling_message;

use self::handlers::SignalingMessage;

pub mod handlers;

/// This is the "main run loop" that handles all clients, reads and writes UdpSocket traffic,
/// and forwards media data between clients.
pub fn sync_run(
    stop_rx: crossbeam_channel::Receiver<()>,
    socket: UdpSocket,
    rx: Receiver<SignalingMessage>,
    server_config: Arc<ServerConfig>,
    server_ip: SocketAddr,
) -> std::io::Result<()> {
    let server_states_config = ServerStates::new(server_config, server_ip).unwrap();

    let server_states = Rc::new(RefCell::new(server_states_config));

    info!("listening {}...", socket.local_addr()?);

    let pipeline = build_pipeline(server_ip, server_states.clone());

    let mut buf = vec![0; 2000];

    pipeline.transport_active();
    loop {
        match stop_rx.try_recv() {
            Ok(_) => break,
            Err(err) => {
                if err.is_disconnected() {
                    break;
                }
            }
        };

        write_socket_output(&socket, &pipeline)?;

        // Spawn new incoming signal message from the signaling server thread.
        if let Ok(signal_message) = rx.try_recv() {
            if let Err(err) = handle_signaling_message(&server_states, signal_message) {
                error!("handle_signaling_message got error:{}", err);
                continue;
            }
        }

        // Poll clients until they return timeout
        let mut eto = Instant::now() + Duration::from_millis(100);
        pipeline.poll_timeout(&mut eto);

        let delay_from_now = eto
            .checked_duration_since(Instant::now())
            .unwrap_or(Duration::from_secs(0));
        if delay_from_now.is_zero() {
            pipeline.handle_timeout(Instant::now());
            continue;
        }

        socket
            .set_read_timeout(Some(delay_from_now))
            .expect("setting socket read timeout");

        if let Some(input) = read_socket_input(&socket, &mut buf, server_ip) {
            pipeline.read(input);
        }

        // Drive time forward in all clients.
        pipeline.handle_timeout(Instant::now());
    }
    pipeline.transport_inactive();

    info!(
        "media server on {} is gracefully down",
        server_ip
    );
    Ok(())
}

fn write_socket_output(
    socket: &UdpSocket,
    pipeline: &Rc<Pipeline<TaggedBytesMut, TaggedBytesMut>>,
) -> std::io::Result<()> {
    while let Some(transmit) = pipeline.poll_transmit() {
        socket.send_to(&transmit.message, transmit.transport.peer_addr)?;
    }

    Ok(())
}

fn read_socket_input(socket: &UdpSocket, buf: &mut [u8], server_ip: SocketAddr) -> Option<TaggedBytesMut> {
    match socket.recv_from(buf) {
        Ok((n, peer_addr)) => {
            return Some(TaggedBytesMut {
                now: Instant::now(),
                transport: TransportContext {
                    local_addr: server_ip,
                    peer_addr,
                    ecn: None,
                },
                message: BytesMut::from(&buf[..n]),
            });
        }

        Err(e) => match e.kind() {
            // Expected error for set_read_timeout(). One for windows, one for the rest.
            ErrorKind::WouldBlock | ErrorKind::TimedOut => None,
            _ => panic!("UdpSocket read failed: {e:?}"),
        },
    }
}

fn build_pipeline(
    local_addr: SocketAddr,
    server_states: Rc<RefCell<ServerStates>>,
) -> Rc<Pipeline<TaggedBytesMut, TaggedBytesMut>> {
    let pipeline: Pipeline<TaggedBytesMut, TaggedBytesMut> = Pipeline::new();

    let demuxer_handler = DemuxerHandler::new();
    let stun_handler = StunHandler::new();
    // DTLS
    let dtls_handler = DtlsHandler::new(local_addr, Rc::clone(&server_states));
    let sctp_handler = SctpHandler::new(local_addr, Rc::clone(&server_states));
    let data_channel_handler = DataChannelHandler::new();
    // SRTP
    let srtp_handler = SrtpHandler::new(Rc::clone(&server_states));
    let interceptor_handler = InterceptorHandler::new(Rc::clone(&server_states));
    // Gateway
    let gateway_handler = GatewayHandler::new(Rc::clone(&server_states));
    let exception_handler = ExceptionHandler::new();

    pipeline.add_back(demuxer_handler);
    pipeline.add_back(stun_handler);
    // DTLS
    pipeline.add_back(dtls_handler);
    pipeline.add_back(sctp_handler);
    pipeline.add_back(data_channel_handler);
    // SRTP
    pipeline.add_back(srtp_handler);
    pipeline.add_back(interceptor_handler);
    // Gateway
    pipeline.add_back(gateway_handler);
    pipeline.add_back(exception_handler);

    pipeline.finalize()
}
