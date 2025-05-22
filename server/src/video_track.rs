use anyhow::anyhow;
use futures::StreamExt;
use gstreamer::{ClockTime, MessageType, prelude::*};
use sdp::description::media::MediaDescription;
use sdp::description::session::SessionDescription;
use sdp::description::session::{ATTR_KEY_MID, ATTR_KEY_MSID, ATTR_KEY_RID, ATTR_KEY_SIMULCAST, ATTR_KEY_SSRC};
use std::collections::HashSet;
use std::io::Cursor;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, trace};
use webrtc::api::media_engine;
use webrtc::rtp::codecs::h264::H264Packet;
use webrtc::rtp::packetizer::Depacketizer;
use webrtc::rtp_transceiver::rtp_transceiver_direction::RTCRtpTransceiverDirection;
use webrtc::sdp::description::session;
use webrtc::{media, sdp};

use crate::webrtc_utils::StatefulTrack;
use crate::{LineTrackingError, ServerMessage, SessionState}; // sdp::direction::Direction in older versions, sdp::extmap::Direction in newer

/// Handles video tracks that the client sends to us.
pub async fn handle_track(
    track_id: i8,
    track: StatefulTrack,
    session_state: SessionState,
    graceful_shutdown: CancellationToken,
) -> Result<(), LineTrackingError> {
    info!(session_state.session_id, track_id = %track.id(), kind = ?track.kind(), "Track connected.");

    let codec = track.codec();
    let mime_type = codec.capability.mime_type.to_lowercase();
    info!(%mime_type, clock_rate = codec.capability.clock_rate, "Track codec details.");

    if !mime_type.as_str().eq_ignore_ascii_case(media_engine::MIME_TYPE_H264) {
        return Err(anyhow!("Video tracks must be in H264 format. [{}] id=[{}]", mime_type, track.id()).into());
    }

    // Initialize GStreamer
    gstreamer::log::remove_default_log_function(); // Removes printing to stdout.
    gstreamer::log::set_default_threshold(gstreamer::DebugLevel::Trace); // Sets default filtering on the gstreamer side.
    tracing_gstreamer::integrate_events(); // Merge gstreamer events into tracing.

    gstreamer::init()?;

    info!("Initializing GStreamer pipeline for track {track_id}.");

    // Build the GStreamer pipeline
    //  - Feed in raw NALU packets in H264 format.
    //  - Output to MP4
    let output_file = session_state
        .data_path
        .join(format!("game_capture_{track_id}.mp4"))
        .into_os_string()
        .into_string()
        .map_err(|e| anyhow!("Data path [{e:?}] provided for session streamer was not valid UTF8 string."))?;

    let pipeline_str = format!("appsrc name=appsrc ! h264parse ! mp4mux name=mux ! filesink location={output_file}",);

    info!("Starting pipeline [{}]", &pipeline_str);

    let pipeline = gstreamer::parse::launch(&pipeline_str)
        .unwrap()
        .downcast::<gstreamer::Pipeline>()
        .unwrap();

    // Get the appsrc element
    let appsrc = pipeline.by_name("appsrc").unwrap().downcast::<gstreamer_app::AppSrc>().unwrap();

    // Configure the appsrc
    appsrc.set_stream_type(gstreamer_app::AppStreamType::Stream);

    let rtp_caps = gstreamer::Caps::builder("video/x-h264")
        .field("stream-format", "byte-stream")
        .build();

    appsrc.set_caps(Some(&rtp_caps));
    appsrc.set_format(gstreamer::Format::Time); // Says that our buffers will have timestamps.
    appsrc.set_is_live(true); // A flag telling downstream pads that we are optimizing for live streaming content.

    // We manually reconstruct the timestamps from received RTP packets. See below.
    appsrc.set_do_timestamp(false);

    // Start the pipeline
    pipeline.set_state(gstreamer::State::Playing)?;

    info!("Started gstreamer pipeline: {:?}", pipeline);

    session_state
        .client_send_tx
        .send(ServerMessage::Info {
            message: "Video stream connected. Streaming begin.".into(),
        })
        .await?;

    // Debugs the NAL units we've received.
    // let mut reader = AnnexBReader::accumulate(|nal: RefNal<'_>| {
    //     let nal_unit_type = nal.header().unwrap().nal_unit_type();
    //     // info!("H264: Saw start of {:?}", nal_unit_type);
    //     if nal.is_complete() {
    //         let mut data = vec![];
    //         nal.reader().read_to_end(&mut data).unwrap();
    //         // info!(
    //         //     "Server: Has NAL Unit: {:?} of size: {}",
    //         //     nal_unit_type,
    //         //     data.len()
    //         // );
    //     }
    //     NalInterest::Buffer
    // });

    let mut depacketizer = H264Packet::default();
    let mut rtp_packets = 0;

    let id = track.id();
    let mut packet_stream = track.into_rtp_stream();
    while let Some(data) = graceful_shutdown.run_until_cancelled(packet_stream.next()).await.flatten() {
        match data {
            Ok((packet, _)) => {
                rtp_packets += 1;
                trace!(
                    "Got rtp packet with sequence number {}, timestamp {}, and payload length {}",
                    packet.header.sequence_number,
                    packet.header.timestamp,
                    packet.payload.len()
                );

                if rtp_packets % 1000 == 0 {
                    info!("Video track [{}] has received {} packets total..", id, rtp_packets);
                }

                if packet.payload.len() == 0 {
                    trace!("Received RTP packet of size 0. Skipping.");
                    continue;
                }

                let h264_bytes = depacketizer.depacketize(&packet.payload)?;

                if h264_bytes.len() > 0 {
                    let mut buffer = gstreamer::Buffer::from_slice(h264_bytes);

                    // Our RTP stream timestamps are given in a specific clock rate, passed in above.
                    // That rate is in Hz, ie 90000Hz, so we convert to nanoseconds.
                    let timestamp_nanos = ClockTime::from_nseconds(
                        (packet.header.timestamp as u64 * ClockTime::SECOND.nseconds()) / codec.capability.clock_rate as u64,
                    );

                    buffer.get_mut().unwrap().set_pts(Some(timestamp_nanos));
                    buffer.get_mut().unwrap().set_dts(Some(timestamp_nanos));
                    appsrc.push_buffer(buffer).expect("Failed to push buffer");
                }
            }
            Err(err) => {
                error!("Error when reading RTP stream for video. [{:?}]", err);
                break;
            }
        };
    }

    info!("Finished receiving RTP stream. Received {rtp_packets} packets.");

    session_state
        .client_send_tx
        .send(ServerMessage::Info {
            message: "Video stream closed.".into(),
        })
        .await?;

    // Say we're done pushing data.
    info!("Sending EOS to GStreamer pipeline.");
    appsrc.end_of_stream()?;

    // todo: Technically we should wait for the pipeline to finish!
    // However, I haven't been able to get this to trigger. The filesink pad doesn't seem to want to close and emit the final EOS on the bus.
    info!("Waiting for bus to finish..");
    let bus = pipeline.bus().expect("Failed to get pipeline bus");
    bus.timed_pop_filtered(None, &[MessageType::Eos, MessageType::Error]);

    // Cleanup resources.
    pipeline.set_state(gstreamer::State::Null).unwrap();

    info!("Finished receiving video track. Saved to {output_file}");

    Ok(())
}

