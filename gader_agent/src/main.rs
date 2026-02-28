use bollard::{API_DEFAULT_VERSION, Docker, query_parameters::LogsOptionsBuilder};
use futures::{StreamExt};
use gader_agent::parsers::{LogParser, immich, vaultwarden};

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

    // clones are cheap here
    let docker_immich = docker_connection.clone();
    let params_immich = params.clone();

    let task_immich = tokio::spawn(async move {
        let mut immich_logs = docker_immich.logs("immich_server", Some(params_immich));

        let immich_parser = immich::ImmichParser::new();

        while let Some(res) = immich_logs.next().await {
            if let Ok(log_output) = res {
                let raw_log = log_output.to_string();

                if let Some(log) = immich_parser.parse(&raw_log) {
                    println!("{:?}", log);
                }
            }
        }
    });

    let docker_vw = docker_connection.clone();
    let params_vw = params.clone();

    let task_vw = tokio::spawn(async move {
        let mut vw_logs = docker_vw.logs("vaultwarden", Some(params_vw));

        let vw_parser = vaultwarden::VWParser::new();

        while let Some(res) = vw_logs.next().await {
            if let Ok(log_output) = res {
                let raw_log = log_output.to_string();

                // println!("{:?}", raw_log);

                if let Some(log) = vw_parser.parse(&raw_log) {
                    println!("{:?}", log);
                }
            }
        }
    });

    let _ = tokio::join!(task_immich, task_vw);

    Ok(())
}
