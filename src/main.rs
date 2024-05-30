/**
 * @authors Mathias Durat <mathias.durat@etu.umontpellier.fr>, Tristan-Mihai Radulescu <tristan-mihai.radulescu@etu.umontpellier.fr>
 * @forked_from https://github.com/webrtc-rs/sfu (Rusty Rain <y@ngr.tc>)
 */
use std::{
    collections::HashMap,
    net::{IpAddr, UdpSocket},
    str::FromStr,
    sync::{mpsc, Arc},
};
use std::net::SocketAddr;

use actix_web::rt::signal;
use clap::{command, Parser};
use dtls::extension::extension_use_srtp::SrtpProtectionProfile;
use log::info;
use sfu::RTCCertificate;
use signalling::web_server;
use tracing::span;
use wg::WaitGroup;

use crate::transport::sync_run;

mod logging;
mod middleware;
mod signalling;
mod transport;

#[derive(Default, Debug, Clone, Copy, clap::ValueEnum)]
enum Level {
    ERROR,
    WARN,
    #[default]
    INFO,
    DEBUG,
    TRACE,
}

impl From<Level> for tracing::Level {
    fn from(level: Level) -> Self {
        match level {
            Level::ERROR => tracing::Level::ERROR,
            Level::WARN => tracing::Level::WARN,
            Level::INFO => tracing::Level::INFO,
            Level::DEBUG => tracing::Level::DEBUG,
            Level::TRACE => tracing::Level::TRACE,
        }
    }
}

#[derive(Parser)]
#[command(name = "Beep SFU Server")]
#[command(author = "Tristan-Mihai Radulescu <tristan-mihai.radulescu@etu.umontpellier.fr>")]
#[command(version = "0.1.0")]
#[command(about = "A SFU Server", long_about = None)]
struct Cli {
    #[arg(long, default_value_t = false)]
    dev: bool,
    #[arg(long, default_value_t = format!("127.0.0.1"))]
    host: String,
    #[arg(long, default_value_t = format!("127.0.0.1"))]
    ip_endpoint: String,
    #[arg(short, long, default_value_t = 8080)]
    signal_port: u16,
    #[arg(long, default_value_t = 3478)]
    media_port_min: u16,
    #[arg(long, default_value_t = 3479)]
    media_port_max: u16,

    #[arg(short, long, default_value_t = format!("prod"))]
    env: String,

    #[arg(short, long)]
    debug: bool,
    #[arg(short, long, default_value_t = Level::INFO)]
    #[clap(value_enum)]
    level: Level,
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let cli = Cli::parse();

    let guard = logging::init_logger(&cli.env).unwrap(); //better error handling needed
    let root = span!(tracing::Level::INFO, "main");

    let _enter = root.enter();
    tracing::info!("Starting Beep SFU Server");

    let host_addr= IpAddr::from_str(&*cli.host).map_err(|e| {
        tracing::error!("Failed to parse host address: {:?}", e);
        std::io::Error::new(std::io::ErrorKind::Other, "Failed to parse host address")
    })?;

    let ip_endpoint = IpAddr::from_str(&*cli.ip_endpoint).map_err(|e| {
        tracing::error!("Failed to parse host address: {:?}", e);
        std::io::Error::new(std::io::ErrorKind::Other, "Failed to parse host address")
    })?;

    let media_ports: Vec<u16> = (cli.media_port_min..=cli.media_port_max).collect();

    let (stop_tx, stop_rx) = crossbeam_channel::bounded::<()>(1);

    // ice_port -> worker
    let mut media_port_thread_map = HashMap::new();

    let key_pair = rcgen::KeyPair::generate(&rcgen::PKCS_ECDSA_P256_SHA256).map_err(|e| {
        tracing::error!("Failed to generate key pair: {:?}", e);
        std::io::Error::new(std::io::ErrorKind::Other, "Failed to generate key pair")
    })?;

    let certificates =
        vec![RTCCertificate::from_key_pair(key_pair)
            .map_err(|_| std::io::ErrorKind::InvalidInput)?];

    let dtls_handshake_config = Arc::new(
        dtls::config::ConfigBuilder::default()
            .with_certificates(
                certificates
                    .iter()
                    .map(|cert| cert.dtls_certificate.clone())
                    .collect(),
            )
            .with_srtp_protection_profiles(vec![SrtpProtectionProfile::Srtp_Aes128_Cm_Hmac_Sha1_80])
            .with_extended_master_secret(dtls::config::ExtendedMasterSecretType::Require)
            .build(false, None)
            .map_err(|e| {
                tracing::error!("Failed to build dtls handshake config: {:?}", e);
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "Failed to build dtls handshake config",
                )
            })?,
    );

    let sctp_endpoint_config = Arc::new(sctp::EndpointConfig::default());
    let sctp_server_config = Arc::new(sctp::ServerConfig::default());
    let server_config = Arc::new(
        sfu::ServerConfig::new(certificates)
            .with_dtls_handshake_config(dtls_handshake_config)
            .with_sctp_endpoint_config(sctp_endpoint_config)
            .with_sctp_server_config(sctp_server_config),
    );

    let wait_group = WaitGroup::new();

    info!("Starting media server with {} workers", media_ports.len());
    for port in media_ports {
        let worker = wait_group.add(1);
        let stop_rx = stop_rx.clone();
        let (signaling_tx, signaling_rx) = mpsc::channel();

        let socket = UdpSocket::bind(format!("{host_addr}:{port}")).map_err(|e| {
            tracing::error!("Failed to bind udp socket: {:?}", e);
            std::io::Error::new(std::io::ErrorKind::Other, "Failed to bind udp socket")
        })?;
        let socket_endpoint = SocketAddr::new(ip_endpoint, port);

        media_port_thread_map.insert(port, signaling_tx);
        let server_config = server_config.clone();

        std::thread::spawn(move || {
            //write sfu handler here
            let _span = span!(tracing::Level::INFO, "worker", port = port).entered();

            match sync_run(stop_rx, socket, signaling_rx, server_config, socket_endpoint) {
                Ok(_) => (),
                Err(e) => {
                    tracing::error!("Failed to run sfu: {:?}", e);
                }
            }
            _span.exit();
            worker.done();
        });
    }

    let signal_port = cli.signal_port;

    web_server::start(
        &host_addr.to_string(),
        &signal_port.to_string(),
        media_port_thread_map.clone(),
    )
    .await?;

    info!("Press Ctrl-C to stop");
    std::thread::spawn(move || {
        let _ = signal::ctrl_c();
        stop_tx.send(()).unwrap();
    });

    let _ = stop_rx.recv();
    info!("Wait for Signaling Sever and Media Server Gracefully Shutdown...");
    wait_group.wait();

    Ok(())
}