/// Parses an SDP offer string and determines the set of expected incoming video tracks.
/// Secondarily returns a list of notices that we can return or print. Notices are not fatal errors, but they
/// are recoverable errors for where the given media track's attributes are invalid.
pub fn determine_supported_video_tracks(session_description: &SessionDescription) -> (Vec<MediaDescription>, Vec<ServerMessage>) {
    let mut expected_tracks = Vec::new();
    let mut notices: Vec<ServerMessage> = vec![];

    for media_desc in session_description.media_descriptions.iter() {
        // 1. Identify Video Media Sections
        let track_type = media_desc.media_name.media.to_lowercase();
        if track_type != "video" {
            continue;
        }

        // 2. Check if the track is active (port != 0)
        if media_desc.media_name.port.value == 0 {
            // Port 0 indicates the track is disabled for this media section
            notices.push(ServerMessage::notice("Media track is disabled (port == 0). We don't support this."));
            continue;
        }

        // 3. Determine Direction
        // We need to infer effective direction.
        // `webrtc_rs::peer_connection::sdp::get_peer_direction` would be ideal here.
        // For simplicity, we'll check common attributes.
        // This logic might need to be more robust based on `webrtc-rs`'s exact interpretation.
        let peer_direction = get_peer_direction(&media_desc);
        match peer_direction {
            RTCRtpTransceiverDirection::Sendrecv => notices.push(ServerMessage::notice(
                "Media track direction is set to [sendrecv]. We only support [send | (undefined)]",
            )),
            RTCRtpTransceiverDirection::Inactive => notices.push(ServerMessage::notice(
                "Media track direction is set to [inactive]. We only support [send | (undefined)]",
            )),
            RTCRtpTransceiverDirection::Recvonly => notices.push(ServerMessage::notice(
                "Media track direction is set to [recv]. We only support [send | (undefined)]",
            )),
            RTCRtpTransceiverDirection::Unspecified | RTCRtpTransceiverDirection::Sendonly => {}
        }

        expected_tracks.push(media_desc);
    }

    (expected_tracks.into_iter().cloned().collect(), notices)
}

