use std::{
    collections::HashMap,
    net::{IpAddr, UdpSocket},
    str::FromStr,
    sync::{mpsc, Arc},
};

use actix_web::rt::signal;
use clap::{command, Parser};
use dtls::extension::extension_use_srtp::SrtpProtectionProfile;
use log::info;
use sfu::RTCCertificate;
use signalling::{sync_run, web_server};
use wg::WaitGroup;

mod signalling;
/**
 * @authors Mathias Durat <mathias.durat@etu.umontpellier.fr>, Tristan-Mihai Radulescu <tristan-mihai.radulescu@etu.umontpellier.fr>
 * @forked_from https://github.com/webrtc-rs/sfu (Rusty Rain <y@ngr.tc>)
 */
mod util;

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
#[command(name = "SFU Server")]
#[command(author = "Tristan-Mihai Radulescu <tristan-mihai.radulescu@etu.umontpellier.fr>")]
#[command(version = "0.1.0")]
#[command(about = "An example of SFU Server", long_about = None)]
struct Cli {
    #[arg(long, default_value_t = false)]
    dev: bool,
    #[arg(long, default_value_t = format!("127.0.0.1"))]
    host: String,
    #[arg(short, long, default_value_t = 8080)]
    signal_port: u16,
    #[arg(long, default_value_t = 3478)]
    media_port_min: u16,
    #[arg(long, default_value_t = 3479)]
    media_port_max: u16,

    #[arg(short, long)]
    force_local_loop: bool,
    #[arg(short, long)]
    debug: bool,
    #[arg(short, long, default_value_t = Level::INFO)]
    #[clap(value_enum)]
    level: Level,
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();
    // let host_addr = if cli.host == "127.0.0.1" && !cli.force_local_loop {
    //     let addr = match util::net::select_host_address() {
    //         Ok(addr) => addr,
    //         Err(e) => {
    //             tracing::error!("Failed to select host address: {:?}", e);
    //             return Err(std::io::Error::new(
    //                 std::io::ErrorKind::Other,
    //                 "Failed to select host address",
    //             ));
    //         }
    //     };
    //     return Ok(addr);
    // } else {
    //     IpAddr::from_str(&cli.host).map_err(|e| {
    //         tracing::error!("Failed to parse host address: {:?}", e);
    //         std::io::Error::new(std::io::ErrorKind::Other, "Failed to parse host address")
    //     })?
    // };

    let host_addr: IpAddr;

    if cli.host == "127.0.0.1" && !cli.force_local_loop {
        host_addr = match util::net::select_host_address() {
            Ok(addr) => addr,
            Err(e) => {
                tracing::error!("Failed to select host address: {:?}", e);
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "Failed to select host address",
                ));
            }
        };
    } else {
        host_addr = IpAddr::from_str(&cli.host).map_err(|e| {
            tracing::error!("Failed to parse host address: {:?}", e);
            std::io::Error::new(std::io::ErrorKind::Other, "Failed to parse host address")
        })?;
    }

    let media_ports: Vec<u16> = (cli.media_port_min..=cli.media_port_max).collect();

    let (stop_tx, stop_rx) = crossbeam_channel::bounded::<()>(1);

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

    for port in media_ports {
        let worker = wait_group.add(1);
        let stop_rx = stop_rx.clone();
        let (signaling_tx, signaling_rx) = mpsc::sync_channel(1);

        let socket = UdpSocket::bind(format!("{host_addr}:{port}")).map_err(|e| {
            tracing::error!("Failed to bind udp socket: {:?}", e);
            std::io::Error::new(std::io::ErrorKind::Other, "Failed to bind udp socket")
        })?;

        media_port_thread_map.insert(port, signaling_tx);
        let server_config = server_config.clone();

        std::thread::spawn(move || {
            //write sfu handler here
            match sync_run(stop_rx, socket, signaling_rx, server_config) {
                Ok(_) => (),
                Err(e) => {
                    tracing::error!("Failed to run sfu: {:?}", e);
                }
            }
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
