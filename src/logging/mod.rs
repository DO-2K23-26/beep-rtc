use std::{
    fs::File,
    io::{BufRead, BufReader},
};

use chrono::DateTime;
use reqwest::Client;
use serde_json::json;
use tracing_log::LogTracer;
use tracing_subscriber::{fmt};

use crate::logging::log_type::Log;

static LOG_FILE: &str = "./var/beep-sfu/";

mod log_type;

pub fn init_logger(
    env: &str,
) -> Result<tracing_appender::non_blocking::WorkerGuard, Box<dyn std::error::Error>> {
    // if env == "prod" {
    //     let file_appender = tracing_appender::rolling::hourly(LOG_FILE, "beep-sfu.log");
    // 
    //     let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
    // 
    //     let subscriber = fmt()
    //         .json()
    //         .with_thread_names(true)
    //         .with_writer(non_blocking)
    //         .finish();
    // 
    //     //trace with json
    // 
    //     tracing::subscriber::set_global_default(subscriber)?;
    // 
    //     // tracing::subscriber::set_global_default(subscriber)?;
    // 
    //     actix_rt::spawn(async {
    //         loop {
    //             tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
    //             let _ =
    //                 match send_logs_to_loki("http://localhost:3100/loki/api/v1/push".to_string())
    //                     .await
    //                 {
    //                     Ok(_) => (),
    //                     Err(e) => {
    //                         println!("Failed to send logs to loki: {:?}", e)
    //                     }
    //                 };
    //         }
    //     });
    // 
    //     Ok(guard)
    // } else {
        LogTracer::init().expect("Failed to set logger");

        // let filter = EnvFilter::try_from_default_env().or_else(|_| EnvFilter::try_new("info"))?;

        let (non_blocking, guard) = tracing_appender::non_blocking(std::io::stdout());

        let subscriber = fmt()
            .with_max_level(tracing::Level::DEBUG)
            .with_thread_names(true)
            .with_writer(non_blocking)
            .finish();

        tracing::subscriber::set_global_default(subscriber)?;
        Ok(guard)
    // }
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
enum LogSource {
    SignallingServer,
    Main,
    Transport,
}

impl LogSource {
    fn from_str(s: &str) -> LogSource {
        match s {
            "signalling_server" => LogSource::SignallingServer,
            "main" => LogSource::Main,
            "transport" => LogSource::Transport,
            _ => LogSource::Main,
        }
    }
    fn to_str(&self) -> &str {
        match self {
            LogSource::SignallingServer => "signalling_server",
            LogSource::Main => "main",
            LogSource::Transport => "transport",
        }
    }
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct PushLog {
    timestamp: String,
    log: String,
    log_source: LogSource,
}

pub async fn send_logs_to_loki(loki_endpoint: String) -> Result<(), reqwest::Error> {
    let now = chrono::Utc::now();
    let log_time_format = now.format("%Y-%m-%d-%H");
    let log_file = format!("{}/{}.{}", LOG_FILE, "beep-sfu.log", log_time_format);
    let client = reqwest::Client::new();

    let file = match File::open(log_file) {
        Ok(file) => file,
        Err(e) => {
            tracing::error!("Failed to open log file: {:?}", e);
            return Ok(());
        }
    };

    let logs = BufReader::new(file)
        .lines()
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| {
            tracing::error!("Failed to read log file: {:?}", e);
        });

    let mut logs_parsed = Vec::<PushLog>::new();

    logs.iter().for_each(|log| {
        log.iter().for_each(|line| {
            let log_parsed: Log = serde_json::from_str(line).unwrap();
            let log_source: LogSource = match log_parsed.thread_name.as_str() {
                thread_name if thread_name.starts_with("actix") => LogSource::SignallingServer,
                "main" => LogSource::Main,
                thread_name if thread_name.starts_with("ThreadId") => LogSource::Transport,
                _ => LogSource::Main,
            };

            logs_parsed.push(PushLog {
                timestamp: log_parsed.timestamp,
                log: line.to_string(),
                log_source: log_source,
            });
        });
    });

    for log in logs_parsed {
        send_single_log(log, loki_endpoint.clone(), &client).await?;
    }

    Ok(())
}

async fn send_single_log(
    log: PushLog,
    loki_endpoint: String,
    client: &Client,
) -> Result<(), reqwest::Error> {
    let datetime = DateTime::parse_from_rfc3339(&log.timestamp).unwrap();

    let timestamp = datetime.timestamp_nanos_opt().unwrap();
    let payload = json!({
        "streams": [{
            "stream": {
                "application": "beep-sfu",
                "source": log.log_source.to_str()
            },
            "values": [
                [
                    timestamp.to_string(),
                    log.log
                ]
            ]
        }]
    });

    let payload_str = serde_json::to_string(&payload).unwrap();

    // println!("Payload: {:?}", payload_str);

    let response = client
        .post(loki_endpoint)
        .body(payload_str)
        .header("Content-Type", "application/json")
        .send()
        .await?;
    // println!("Content: {:?}", response.text().await?);

    Ok(())
}
