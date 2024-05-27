use std::{collections::HashMap, sync::mpsc::Sender};

use actix_cors::Cors;
use actix_web::{web::Data, App, HttpServer};
use tracing::info;

use crate::{
    signalling::signaling_controller::{handle_offer, health, leave},
    transport::handlers::SignalingMessage,
    middleware::verify_jwt::verify_token,
};

pub async fn start(
    addr: &str,
    port: &str,
    media_port_thread_map: HashMap<u16, Sender<SignalingMessage>>,
) -> std::io::Result<()> {
    let addr = format!("{}:{}", addr, port);

    info!("Running in prod mode");
    return HttpServer::new(move || {
        let cors = Cors::default()
            .allow_any_origin()
            .allow_any_method()
            .allow_any_header()
            .max_age(3600);

        App::new()
            .wrap(cors)
            .app_data(Data::new(media_port_thread_map.clone()))
            .service(handle_offer)
            .service(health)
            .service(leave)
    })
    .bind(addr)?
    .run()
    .await;
}