fn get_peer_direction(media: &MediaDescription) -> RTCRtpTransceiverDirection {
    for a in &media.attributes {
        let direction = RTCRtpTransceiverDirection::from(a.key.as_str());
        if direction != RTCRtpTransceiverDirection::Unspecified {
            return direction;
        }
    }
    RTCRtpTransceiverDirection::Unspecified
}

#[cfg(test)]
mod tests {
    use std::{collections::HashSet, io::Cursor};

    use webrtc::{peer_connection::sdp::session_description::RTCSessionDescription, rtp_transceiver::rtp_transceiver_direction::RTCRtpTransceiverDirection, sdp::SessionDescription};

    use crate::ServerMessage;

    use super::determine_supported_video_tracks;

    // This one is straight out of unity's webrtc package:
    #[test]
    fn test_sample_unity_sdp() {
        let offer = "v=0
o=- 6648533537093923026 2 IN IP4 127.0.0.1
s=-
t=0 0
a=group:BUNDLE 0 1
a=extmap-allow-mixed
a=msid-semantic: WMS
m=video 63816 UDP/TLS/RTP/SAVPF 127 120 125 119 124 118 123 117 122 116 39 40 115 114 121
c=IN IP4 97.113.95.58
a=rtcp:9 IN IP4 0.0.0.0
a=candidate:2208109002 1 udp 2122194687 192.168.0.62 63816 typ host generation 0 network-id 1 network-cost 50
a=candidate:2977440256 1 udp 2122265343 fd7b:7f96:14ec:18fc:4a2:7b0b:9b0e:99c9 50087 typ host generation 0 network-id 2 network-cost 50
a=candidate:1486762379 1 udp 1685987071 97.113.95.58 63816 typ srflx raddr 192.168.0.62 rport 63816 generation 0 network-id 1 network-cost 50
a=candidate:4250071890 1 tcp 1518214911 192.168.0.62 54184 typ host tcptype passive generation 0 network-id 1 network-cost 50
a=candidate:3484926104 1 tcp 1518285567 fd7b:7f96:14ec:18fc:4a2:7b0b:9b0e:99c9 54185 typ host tcptype passive generation 0 network-id 2 network-cost 50
a=ice-ufrag:c/WI
a=ice-pwd:7PV5SNpanak/qTOvUN0mUKkz
a=ice-options:trickle
a=fingerprint:sha-256 D9:C2:E1:0A:69:DF:18:E8:FD:89:51:04:D5:B2:96:B2:A7:EC:23:A5:B3:49:E3:C6:F5:7A:A0:AF:CF:1A:5F:81
a=setup:actpass
a=mid:0
a=extmap:1 urn:ietf:params:rtp-hdrext:toffset
a=extmap:2 http://www.webrtc.org/experiments/rtp-hdrext/abs-send-time
a=extmap:3 urn:3gpp:video-orientation
a=extmap:4 http://www.ietf.org/id/draft-holmer-rmcat-transport-wide-cc-extensions-01
a=extmap:5 http://www.webrtc.org/experiments/rtp-hdrext/playout-delay
a=extmap:6 http://www.webrtc.org/experiments/rtp-hdrext/video-content-type
a=extmap:7 http://www.webrtc.org/experiments/rtp-hdrext/video-timing
a=extmap:8 http://www.webrtc.org/experiments/rtp-hdrext/color-space
a=extmap:9 urn:ietf:params:rtp-hdrext:sdes:mid
a=extmap:10 urn:ietf:params:rtp-hdrext:sdes:rtp-stream-id
a=extmap:11 urn:ietf:params:rtp-hdrext:sdes:repaired-rtp-stream-id
a=sendonly
a=msid:- 6364666f-6f27-4d06-a40e-940cccd00eed
a=rtcp-mux
a=rtcp-rsize
a=rtpmap:127 VP8/90000
a=rtcp-fb:127 goog-remb
a=rtcp-fb:127 transport-cc
a=rtcp-fb:127 ccm fir
a=rtcp-fb:127 nack
a=rtcp-fb:127 nack pli
a=fmtp:127 implementation_name=Internal
a=rtpmap:120 rtx/90000
a=fmtp:120 apt=127
a=rtpmap:125 VP9/90000
a=rtcp-fb:125 goog-remb
a=rtcp-fb:125 transport-cc
a=rtcp-fb:125 ccm fir
a=rtcp-fb:125 nack
a=rtcp-fb:125 nack pli
a=fmtp:125 implementation_name=Internal;profile-id=0
a=rtpmap:119 rtx/90000
a=fmtp:119 apt=125
a=rtpmap:124 VP9/90000
a=rtcp-fb:124 goog-remb
a=rtcp-fb:124 transport-cc
a=rtcp-fb:124 ccm fir
a=rtcp-fb:124 nack
a=rtcp-fb:124 nack pli
a=fmtp:124 implementation_name=Internal;profile-id=2
a=rtpmap:118 rtx/90000
a=fmtp:118 apt=124
a=rtpmap:123 H264/90000
a=rtcp-fb:123 goog-remb
a=rtcp-fb:123 transport-cc
a=rtcp-fb:123 ccm fir
a=rtcp-fb:123 nack
a=rtcp-fb:123 nack pli
a=fmtp:123 implementation_name=VideoToolbox;level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=640c1f
a=rtpmap:117 rtx/90000
a=fmtp:117 apt=123
a=rtpmap:122 H264/90000
a=rtcp-fb:122 goog-remb
a=rtcp-fb:122 transport-cc
a=rtcp-fb:122 ccm fir
a=rtcp-fb:122 nack
a=rtcp-fb:122 nack pli
a=fmtp:122 implementation_name=VideoToolbox;level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=42e01f
a=rtpmap:116 rtx/90000
a=fmtp:116 apt=122
a=rtpmap:39 AV1/90000
a=rtcp-fb:39 goog-remb
a=rtcp-fb:39 transport-cc
a=rtcp-fb:39 ccm fir
a=rtcp-fb:39 nack
a=rtcp-fb:39 nack pli
a=fmtp:39 implementation_name=Internal
a=rtpmap:40 rtx/90000
a=fmtp:40 apt=39
a=rtpmap:115 red/90000
a=rtpmap:114 rtx/90000
a=fmtp:114 apt=115
a=rtpmap:121 ulpfec/90000
a=ssrc-group:FID 667103293 1525532025
a=ssrc:667103293 cname:rSijxOb75cVEZipP
a=ssrc:667103293 msid:- 6364666f-6f27-4d06-a40e-940cccd00eed
a=ssrc:1525532025 cname:rSijxOb75cVEZipP
a=ssrc:1525532025 msid:- 6364666f-6f27-4d06-a40e-940cccd00eed
m=application 62624 UDP/DTLS/SCTP webrtc-datachannel
c=IN IP4 97.113.95.58
a=candidate:2208109002 1 udp 2122194687 192.168.0.62 62624 typ host generation 0 network-id 1 network-cost 50
a=candidate:2977440256 1 udp 2122265343 fd7b:7f96:14ec:18fc:4a2:7b0b:9b0e:99c9 53897 typ host generation 0 network-id 2 network-cost 50
a=candidate:1486762379 1 udp 1685987071 97.113.95.58 62624 typ srflx raddr 192.168.0.62 rport 62624 generation 0 network-id 1 network-cost 50
a=candidate:4250071890 1 tcp 1518214911 192.168.0.62 54186 typ host tcptype passive generation 0 network-id 1 network-cost 50
a=candidate:3484926104 1 tcp 1518285567 fd7b:7f96:14ec:18fc:4a2:7b0b:9b0e:99c9 54187 typ host tcptype passive generation 0 network-id 2 network-cost 50
a=ice-ufrag:c/WI
a=ice-pwd:7PV5SNpanak/qTOvUN0mUKkz
a=ice-options:trickle
a=fingerprint:sha-256 D9:C2:E1:0A:69:DF:18:E8:FD:89:51:04:D5:B2:96:B2:A7:EC:23:A5:B3:49:E3:C6:F5:7A:A0:AF:CF:1A:5F:81
a=setup:actpass
a=mid:1
a=sctp-port:5000
a=max-message-size:262144";

        let sdp = RTCSessionDescription::offer(offer.into()).unwrap().unmarshal().unwrap();

        let (supported_tracks, notices) = determine_supported_video_tracks(&sdp);

        assert_eq!(notices.len(), 0);
        assert_eq!(supported_tracks.len(), 1);
        dbg!(supported_tracks);
    }

