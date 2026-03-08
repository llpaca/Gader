use core::{
    default::Default,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    time::Duration,
};
use std::sync::Arc;

use anyhow::{Context, Result};
use bollard::{API_DEFAULT_VERSION, Docker, query_parameters::LogsOptionsBuilder};
use bytes::Bytes;
use futures::{SinkExt, StreamExt};
use gader_agent::{
    AppState, cert,
    parsers::{LogParser, immich, vaultwarden},
};
use gader_common::{LogEntry, NetworkPacket};
use quinn::{Endpoint, ServerConfig, TransportConfig};
use tokio::sync::broadcast;
use tokio_util::{
    codec::{FramedRead, FramedWrite, length_delimited::LengthDelimitedCodec},
    sync::CancellationToken,
};
use tracing::{debug, error, info};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

    let docker_conn = Docker::connect_with_http("http://127.0.0.1:2375", 5, API_DEFAULT_VERSION)
        .expect("Unable to connect to docker");

    let server_endpoint = get_connection_endpoint().context("Error in making endpoint")?;
    info!("got server connection endpoint");

    let state = Arc::new(AppState::default());

    let (tx, _) = broadcast::channel::<LogEntry>(1000);
    let c_token = CancellationToken::new();

    info!("spawning tasks for immich_server and vaultwarden containers");

    let tx_immich = tx.clone();
    let docker_immich = docker_conn.clone();
    let c_im = c_token.clone();
    let state_im = state.clone();
    let _task_immich = tokio::spawn(async move {
        let immich_parser = immich::ImmichParser::new();
        spawn_watcher(
            docker_immich,
            "immich_server",
            immich_parser,
            tx_immich,
            c_im,
            state_im,
        )
        .await;
    });

    let tx_vw = tx.clone();
    let docker_vw = docker_conn.clone();
    let c_vw = c_token.clone();
    let state_vw = state.clone();
    let _task_vw = tokio::spawn(async move {
        let vw_parser = vaultwarden::VWParser::new();
        spawn_watcher(docker_vw, "vaultwarden", vw_parser, tx_vw, c_vw, state_vw).await;
    });

    info!("Awaiting connections");
    loop {
        tokio::select! {
            Some(conn) = server_endpoint.accept() => {
                info!("Accepting a client");
                tokio::spawn(handle_client(conn, tx.clone(), c_token.clone(), state.clone()));
            }
            _ = tokio::signal::ctrl_c() => {
                info!("SIGINT received, cancelling tasks...");
                c_token.cancel();
                break;
            }
        }
    }

    // wait for all QUIC connections to send CONNECTION_CLOSE and drain cleanly
    server_endpoint.wait_idle().await;

    Ok(())
}

fn get_connection_endpoint() -> Result<Endpoint> {
    let (cert_chain, key_der) = cert::load_or_generate_keys();

    let mut crypto = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(cert_chain, key_der)
        .context("failed to build TLS config")?;

    crypto.alpn_protocols = vec![b"gader-v1".to_vec()];

    let quic_crypto = quinn::crypto::rustls::QuicServerConfig::try_from(crypto)?;

    let socket_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 23456);

    let mut transport_config = TransportConfig::default();
    transport_config.keep_alive_interval(Some(Duration::from_secs(20)));

    let mut quic_server_config = ServerConfig::with_crypto(Arc::new(quic_crypto));

    quic_server_config.transport_config(Arc::new(transport_config));

    let server_endpoint = Endpoint::server(quic_server_config, socket_addr)?;

    Ok(server_endpoint)
}

