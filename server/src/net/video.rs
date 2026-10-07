//! Window video data-plane hub (phase 3C).
//!
//! Mirror of [`super::frames::FrameHub`] for the **encoded** video plane, but
//! with the two differences inter-frame codecs force:
//!
//! - it is **not** latest-frame-wins: dropping an arbitrary P-frame corrupts the
//!   picture until the next keyframe, so the queue is **bounded** and drops by
//!   whole GOPs (from a keyframe boundary);
//! - when it cannot keep a decodable point it records a **keyframe request** that
//!   the core drains and forwards to `VideoEncoderSession::request_keyframe()`.
//!
//! One unidirectional QUIC stream per `RemoteWindow` (Model B). The wire shape is
//! `[be32 header_len][WindowVideoStreamHeader][payload raw]` (see
//! `crate::protocol::video`). The first message is always `HELLO` with
//! `stream_type = DATA_STREAM_WINDOW_VIDEO`.

use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::{Arc, Mutex},
};

use tokio::sync::{watch, Notify};

use crate::gpu::{EncodedFrame, VideoChroma, VideoCodec};
use crate::protocol::video;

/// Stream configuration (codec/size) plus the codec parameter sets in Annex-B.
///
/// `codec_config` is always carried with `codec_config = true` on the wire: it is
/// the parameter-set-only payload (VPS/SPS/PPS for HEVC, SPS/PPS for H.264) that
/// precedes the first picture and is repeated on resize/codec change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoConfig {
    pub codec: VideoCodec,
    pub chroma: VideoChroma,
    pub width: u32,
    pub height: u32,
    /// Parameter sets in Annex-B (no picture).
    pub codec_config: Vec<u8>,
}

#[derive(Debug, Clone)]
struct QueuedFrame {
    /// Config in effect when the frame was enqueued (so codec/size always match
    /// the bitstream, even across a resize).
    config: VideoConfig,
    frame: EncodedFrame,
}

#[derive(Default)]
struct StreamState {
    queue: VecDeque<QueuedFrame>,
    /// Latest config (sent when `config_pending`).
    config: Option<VideoConfig>,
    config_pending: bool,
    /// Set when the queue could not preserve a decodable point.
    keyframe_needed: bool,
    /// Window closed: the writer drains and exits.
    closed: bool,
}

/// Implemented by the transport; [`crate::wayland::AquaState`] only sees this.
pub trait VideoSink: Send + Sync {
    /// Install/replace the stream config for a window (creates the stream on the
    /// first call). Sent as `CONFIG` before the next frame.
    fn submit_config(&self, window_id: &str, config: VideoConfig);
    /// Append one encoded access unit to the window's bounded queue.
    fn submit_frame(&self, frame: EncodedFrame);
    /// A window closed: flush and end its stream.
    fn drop_window(&self, _window_id: &str) {}
    /// Windows whose queue overflowed and need a forced keyframe. Drained by the
    /// core so it can call `VideoEncoderSession::request_keyframe()`.
    fn take_keyframe_requests(&self) -> Vec<String> {
        Vec::new()
    }
}

struct Entry {
    state: Arc<Mutex<StreamState>>,
    notify: Arc<Notify>,
}

/// Shared window-video hub. Cheap to clone via `Arc`.
pub struct VideoHub {
    streams: Mutex<HashMap<String, Entry>>,
    catalog: watch::Sender<Vec<String>>,
    capacity: usize,
}

impl VideoHub {
    pub fn new() -> Arc<Self> {
        Self::with_capacity(default_capacity())
    }

    pub fn with_capacity(capacity: usize) -> Arc<Self> {
        let (catalog, _) = watch::channel(Vec::new());
        Arc::new(Self {
            streams: Mutex::new(HashMap::new()),
            catalog,
            capacity: capacity.max(1),
        })
    }