    fn sdp_unmarshal(sdp_str: &str) -> SessionDescription {
        SessionDescription::unmarshal(&mut Cursor::new(sdp_str)).expect("Failed to parse SDP for test")
    }

    #[test]
    fn test_determine_no_video_m_lines() {
        let sdp = "v=0\r\n\
                   o=- 0 0 IN IP4 127.0.0.1\r\n\
                   s=-\r\n\
                   t=0 0\r\n\
                   m=audio 9 UDP/TLS/RTP/SAVPF 0\r\n\
                   a=mid:audio0\r\n";
        let (tracks, notices) = determine_supported_video_tracks(&sdp_unmarshal(sdp));
        assert!(tracks.is_empty());
        assert!(notices.is_empty());
    }

    #[test]
    fn test_determine_video_m_line_port_zero() {
        let sdp = "v=0\r\n\
                   o=- 0 0 IN IP4 127.0.0.1\r\n\
                   s=-\r\n\
                   t=0 0\r\n\
                   m=video 0 UDP/TLS/RTP/SAVPF 96\r\n\
                   a=mid:video0\r\n\
                   a=sendonly\r\n";
        let (tracks, notices) = determine_supported_video_tracks(&sdp_unmarshal(sdp));
        assert!(tracks.is_empty());
        assert_eq!(notices.len(), 1);
        assert_eq!(notices[0], ServerMessage::notice("Media track is disabled (port == 0). We don't support this."));
    }

    #[test]
    fn test_determine_video_sendrecv_unsupported() {
        let sdp = "v=0\r\n\
                   o=- 0 0 IN IP4 127.0.0.1\r\n\
                   s=-\r\n\
                   t=0 0\r\n\
                   m=video 9 UDP/TLS/RTP/SAVPF 96\r\n\
                   a=mid:video0\r\n\
                   a=msid:stream1 track1\r\n\
                   a=sendrecv\r\n";
        let (tracks, notices) = determine_supported_video_tracks(&sdp_unmarshal(sdp));
        assert_eq!(notices.len(), 1);
        assert_eq!(notices[0], ServerMessage::notice("Media track direction is set to [sendrecv]. We only support [send | (undefined)]"));
    }
}
