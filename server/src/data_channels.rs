use anyhow::anyhow;
use futures::StreamExt;
use tokio::{fs::File, io::AsyncWriteExt};
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, Span, debug, error, info, trace};

use crate::{
    path_ext::PathExt, timestamped_bytes::TimestampedUtf8Depacketizer, webrtc_utils::StatefulDataChannel, ClientMessage, LineTrackingError, SessionState
};

pub async fn handle_data_channel(
    state: SessionState,
    mut channel: StatefulDataChannel,
    graceful_shutdown: CancellationToken,
) -> Result<(), LineTrackingError> {
    info!(ready_state=?channel.ready_state(), protocol=?channel.protocol(), "Data channel connected: {}", &channel.label());

    let label = channel.label().to_string();
    let errors = channel.errors();

    // Log any errors the data channel encounters.
    let err_label = label.clone();
    tokio::spawn(
        async move {
            errors
                .filter_map(async |val| val.map_err(|err| error!("{}", err)).ok())
                .for_each(move |error| {
                    let err_label = err_label.clone();
                    async move {
                        error!("Data channel [{}] had error: {}", &err_label, error);
                    }
                })
                .instrument(Span::current())
        }
        .instrument(Span::current()),
    );

    // Wait for the 'general' data channel to connect and receive messages on the other end of the above channel.
    match label.as_ref() {
        // The 'general' data channel is used for signaling between the client / server.
        // Note: general stays open during graceful shutdown!
        "general" => handle_general_channel(state, channel, graceful_shutdown).await,
        // Other data channels are dumped directly to disk by id.
        _ => handle_generic_channel(state, channel, graceful_shutdown).await,
    }
}

async fn handle_general_channel(
    state: SessionState,
    mut channel: StatefulDataChannel,
    graceful_shutdown: CancellationToken,
) -> Result<(), LineTrackingError> {
    let label = channel.label().to_string();
    let (opened, mut messages) = channel.on_open().await;

    // spawn a new task and send stuff
    // Our primary 'send stuff to the client' channel.
    let state2 = state.clone();
    tokio::spawn(
        async move {
            // Take the receiving end of the client send.
            if let Some(mut rx) = state.client_send_rx.lock().await.take() {
                while let Some(data) = rx.recv().await {
                    debug!("Sending message to client [{:?}]", &data);
                    match serde_json::to_string(&data) {
                        Ok(json) => match opened.send_text(json).await {
                            Ok(_) => {
                                if let Err(err) = state.messages_sink.send(format!("ToClient: {:?}", &data)) {
                                    error!("Failed to send to message log: {:?} {err}", &data);
                                }
                            }
                            Err(err) => match err {
                                webrtc::Error::ErrClosedPipe => {
                                    debug!("Datachannel closed. Skipping sending messages to client.");
                                }
                                _ => error!("Error sending message to client [{}]", err),
                            },
                        },
                        Err(err) => {
                            error!("Failed to serialize data message. {} {:?}", err, data)
                        }
                    }
                }
            } else {
                error!("Data channel already initialized [{}]", label);
            }

            info!("Closed client sending channel.");
        }
        .instrument(Span::current()),
    );

    // Handle messages from the client.
    while let Some(msg) = graceful_shutdown.run_until_cancelled(messages.next()).await.flatten() {
        match serde_json::from_slice::<ClientMessage>(&msg.data) {
            Err(e) => {
                error!("Client did not send valid ClientMessage JSON. [{}]", e)
            }
            Ok(msg) => {
                if let Err(err) = state2.messages_sink.send(format!("ToServer: {:?}", &msg)) {
                    error!("Failed to send to message log: {:?} {err}", &msg);
                }
                match msg {
                    ClientMessage::SessionEnding => {
                        info!("Client requested polite closing of session.");
                        graceful_shutdown.cancel();
                    }
                    ClientMessage::Info { message } => info!("Received message from client: [{message}]"),
                    ClientMessage::Error { message } => error!("Received error from client: [{message}]"),
                }
            }
        };
    }

    Ok(())
}

async fn handle_generic_channel(
    state: SessionState,
    mut channel: StatefulDataChannel,
    graceful_shutdown: CancellationToken,
) -> Result<(), LineTrackingError> {
    let protocol = channel.protocol().to_string();
    let label = channel.label().to_string();

    // Drain all the messages to disk!
    let (_, mut messages) = channel.on_open().await;

    let ext = if protocol == "timestamped_bytes" { "txt" } else { "dat" };

    let file_path = state.data_path.join_safe(format!("data_{label}.{ext}"));
    let mut file_sink = match File::create(&file_path).await {
        Ok(file) => file,
        Err(err) => {
            return Err(anyhow!("Couldn't create file writer for data channel: [{}] [{}]", label, err).into());
        }
    };

    info!("Created data channel file: [{:?}]", file_path);

    let mut byteStream = TimestampedUtf8Depacketizer::new();

    let mut msg_count = 0;
    while let Some(msg) = graceful_shutdown.run_until_cancelled(messages.next()).await.flatten() {
        trace!(size = msg.data.len(), msg.is_string, "Data channel [{}] received packet.", label);

        // Some simple stats logging.
        msg_count += 1;
        if msg_count % 100 == 0 {
            info!("Data channel [{}] has received {} packets total..", label, msg_count);
        }

        if protocol == "timestamped_bytes" {
            if msg.is_string {
                state
                    .send_client_info(format!(
                        "data channel with protocol [{protocol}] cannot send messages of type string."
                    ))
                    .await;
                return Err(anyhow::anyhow!("Invalid protocol.").into());
            }

            let lines = byteStream.write_packet(msg.data)?;

            for line in lines {
                if let Err(err) = file_sink.write_all(line.as_bytes()).await {
                    error!("Failed to write datachannel buffer to disk: [{}] [{err}]", label);
                }
                if let Err(err) = file_sink.write_all("\n".as_bytes()).await {
                    error!("Failed to write datachannel buffer to disk: [{}] [{err}]", label);
                }
            }
        } else {
            // Write data channel raw bytes to the file.
            if let Err(err) = file_sink.write_all(&msg.data).await {
                error!("Failed to write datachannel buffer to disk: [{}] [{err}]", label);
            }
        }
    }

    Ok(())
}