    /// Number of queued frames allowed per window before the drop policy kicks
    /// in. Small on purpose: video latency is bounded by this.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    fn entry_or_create(&self, window_id: &str) -> (Arc<Mutex<StreamState>>, Arc<Notify>, bool) {
        let mut streams = self.streams.lock().unwrap();
        if let Some(entry) = streams.get(window_id) {
            return (entry.state.clone(), entry.notify.clone(), false);
        }
        let state = Arc::new(Mutex::new(StreamState::default()));
        let notify = Arc::new(Notify::new());
        streams.insert(
            window_id.to_string(),
            Entry {
                state: state.clone(),
                notify: notify.clone(),
            },
        );
        let mut catalog = self.catalog.borrow().clone();
        if !catalog.iter().any(|id| id == window_id) {
            catalog.push(window_id.to_string());
            self.catalog.send_replace(catalog);
        }
        (state, notify, true)
    }

    fn subscribe_catalog(&self) -> watch::Receiver<Vec<String>> {
        self.catalog.subscribe()
    }

    fn entry_handles(&self, window_id: &str) -> Option<(Arc<Mutex<StreamState>>, Arc<Notify>)> {
        self.streams
            .lock()
            .unwrap()
            .get(window_id)
            .map(|e| (e.state.clone(), e.notify.clone()))
    }

    fn remove_window(&self, window_id: &str) {
        let entry = self.streams.lock().unwrap().remove(window_id);
        if let Some(entry) = entry {
            {
                let mut state = entry.state.lock().unwrap();
                state.closed = true;
            }
            entry.notify.notify_waiters();
        }
        let catalog: Vec<String> = self
            .catalog
            .borrow()
            .iter()
            .filter(|id| id.as_str() != window_id)
            .cloned()
            .collect();
        self.catalog.send_replace(catalog);
    }

    /// Bounded-queue drop policy: prefer to keep whole GOPs. Drops older
    /// GOPs (everything before the most recent keyframe); if even the newest GOP
    /// alone is too big (or there is no keyframe at all), clears the queue and
    /// asks for a keyframe. Never leaves an arbitrary P-frame at the front.
    fn make_room(state: &mut StreamState, capacity: usize) {
        if state.queue.len() < capacity {
            return;
        }
        match state.queue.iter().rposition(|q| q.frame.keyframe) {
            Some(k) if k > 0 => {
                state.queue.drain(0..k);
            }
            None => {
                state.queue.clear();
                state.keyframe_needed = true;
                return;
            }
            Some(_) => { /* newest keyframe is already at the front */ }
        }
        if state.queue.len() >= capacity {
            state.queue.clear();
            state.keyframe_needed = true;
        }
    }
}

impl VideoSink for VideoHub {
    fn submit_config(&self, window_id: &str, config: VideoConfig) {
        let (state, notify, _created) = self.entry_or_create(window_id);
        {
            let mut state = state.lock().unwrap();
            state.config = Some(config);
            state.config_pending = true;
        }
        notify.notify_one();
    }

    fn submit_frame(&self, frame: EncodedFrame) {
        let window_id = frame.window_id.clone();
        let (state, notify, _created) = self.entry_or_create(&window_id);
        {
            let mut state = state.lock().unwrap();
            let Some(config) = state.config.clone() else {
                // No CONFIG yet: a frame cannot be labelled. Drop it; the encoder
                // always emits CONFIG before the first picture.
                tracing::debug!(window = %window_id, "video.frame without config dropped");
                return;
            };
            if state.queue.len() >= self.capacity {
                if frame.keyframe {
                    // The incoming frame starts a fresh GOP: it is safe to discard
                    // everything older, and no keyframe must be requested.
                    state.queue.clear();
                } else {
                    Self::make_room(&mut state, self.capacity);
                }
            }
            state.queue.push_back(QueuedFrame { config, frame });
        }
        notify.notify_one();
    }

    fn drop_window(&self, window_id: &str) {
        self.remove_window(window_id);
    }

    fn take_keyframe_requests(&self) -> Vec<String> {
        let mut out = Vec::new();
        for (id, entry) in self.streams.lock().unwrap().iter() {
            let mut state = entry.state.lock().unwrap();
            if state.keyframe_needed {
                state.keyframe_needed = false;
                out.push(id.clone());
            }
        }
        out
    }
}

fn default_capacity() -> usize {
    std::env::var("AQUA_VIDEO_QUEUE")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|v| *v >= 1)
        .unwrap_or(8)
}

/// Per-connection dispatcher: one unidirectional stream per window.
pub async fn dispatch_video(connection: quinn::Connection, hub: Arc<VideoHub>) {
    let mut catalog = hub.subscribe_catalog();
    let mut started: HashSet<String> = HashSet::new();

    loop {
        let ids = catalog.borrow_and_update().clone();
        for window_id in ids {
            if !started.insert(window_id.clone()) {
                continue;
            }
            let Some((state, notify)) = hub.entry_handles(&window_id) else {
                continue;
            };
            match connection.open_uni().await {
                Ok(send) => {
                    tracing::debug!(window_id = %window_id, "video stream opened");
                    tokio::spawn(video_writer(send, window_id.clone(), state, notify));
                }
                Err(error) => {
                    tracing::debug!(%error, "failed to open video stream");
                    break;
                }
            }
        }

        if catalog.changed().await.is_err() {
            break;
        }
    }
}

