use std::{ops::Deref, sync::Arc};

use futures::{SinkExt, Stream};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tracing::{Instrument, Span, info_span};
use webrtc::{
    data_channel::{data_channel_message::DataChannelMessage, RTCDataChannel},
    peer_connection::{peer_connection_state::RTCPeerConnectionState, RTCPeerConnection},
    rtp_transceiver::{rtp_receiver::RTCRtpReceiver, RTCRtpTransceiver},
    track::track_remote::TrackRemote,
};

pub struct RTCPeerConnectionAsync {
    pub peer: RTCPeerConnection,
}

impl Deref for RTCPeerConnectionAsync {
    type Target = RTCPeerConnection;

    fn deref(&self) -> &Self::Target {
        &self.peer
    }
}

pub struct StatefulPeerConnection<T>
where
    T: Clone + Send + Sync + 'static,
{
    pub state: T,
    pub peer: RTCPeerConnection,
}

impl<T> Deref for StatefulPeerConnection<T>
where
    T: Clone + Send + Sync + 'static,
{
    type Target = RTCPeerConnection;

    fn deref(&self) -> &Self::Target {
        &self.peer
    }
}
impl<T> StatefulPeerConnection<T>
where
    T: Clone + Send + Sync + 'static,
{
    pub fn new(peer: RTCPeerConnection, state: T) -> StatefulPeerConnection<T> {
        StatefulPeerConnection {
            peer: peer,
            state: state,
        }
    }

    pub fn on_track<X, Fut>(&self, mut handler: X)
    where
        X: FnMut(T, Arc<TrackRemote>, Arc<RTCRtpReceiver>, Arc<RTCRtpTransceiver>) -> Fut
            + Send
            + Sync
            + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let span = Span::current();
        let state = self.state.clone();
        self.peer.on_track(Box::new(move |a, b, c| {
            let fut = handler(state.clone(), a, b, c).instrument(span.clone());
            Box::pin(async move { fut.await })
        }));
    }

    pub fn on_peer_connection_state_change<F, Fut>(&self, mut handler: F)
    where
        F: FnMut(T, RTCPeerConnectionState) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let span = Span::current();
        let state = self.state.clone();
        self.peer
            .on_peer_connection_state_change(Box::new(move |peer_state| {
                let fut = handler(state.clone(), peer_state).instrument(span.clone());
                Box::pin(async move { fut.await })
            }));
    }

    pub fn on_data_channel<F, Fut>(&self, mut handler: F)
    where
        F: FnMut(T, StatefulDataChannel<T>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let span = Span::current();
        let state = self.state.clone();
        self.peer.on_data_channel(Box::new(move |channel| {
            let fut = handler(
                state.clone(),
                StatefulDataChannel::new(channel, state.clone()),
            )
            .instrument(span.clone());
            Box::pin(async move { fut.await })
        }));
    }
}

#[derive(Clone)]
pub struct StatefulDataChannel<T>
where
    T: Clone + Send + Sync + 'static,
{
    pub state: T,
    pub data_channel: Arc<RTCDataChannel>,
}

impl<T> Deref for StatefulDataChannel<T>
where
    T: Clone + Send + Sync + 'static,
{
    type Target = RTCDataChannel;

    fn deref(&self) -> &Self::Target {
        &self.data_channel
    }
}

impl<T> StatefulDataChannel<T>
where
    T: Clone + Send + Sync + 'static,
{
    pub fn new(data_channel: Arc<RTCDataChannel>, state: T) -> StatefulDataChannel<T> {
        StatefulDataChannel {
            data_channel,
            state,
        }
    }

    pub fn on_open<F, Fut>(&self, handler: F)
    where
        F: FnOnce(T, Arc<RTCDataChannel>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let span = Span::current();
        let state = self.state.clone();
        let channel = self.data_channel.clone();
        self.data_channel.on_open(Box::new(move || {
            let fut = handler(state.clone(), channel).instrument(span.clone());
            Box::pin(async move { fut.await })
        }));
    }

    pub fn on_close<F, Fut>(&self, mut handler: F)
    where
        F: FnMut(T) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let span = Span::current();
        let state = self.state.clone();
        self.data_channel.on_close(Box::new(move || {
            let fut = handler(state.clone()).instrument(span.clone());
            Box::pin(async move { fut.await })
        }));
    }

    pub fn on_error<F, Fut>(&self, mut handler: F)
    where
        F: FnMut(T, webrtc::Error) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let span = Span::current();
        let state = self.state.clone();
        self.data_channel.on_error(Box::new(move |error| {
            let fut = handler(state.clone(), error).instrument(span.clone());
            Box::pin(async move { fut.await })
        }));
    }

    pub fn on_message<F, Fut>(&self, mut handler: F)
    where
        F: FnMut(T, DataChannelMessage) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let span = Span::current();
        let state = self.state.clone();
        self.data_channel.on_message(Box::new(move |error| {
            let fut = handler(state.clone(), error).instrument(span.clone());
            Box::pin(async move { fut.await })
        }));
    }
}
