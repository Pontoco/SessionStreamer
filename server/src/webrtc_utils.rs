use std::{ops::Deref, sync::Arc};

use tokio::sync::{
    broadcast,
    mpsc::{self},
    oneshot::{self},
    watch,
};
use tokio_stream::wrappers::{BroadcastStream, UnboundedReceiverStream};
use tracing::{Instrument, Span, debug, info, info_span};
use webrtc::{
    data_channel::{RTCDataChannel, data_channel_message::DataChannelMessage, data_channel_state::RTCDataChannelState},
    interceptor,
    interceptor::Attributes,
    peer_connection::{
        RTCPeerConnection, offer_answer_options::RTCAnswerOptions, peer_connection_state::RTCPeerConnectionState,
        sdp::session_description::RTCSessionDescription,
    },
    rtp::packet::Packet,
    track::track_remote::TrackRemote,
};

pub struct StatefulPeerConnection {
    peer: StatefulPeerConnectionMut,
}

pub struct StatefulPeerConnectionMut {
    peer: RTCPeerConnection,
    pub state: watch::Receiver<RTCPeerConnectionState>,
}

impl Deref for StatefulPeerConnection {
    type Target = StatefulPeerConnectionMut;

    fn deref(&self) -> &Self::Target {
        &self.peer
    }
}

impl StatefulPeerConnectionMut {
    pub async fn set_local_description(&mut self, description: RTCSessionDescription) -> Result<(), webrtc::Error> {
        self.peer.set_local_description(description).await
    }

    pub async fn set_remote_description(&mut self, description: RTCSessionDescription) -> Result<(), webrtc::Error> {
        self.peer.set_remote_description(description).await
    }

    pub async fn create_answer(&self, options: Option<RTCAnswerOptions>) -> Result<RTCSessionDescription, webrtc::Error> {
        self.peer.create_answer(options).await
    }

    pub async fn local_description(&self) -> Option<RTCSessionDescription> {
        self.peer.local_description().await
    }

    pub async fn on_gathering_complete(&self) -> mpsc::Receiver<()> {
        self.peer.gathering_complete_promise().await
    }

    /// Requests that the PeerConnection closes from this end.
    pub async fn close(&self) -> Result<(), webrtc::Error> {
        self.peer.close().await
    }
}

// pub struct StatefulPeerConnectionConnected {
//     peer: RTCPeerConnection,
//     data_channels: UnboundedReceiverStream<StatefulDataChannel>,
//     tracks: UnboundedReceiverStream<TrackRemote>
// }

impl StatefulPeerConnection {
    pub fn new(peer: RTCPeerConnection) -> StatefulPeerConnection {
        let (tx_conn_state, rx_conn_state) = watch::channel(peer.connection_state());

        let span = Span::current().clone();
        peer.dtls_transport().on_state_change(Box::new(move |ice| {
            let _span = span.clone().entered();
            info!("!!!!!!!!!!! DTLS Connection State: {}", ice);
            info!("!!!!!!!!!!! DTLS Connection State: {}", ice);
            info!("!!!!!!!!!!! DTLS Connection State: {}", ice);
            info!("!!!!!!!!!!! DTLS Connection State: {}", ice);
            Box::pin(async move {})
        }));

        peer.on_peer_connection_state_change(Box::new(move |peer_state| {
            info!("RTCPeerConnection state updated: {}", peer_state);
            let _ = tx_conn_state.send(peer_state);
            Box::pin(async move {})
        }));

        StatefulPeerConnection {
            peer: StatefulPeerConnectionMut {
                peer: peer,
                state: rx_conn_state,
            },
        }
    }

    pub fn get_channels(
        self,
    ) -> (
        StatefulPeerConnectionMut,
        UnboundedReceiverStream<StatefulDataChannel>,
        UnboundedReceiverStream<StatefulTrack>,
    ) {
        let (tx_channels, rx_channels) = mpsc::unbounded_channel();

        let span = Span::current().clone();
        self.peer.peer.on_data_channel(Box::new(move |channel| {
            let _span = span.clone().entered();

            // Ignore: Channel receiver dropped, so user doesn't want to hear about more channels.
            let _ = tx_channels.send(StatefulDataChannel::new(channel));
            Box::pin(async move {})
        }));

        let (tx_tracks, rx_tracks) = mpsc::unbounded_channel();

        let peer_conn_state_watch = self.peer.state.clone();
        let span = Span::current().clone();
        self.peer.peer.on_track(Box::new(move |remote_track, _, _| {
            let _span = span.clone().entered();

            info!("New remote track received: {} {}", remote_track.id(), remote_track.stream_id());

            let stateful_track = StatefulTrack::new(remote_track, peer_conn_state_watch.clone());
            let _ = tx_tracks
                .send(stateful_track)
                .map_err(|_| info!("Track receiver dropped, cannot send new StatefulTrack."));

            Box::pin(async move {})
        }));

        return (
            self.peer,
            UnboundedReceiverStream::new(rx_channels),
            UnboundedReceiverStream::new(rx_tracks),
        );
    }
}

