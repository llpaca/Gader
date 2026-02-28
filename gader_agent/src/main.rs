use bollard::{API_DEFAULT_VERSION, Docker, query_parameters::LogsOptionsBuilder};
use futures::StreamExt;
use gader_agent::parsers::{LogEntry, LogParser, immich, vaultwarden};
use tokio::{
    sync::mpsc::channel,
    time::{self, Duration},
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let docker_connection =
        Docker::connect_with_http("http://127.0.0.1:2375", 5, API_DEFAULT_VERSION)
            .expect("Unable to connect to docker");

    // services to watch out for
    // immich and vaultwarden

    let params = LogsOptionsBuilder::new()
        .follow(true)
        .stderr(true)
        .stdout(true)
        .tail("30")
        .build();

    let (tx, mut rx) = channel::<LogEntry>(500);

    // clones are cheap here
    let tx_immich = tx.clone();
    let docker_immich = docker_connection.clone();
    let params_immich = params.clone();

    let _task_immich = tokio::spawn(async move {
        let mut immich_logs = docker_immich.logs("immich_server", Some(params_immich));

        let immich_parser = immich::ImmichParser::new();

        while let Some(res) = immich_logs.next().await {
            if let Ok(log_output) = res {
                let raw_log = log_output.to_string();

                if let Some(log) = immich_parser.parse(&raw_log) {
                    //println!("{:?}", log);
                    let _ = tx_immich.send(log).await;
                }
            }
        }
    });

    let tx_vw = tx.clone();
    let docker_vw = docker_connection.clone();
    let params_vw = params.clone();

    let _task_vw = tokio::spawn(async move {
        let mut vw_logs = docker_vw.logs("vaultwarden", Some(params_vw));

        let vw_parser = vaultwarden::VWParser::new();

        while let Some(res) = vw_logs.next().await {
            if let Ok(log_output) = res {
                let raw_log = log_output.to_string();

                // println!("{:?}", raw_log);

                if let Some(log) = vw_parser.parse(&raw_log) {
                    //println!("{:?}", log);
                    let _ = tx_vw.send(log).await;
                }
            }
        }
    });

    drop(tx);

    let mut batch: Vec<LogEntry> = Vec::with_capacity(10);
    let mut interval = time::interval(Duration::from_secs(5));

    loop { // consumer loop
        tokio::select! {
            Some(entry) = rx.recv() => {
                batch.push(entry);
                if batch.len() >= 10 {
                    println!("--- BATCH FULL ({} items) ---", batch.len());
                    for log in &batch {
                        println!(">> {}", log);
                    }
                    batch.clear();
                }
            }

            _ = interval.tick() => {
                if !batch.is_empty() {
                    println!("--- TIMEOUT FLUSH ({} items) ---", batch.len());
                    for log in &batch {
                        println!(">> {}", log);
                    }
                    batch.clear();
                }
            }

            else => break,
        }
    }

    Ok(())
}