async fn video_writer(
    mut send: quinn::SendStream,
    window_id: String,
    state: Arc<Mutex<StreamState>>,
    notify: Arc<Notify>,
) {
    if send
        .write_all(&video::hello_window_stream_message(&window_id))
        .await
        .is_err()
    {
        return;
    }

    loop {
        let (config, frames, closed) = {
            let mut state = state.lock().unwrap();
            let config = if state.config_pending {
                state.config.clone()
            } else {
                None
            };
            state.config_pending = false;
            let frames: Vec<QueuedFrame> = state.queue.drain(..).collect();
            (config, frames, state.closed)
        };

        if let Some(config) = &config {
            let message = video::config_stream_message(
                &window_id,
                config.codec.to_wire(),
                config.chroma.to_wire(),
                config.width,
                config.height,
                true,
                &config.codec_config,
            );
            if send.write_all(&message).await.is_err() {
                return;
            }
        }

        for queued in frames {
            let config = &queued.config;
            let frame = &queued.frame;
            let message = video::frame_stream_message(
                &window_id,
                config.codec.to_wire(),
                config.chroma.to_wire(),
                config.width,
                config.height,
                frame.frame_id,
                frame.keyframe,
                frame.pts_us,
                &frame.data,
            );
            if send.write_all(&message).await.is_err() {
                return;
            }
        }

        if closed {
            break;
        }
        notify.notified().await;
    }
    let _ = send.finish();
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;

    fn config(width: u32) -> VideoConfig {
        VideoConfig {
            codec: VideoCodec::Hevc,
            chroma: VideoChroma::Nv12,
            width,
            height: 600,
            codec_config: vec![0x00, 0x00, 0x00, 0x01, 0x40],
        }
    }

    fn frame(id: u64, keyframe: bool) -> EncodedFrame {
        EncodedFrame {
            window_id: "window-1".into(),
            frame_id: id,
            keyframe,
            codec_config: false,
            pts_us: id * 16_666,
            data: Bytes::from_static(&[0xaa, 0xbb]),
        }
    }

    #[test]
    fn config_creates_stream_and_is_pending() {
        let hub = VideoHub::with_capacity(4);
        hub.submit_config("window-1", config(800));
        assert!(hub.entry_handles("window-1").is_some());
        let state = hub.streams.lock().unwrap().get("window-1").unwrap().state.clone();
        let state = state.lock().unwrap();
        assert!(state.config_pending);
        assert_eq!(state.config.as_ref().unwrap().width, 800);
    }

    #[test]
    fn bounds_queue_and_drops_whole_old_gops() {
        let hub = VideoHub::with_capacity(3);
        hub.submit_config("window-1", config(800));
        // GOP 1: KF at id 1, then P 2,3.
        hub.submit_frame(frame(1, true));
        hub.submit_frame(frame(2, false));
        hub.submit_frame(frame(3, false));
        // GOP 2: KF at 4 -> overflow should drop GOP 1 entirely.
        hub.submit_frame(frame(4, true));

        let entry = hub.streams.lock().unwrap().get("window-1").unwrap().state.clone();
        let state = entry.lock().unwrap();
        let ids: Vec<u64> = state.queue.iter().map(|q| q.frame.frame_id).collect();
        assert_eq!(ids, vec![4], "older GOP dropped, newest keyframe kept");
        assert!(!state.keyframe_needed);
    }

    #[test]
    fn queue_without_keyframe_requests_one() {
        let hub = VideoHub::with_capacity(2);
        hub.submit_config("window-1", config(800));
        hub.submit_frame(frame(1, false));
        hub.submit_frame(frame(2, false));
        // No keyframe anywhere: overflow must clear and ask for a keyframe.
        hub.submit_frame(frame(3, false));
        assert_eq!(hub.take_keyframe_requests(), vec!["window-1".to_string()]);
        // Drained exactly once.
        assert!(hub.take_keyframe_requests().is_empty());
    }

    #[test]
    fn frame_without_config_is_dropped() {
        let hub = VideoHub::with_capacity(4);
        hub.submit_frame(frame(1, true));
        let entry = hub.streams.lock().unwrap().get("window-1").unwrap().state.clone();
        assert!(entry.lock().unwrap().queue.is_empty());
    }

    #[test]
    fn drop_window_marks_closed_and_removes_from_catalog() {
        let hub = VideoHub::with_capacity(4);
        hub.submit_config("window-1", config(800));
        assert_eq!(hub.subscribe_catalog().borrow().len(), 1);
        hub.drop_window("window-1");
        assert!(hub.entry_handles("window-1").is_none());
        assert!(hub.subscribe_catalog().borrow().is_empty());
    }
}
