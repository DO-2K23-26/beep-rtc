use std::{collections::HashMap, sync::mpsc::SyncSender};

use actix_cors::Cors;
use actix_web::{web::Data, App, HttpServer, HttpResponse};
use tracing::info;
use tracing_actix_web::TracingLogger;

use crate::{
    signalling::signaling_controller::{handle_offer, health, leave},
    transport::handlers::SignalingMessage,
    middleware::verify_jwt::SayHi,
};
use crate::middleware::verify_jwt::verify_token;

pub async fn start(
    addr: &str,
    port: &str,
    media_port_thread_map: HashMap<u16, SyncSender<SignalingMessage>>,
    env: String,
) -> std::io::Result<()> {
    let addr = format!("{}:{}", addr, port);

    if env == "prod" {
        info!("Running in prod mode");
        return HttpServer::new(move || {
            let cors = Cors::default()
                .allow_any_origin()
                .allow_any_method()
                .allow_any_header()
                .max_age(3600);

            App::new()
                .wrap(cors)
                .wrap_fn(|req, srv| {
                    if verify_token(req) {
                        let fut = srv.call(req.clone());
                        Box::pin(async {
                            let res = fut.await?;
                            Ok(res)
                        })
                    } else {
                        Box::pin(async {
                            let res = HttpResponse::Unauthorized().body("Unauthorized access");
                            Ok(req.clone().into_response(res))
                        })
                    }
                })
                .app_data(Data::new(media_port_thread_map.clone()))
                .service(handle_offer)
                .service(health)
                .service(leave)
        })
        .bind(addr)?
        .run()
        .await;
    } else {
        let mut builder =
            openssl::ssl::SslAcceptor::mozilla_intermediate(openssl::ssl::SslMethod::tls())
                .unwrap();
        builder
            .set_private_key_file("certs/key.pem", openssl::ssl::SslFiletype::PEM)
            .unwrap();
        builder.set_certificate_chain_file("certs/cer.pem").unwrap();

        info!("Starting web server at {}", addr);
        return HttpServer::new(move || {
            let cors = Cors::default()
                .allow_any_origin()
                .allow_any_method()
                .allow_any_header()
                .max_age(3600);

            App::new()
                .wrap(cors)
                .wrap(SayHi)
                .app_data(Data::new(media_port_thread_map.clone()))
                .service(health)
                .service(handle_offer)
                .service(leave)
        })
        .bind_openssl(addr, builder)?
        .run()
        .await;
    }
}
