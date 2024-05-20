use std::{
    fs::File,
    io::{BufRead, BufReader},
};

use serde_json::{json, Value};
use tracing_subscriber::{fmt, EnvFilter};

use crate::logging::log_type::Log;

static LOG_FILE: &str = "/var/log/beep-sfu/";

mod log_type;

pub fn init_logger(
    env: &str,
) -> Result<tracing_appender::non_blocking::WorkerGuard, Box<dyn std::error::Error>> {
    if env == "prod" {
        let file_appender = tracing_appender::rolling::hourly(LOG_FILE, "beep-sfu.log");

        let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

        let subscriber = fmt()
            .json()
            .with_thread_names(true)
            .with_writer(non_blocking)
            .finish();

        //trace with json

        tracing::subscriber::set_global_default(subscriber)?;

        // tracing::subscriber::set_global_default(subscriber)?;

        actix_rt::spawn(async {
            loop {
                println!("Logging to file: {}/beep-sfu.log", LOG_FILE);
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                let _ =
                    match send_logs_to_loki("http://localhost:3100/loki/api/v1/push".to_string())
                        .await
                    {
                        Ok(_) => (),
                        Err(e) => {
                            println!("Failed to send logs to loki: {:?}", e)
                        }
                    };
            }
        });

        Ok(guard)
    } else {
        let filter = EnvFilter::try_from_default_env().or_else(|_| EnvFilter::try_new("info"))?;

        let (non_blocking, guard) = tracing_appender::non_blocking(std::io::stdout());

        let subscriber = fmt()
            .with_thread_names(true)
            .with_env_filter(filter)
            .with_writer(non_blocking)
            .finish();

        tracing::subscriber::set_global_default(subscriber)?;
        Ok(guard)
    }
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct PushLog {
    level: String,
    message: String,
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
            logs_parsed.push(PushLog {
                level: log_parsed.level,
                message: log_parsed.fields.message,
            });
        });
    });

    let payload = json!({
        "streams": [{
            "stream": {
                "label": "rust_backend"
            },
            "values": logs_parsed,
        }]
    });

    let payload_str = serde_json::to_string(&payload).unwrap();

    println!("Payload: {:?}", payload_str);

    let response = client
        .post(loki_endpoint)
        .body(payload_str)
        .header("Content-Type", "application/json")
        .send()
        .await?;
    println!("Response: {:?}", response);
    println!("Content: {:?}", response.text().await?);

    Ok(())
}