async fn spawn_watcher<P: LogParser>(
    docker: Docker,
    name: &str,
    parser: P,
    tx: broadcast::Sender<LogEntry>,
    c_token: CancellationToken,
    state: Arc<AppState>,
) {
    info!("Watching: {}", name);
    let params = LogsOptionsBuilder::new()
        .follow(true)
        .stderr(true)
        .stdout(true)
        .tail("30")
        .build();

    let mut stream = docker.logs(name, Some(params));

    loop {
        tokio::select! {
            recv_stream = stream.next() => {
                match recv_stream {
                    Some(Ok(log)) => {
                        debug!("{:?}", log);

                        if let Some(entry) = parser.parse(&log.to_string()) {
                            debug!("Receiving logs!");
                            state.add_log(entry.clone());
                            let _ = tx.send(entry);
                        }
                    }
                    Some(Err(e)) => error!("Docker stream error: {}", e),
                    _ => {}
                }
            }
            _ = c_token.cancelled() => {
                debug!("Watcher {} received cancel signal", name);
                break;
            }
        }
    }
}

async fn handle_client(
    conn: quinn::Incoming,
    tx: broadcast::Sender<LogEntry>,
    c_token: CancellationToken,
    state: Arc<AppState>,
) {
    let connection = match conn.await {
        Ok(c) => {
            debug!("Handshake successful");
            c
        }
        Err(e) => {
            error!("Handshake failed: {}", e);
            return;
        }
    };

    info!("Client connected: {}", connection.remote_address());

    let (send_stream, recv_stream) = match connection.accept_bi().await {
        Ok(s) => {
            info!("Received bi-stream");
            s
        }
        Err(e) => {
            error!("Failed to accept bi-stream: {}", e);
            return;
        }
    };

    let mut writer = FramedWrite::new(send_stream, LengthDelimitedCodec::new());
    let mut reader = FramedRead::new(recv_stream, LengthDelimitedCodec::new());

    let mut rx = tx.subscribe();
    let mut batch: Vec<LogEntry> = Vec::with_capacity(10);

    let mut filter: Option<String> = None;
    let mut flush_timer = tokio::time::interval(tokio::time::Duration::from_millis(500));

    let history = state.get_snapshot();
    if !history.is_empty() {
        let packet = NetworkPacket::Batch(history);
        if let Ok(data) = postcard::to_stdvec(&packet) {
            writer.send(Bytes::from(data)).await.ok();
        }
    }

    loop {
        tokio::select! {
            msg = rx.recv() => {
                match msg {
                    Ok(entry) => {
                        if let Some(ref svc) = filter
                            && !entry.service.eq_ignore_ascii_case(svc) {
                                continue;
                            }

                        batch.push(entry);

                        if batch.len() >= 10 {
                            let batch_to_send = std::mem::take(&mut batch);
                            let packet = NetworkPacket::Batch(batch_to_send);

                            debug!("Sending packet: {:#?}", packet);

                            if let Ok(data) = postcard::to_stdvec(&packet)
                                && writer.send(Bytes::from(data)).await.is_err() {
                                    break;
                                }
                        }
                    }
                    Err(e) => {
                        error!("Encountered Error: {:?}", e);
                    }
                }
            }

            packet_res = reader.next() => {
                match packet_res {
                    Some(Ok(bytes)) => {
                        if let Ok(packet) = postcard::from_bytes::<NetworkPacket>(&bytes)
                            && let NetworkPacket::UpdateFilter {
                                    service,
                                    ..
                                } = packet {
                                info!("Updating filter to: {:?}", service);
                                filter = service;
                            }
                    }
                    Some(Err(e)) => {
                        error!("Framing Error: {}", e);
                        break;
                    }
                    None => break,
                }
            }

            _ = c_token.cancelled() => {
                info!("Client handler shutting down -- received cancel signal");
                break;
            }

            _ = flush_timer.tick() => {

                if !batch.is_empty() {

                    let batch_to_send = std::mem::take(&mut batch);
                    let packet = NetworkPacket::Batch(batch_to_send);
                    debug!("Sending packet: {:#?}", packet);
                    if let Ok(data) = postcard::to_stdvec(&packet)
                        && writer.send(Bytes::from(data)).await.is_err() {
                            break;
                        }
                }
            }
        }
    }

    info!("Client disconnected!");
}