// Should not be clone because we don't want a bunch of receivers made.
// Should only be one of these. Stick it in an arc if you must.
pub struct StatefulDataChannel {
    data_channel: DataChannelMut,
    messages: UnboundedReceiverStream<DataChannelMessage>,
    on_open: oneshot::Receiver<()>,
}

impl Deref for StatefulDataChannel {
    type Target = DataChannelMut;

    fn deref(&self) -> &Self::Target {
        &self.data_channel
    }
}

pub struct DataChannelMut {
    channel: Arc<RTCDataChannel>,
    errors: broadcast::Sender<Arc<webrtc::Error>>,
}

impl DataChannelMut {
    pub async fn send_text<M: Into<String>>(&self, msg: M) -> Result<usize, webrtc::Error> {
        self.channel.send_text(msg).await
    }

    /// Returns the label of the data channel.
    pub fn label(&self) -> &str {
        self.channel.label()
    }

    /// Returns the ready state of the data channel.
    pub fn ready_state(&self) -> RTCDataChannelState {
        self.channel.ready_state()
    }

    pub async fn close(self) -> Result<(), webrtc::Error> {
        self.channel.close().await
    }
}

impl StatefulDataChannel {
    pub fn new(data_channel: Arc<RTCDataChannel>) -> StatefulDataChannel {
        let (tx_open, rx_open) = tokio::sync::oneshot::channel();

        info!("Register onopen");
        let span = Span::current().clone();
        data_channel.on_open(Box::new(move || {
            let _span = span.entered();
            info!("ONOPEN");
            // Receiver was dropped. This means that the statefuldatachannel was dropped too.
            let _ = tx_open.send(());
            Box::pin(async {})
        }));

        // Register singular on_message handler so we can multi-plex.
        // webrtc-rs doesn't support multiple on_message handlers.
        let (tx, rx) = mpsc::unbounded_channel();

        let sender = tx.clone();
        let span = Span::current().clone();
        data_channel.on_message(Box::new(move |msg| {
            let _span = span.clone().entered();
            info!("got msg");
            // Ignore error: If the receiver has hung up, the stream was dropped,
            // so the messages are no longer requested. Nothing to do here.
            let _ = sender.send(msg);
            Box::pin(async move {})
        }));

        // Errors that overflow this queue will block within the tokio task that executes the
        // on_error callback. We don't expect a ton of errors, so this queue should be small.
        let (tx_err, _) = broadcast::channel(32);

        let sender = tx_err.clone();
        let span = Span::current().clone();
        data_channel.on_error(Box::new(move |error| {
            let _span = span.clone().entered();
            // Ignored: If the receiver has hung up, the stream was dropped,
            // so the messages are no longer requested. Nothing to do here.
            let _ = sender.send(Arc::new(error));
            Box::pin(async {})
        }));

        // Note: If the RtcDataChannel is properly dropped when the channel closes, then there's no
        // need to manually close the stream. The receiver will simply be dropped, because it is owned
        // by the on_open closure above, and the channel (and thus the stream) will close.
        //
        // However, I have my suspicions about webrtc-rs, so I would not be shocked if the data channel
        // is left resident, and then we'll need to use a oneshot or some other sync structure to close
        // the receiver manually when the data channel on_close happens.

        StatefulDataChannel {
            data_channel: DataChannelMut {
                channel: data_channel,
                errors: tx_err,
            },
            on_open: rx_open,
            messages: UnboundedReceiverStream::new(rx),
        }
    }

    /// Returns a Stream of messages for every data channel that is opened.
    /// This stream will send messages as long as the data channel is open, until the channel is closed
    ///
    /// Note: Even if the data channel is closed, the stream may continue to send messages if earlier
    /// messages were buffered and not yet received.
    /// - consumes because only one logical open can happen, and returns unique handle to message stream.
    /// - could also return the data channel.. for sending?
    pub async fn on_open(self) -> (DataChannelMut, UnboundedReceiverStream<DataChannelMessage>) {
        if let Err(_) = self.on_open.await {
            // todo: We can get rid of this panic by using a more ergonomic API all the way up to PeerConnection which guarantees no access to the RtcDataChannel.
            panic!(
                "on_open sender was dropped before the channel opened. This should only happen if on_open() is overwritten on the underlying RtcDataChannel."
            )
        }

        (self.data_channel, self.messages)
    }

