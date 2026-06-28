#![allow(clippy::collapsible_if, clippy::collapsible_match)]
use std::net::SocketAddr;

use anyhow::{Context, Result, bail};
use bytes::Bytes;
use clap::Parser;
use futures::{SinkExt, StreamExt};
use gader_common::{LogEntry, NetworkPacket};
use gader_tui::{
    app::{Action, App},
    config, get_endpoint, tui, ui,
};
use tokio::{fs::OpenOptions, io::AsyncWriteExt, sync::mpsc};
use tokio_util::codec::{FramedRead, FramedWrite, length_delimited::LengthDelimitedCodec};
use tracing::{debug, error, info};

#[derive(Parser)]
#[command(name = "Gader", version = "0.1", about = "Gader TUI log viewer")]
struct Args {
    #[arg(short, long, default_value = "127.0.0.1:23456")]
    server: SocketAddr,

    #[arg(long, default_value = "info")]
    log_level: String,

    #[arg(long, default_value_t = false)]
    log_flush: bool,

    #[arg(long, default_value = "gader_archive.log")]
    flush_path: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    let home = std::env::var("HOME").context("HOME env var not set")?;
    let log_dir = std::path::PathBuf::from(home).join(".gader");
    std::fs::create_dir_all(&log_dir).context("Failed to create ~/.gader")?;
    let file_appender = tracing_appender::rolling::never(&log_dir, "tui_logs");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);
    tracing_subscriber::fmt()
        .with_writer(non_blocking)
        .with_ansi(false)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(&args.log_level)),
        )
        .init();

    tui::install_panic_hook();
    let mut app = App::new();

    let client_secret = config::load_secret().context("Failed to load client secret")?;
    let endpoint = get_endpoint()?;

    let server_addr = args.server;

    let connection = endpoint
        .connect(server_addr, "localhost")?
        .await
        .context("Failed to connect to agent")?;

    info!("Connected to server at: {}", server_addr);

    let (send_stream, recv_stream) = connection
        .open_bi()
        .await
        .context("Failed to initiate bi-stream")?;

    debug!("Bi-directional stream successfully established");

    let mut terminal = tui::init_terminal()?;
    let mut key_reader = crossterm::event::EventStream::new();

    let mut writer = FramedWrite::new(send_stream, LengthDelimitedCodec::new());
    let mut reader = FramedRead::new(recv_stream, LengthDelimitedCodec::new());

    info!("Starting handshake");
    let handshake = NetworkPacket::Handshake {
        secret_token: client_secret,
    };

    writer
        .send(Bytes::from(postcard::to_stdvec(&handshake)?))
        .await?;

    match reader.next().await {
        Some(Ok(bytes)) => {
            if let Ok(NetworkPacket::HandshakeAck { accepted: true }) = postcard::from_bytes(&bytes)
            {
                info!("Handshake accepted! Starting TUI");
            } else {
                info!("CLIENT_SECRET rejected by server");
                bail!("Failed handshake");
            }
        }
        _ => {
            error!("Handshake with server failed! Connection Error");
            bail!("Handshake network error")
        }
    }

    let (disk_tx, mut disk_rx) = mpsc::channel::<LogEntry>(4096);

    if args.log_flush {
        let flush_path = args.flush_path.clone();
        tokio::spawn(async move {
            let mut file = match OpenOptions::new()
                .create(true)
                .append(true)
                .open(&flush_path)
                .await
            {
                Ok(f) => f,
                Err(e) => {
                    eprintln!("Failed to open flush file path: {:?}", e);
                    return;
                }
            };

            while let Some(entry) = disk_rx.recv().await {
                let log_line = format!("{}\n", entry);
                if let Err(e) = file.write_all(log_line.as_bytes()).await {
                    eprintln!("Error writing log to disk background worker: {:?}", e);
                    break;
                }
            }
        });
    }

    info!("Listening for logs...");

    while !app.should_quit {
        terminal.draw(|f| ui::view(f, &mut app))?;

        tokio::select! {
            Some(msg_res) = reader.next() => {
                if let Ok(bytes) = msg_res {
                     if let Ok(packet) = postcard::from_bytes(&bytes) {
                         if args.log_flush {
                             if let NetworkPacket::Batch(ref new_logs) = packet {
                                 for log in new_logs {
                                     // cheap pointer clone on the stack
                                     let _ = disk_tx.send(log.clone()).await;
                                 }
                             }
                         }
                         app.update(Action::Network(packet));
                     }
                }
            }

            Some(Ok(event)) = key_reader.next() => {
                match event {
                    crossterm::event::Event::Key(key) => {
                        if key.kind == crossterm::event::KeyEventKind::Press {
                            app.update(Action::Input(key.code));
                        }
                    }
                    crossterm::event::Event::Mouse(mouse) => {
                        match mouse.kind {
                            crossterm::event::MouseEventKind::ScrollUp => {
                                app.update(Action::Input(crossterm::event::KeyCode::Up));
                            }
                            crossterm::event::MouseEventKind::ScrollDown => {
                                app.update(Action::Input(crossterm::event::KeyCode::Down));
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
        }

        for packet in app.outbox.drain(..) {
            if let Ok(data) = postcard::to_stdvec(&packet) {
                writer.send(Bytes::from(data)).await.ok();
            }
        }
    }

    tui::restore_terminal()?;
    Ok(())
}