    // /// Returns a Stream of messages received on this data channel (from the moment this is called)
    // /// ah fuck we could miss messages..
    // pub fn messages(&mut self) -> &mut UnboundedReceiverStream<(T, Arc<RTCDataChannel>, DataChannelMessage)> {
    //     &mut self.messages
    // }

    pub fn errors(&mut self) -> BroadcastStream<Arc<webrtc::Error>> {
        BroadcastStream::new(self.errors.subscribe())
    }

    // pub fn on_close<F, Fut>(&self, mut handler: F)
    // where
    //     F: FnMut(T, Arc<RTCDataChannel>) -> Fut + Send + Sync + 'static,
    //     Fut: Future<Output = ()> + Send + 'static,
    // {
    //     let span = Span::current();
    //     let state = self.state.clone();
    //     let channel = self.data_channel.clone();
    //     self.data_channel.on_close(Box::new(move || {
    //         let fut = handler(state.clone(), channel.clone()).instrument(span.clone());
    //         Box::pin(async move { fut.await })
    //     }));
    // }
}

pub struct StatefulTrack {
    track: Arc<TrackRemote>,
    rtp_rx: mpsc::UnboundedReceiver<Result<(Packet, Attributes), webrtc::Error>>,
}

impl StatefulTrack {
    pub fn new(track: Arc<TrackRemote>, mut peer_conn_state_rx: watch::Receiver<RTCPeerConnectionState>) -> Self {
        let (rtp_tx, rtp_rx) = mpsc::unbounded_channel();

        let track_clone = track.clone();
        // Clone id for logging, as the task might outlive the track reference if it's moved early
        let track_id_for_log = track.id().to_string();

        // Placed here so the formatter doesn't get confused by the macro.
        let buf_closed_error = webrtc::Error::Interceptor(interceptor::Error::Srtp(webrtc::srtp::Error::Util(
            webrtc::util::Error::ErrBufferClosed,
        )));

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    // biased; 

                    _ = peer_conn_state_rx.changed() => {
                        let state = *peer_conn_state_rx.borrow();
                        if matches!(state, RTCPeerConnectionState::Closed | RTCPeerConnectionState::Disconnected | RTCPeerConnectionState::Failed) {
                            info!("Peer connection state is {:?}, stopping RTP stream for track {}", state, track_id_for_log);
                            break; // This will drop rtp_tx, closing the channel.
                        }
                    }

                    result = track_clone.read_rtp() => {
                        match result {
                            Ok(packet_attribute_pair) => {
                                if rtp_tx.send(Ok(packet_attribute_pair)).is_err() {
                                    // Receiver (rtp_rx) was dropped by the consumer.
                                    info!("RTP receiver for track {} dropped by consumer, stopping stream.", track_id_for_log);
                                    break;
                                }
                            }
                            Err(e) => {
                                if e == buf_closed_error || e == webrtc::Error::ErrClosedPipe {
                                    debug!("RTP buffer closed. {}", track_id_for_log);
                                    break;
                                }

                                // Unknown error.
                                // Pass the error along. If this fails, the user no longer cares because they dropped the stream.
                                let _ = rtp_tx.send(Err(e));
                                break;
                            }
                        }
                    }
                }
            }

            // Sender should drop, closing stream & channel.

            info!("RTP reading task for track {} concluded.", track_id_for_log);
        }.instrument(info_span!("stateful_track", track_id = %track.id())));

        Self { track, rtp_rx }
    }

    pub fn track(&self) -> &Arc<TrackRemote> {
        &self.track
    }

    pub fn id(&self) -> String {
        self.track.id()
    }

    pub fn kind(&self) -> webrtc::rtp_transceiver::rtp_codec::RTPCodecType {
        self.track.kind()
    }

    pub fn codec(&self) -> webrtc::rtp_transceiver::rtp_codec::RTCRtpCodecParameters {
        self.track.codec()
    }

    pub fn into_rtp_stream(self) -> UnboundedReceiverStream<Result<(Packet, Attributes), webrtc::Error>> {
        UnboundedReceiverStream::new(self.rtp_rx)
    }
}
