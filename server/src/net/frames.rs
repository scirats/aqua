//! Data-plane frame hub.
//!
//! The core (calloop thread) submits owned [`SurfaceFrame`]s here. The hub keeps
//! **one latest frame per surface** (latest-frame-wins, bounded by construction:
//! a `watch` channel holds at most one value). Each QUIC connection runs a
//! dispatcher that opens one unidirectional stream per surface and writes the
//! latest frame as it changes.
//!
//! This is the backpressure design: no unbounded queue can ever build up. If the
//! producer is faster than the network, older frames are simply overwritten.

use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};

use tokio::sync::watch;

use crate::protocol::data::{self, SurfaceFrame};

/// Implemented by the transport; the Wayland adapter only sees this trait.
pub trait FrameSink: Send + Sync {
    fn submit_frame(&self, frame: SurfaceFrame);
    /// Drop all queued/held frames for a destroyed surface (releases memory and
    /// ends its stream writer).
    fn drop_surface(&self, _surface_id: &str) {}
}

/// Shared frame hub. Cheap to clone via `Arc`.
pub struct FrameHub {
    frames: Mutex<HashMap<String, watch::Sender<Option<Arc<SurfaceFrame>>>>>,
    catalog: watch::Sender<Vec<String>>,
}

impl FrameHub {
    pub fn new() -> Arc<Self> {
        let (catalog, _) = watch::channel(Vec::new());
        Arc::new(Self {
            frames: Mutex::new(HashMap::new()),
            catalog,
        })
    }

    fn publish(&self, frame: SurfaceFrame) {
        let surface_id = frame.surface_id.clone();
        let frame = Arc::new(frame);

        let mut is_new = false;
        {
            let mut frames = self.frames.lock().unwrap();
            if !frames.contains_key(&surface_id) {
                is_new = true;
                let (tx, _) = watch::channel(None);
                frames.insert(surface_id.clone(), tx);
            }
            if let Some(sender) = frames.get(&surface_id) {
                sender.send_replace(Some(frame));
            }
        }

        if is_new {
            let mut catalog = self.catalog.borrow().clone();
            if !catalog.contains(&surface_id) {
                catalog.push(surface_id);
                self.catalog.send_replace(catalog);
            }
        }
    }

    fn subscribe_catalog(&self) -> watch::Receiver<Vec<String>> {
        self.catalog.subscribe()
    }

    fn frame_receiver(
        &self,
        surface_id: &str,
    ) -> Option<watch::Receiver<Option<Arc<SurfaceFrame>>>> {
        self.frames
            .lock()
            .unwrap()
            .get(surface_id)
            .map(|sender| sender.subscribe())
    }

    fn remove_surface(&self, surface_id: &str) {
        {
            let mut frames = self.frames.lock().unwrap();
            if frames.remove(surface_id).is_none() {
                return;
            }
        }
        let catalog: Vec<String> = self
            .catalog
            .borrow()
            .iter()
            .filter(|id| id.as_str() != surface_id)
            .cloned()
            .collect();
        self.catalog.send_replace(catalog);
    }
}

impl FrameSink for FrameHub {
    fn submit_frame(&self, frame: SurfaceFrame) {
        self.publish(frame);
    }

    fn drop_surface(&self, surface_id: &str) {
        self.remove_surface(surface_id);
    }
}

/// Per-connection dispatcher: one unidirectional stream per surface.
pub async fn dispatch_frames(connection: quinn::Connection, hub: Arc<FrameHub>) {
    let mut catalog = hub.subscribe_catalog();
    let mut started: HashSet<String> = HashSet::new();

    loop {
        let ids = catalog.borrow_and_update().clone();
        for surface_id in ids {
            if !started.insert(surface_id.clone()) {
                continue;
            }
            let Some(receiver) = hub.frame_receiver(&surface_id) else {
                continue;
            };
            match connection.open_uni().await {
                Ok(send) => {
                    tracing::debug!(surface_id = %surface_id, "surface stream opened");
                    tokio::spawn(surface_writer(send, surface_id, receiver));
                }
                Err(error) => {
                    tracing::debug!(%error, "failed to open surface stream");
                    break;
                }
            }
        }

        if catalog.changed().await.is_err() {
            break;
        }
    }
}

/// Writes the latest frame of one surface to its dedicated stream.
async fn surface_writer(
    mut send: quinn::SendStream,
    surface_id: String,
    mut receiver: watch::Receiver<Option<Arc<SurfaceFrame>>>,
) {
    let window_id = receiver
        .borrow()
        .as_ref()
        .map(|frame| frame.window_id.clone())
        .unwrap_or_default();
    if send
        .write_all(&data::hello_stream_message(&surface_id, &window_id))
        .await
        .is_err()
    {
        return;
    }

    let mut last_sent = 0u64;

    // Send the frame that is already pending (the producer may be idle, e.g. a
    // terminal with no new commits); `changed()` alone would never fire.
    let initial = receiver.borrow_and_update().clone();
    if let Some(frame) = initial {
        last_sent = frame.frame_id;
        if send.write_all(&data::frame_stream_message(&frame)).await.is_err() {
            return;
        }
    }

    loop {
        if receiver.changed().await.is_err() {
            break;
        }
        let frame = receiver.borrow().clone();
        let Some(frame) = frame else { continue };
        if frame.frame_id <= last_sent {
            continue; // stale (already sent a newer frame)
        }
        last_sent = frame.frame_id;
        if send
            .write_all(&data::frame_stream_message(&frame))
            .await
            .is_err()
        {
            break;
        }
    }
    let _ = send.finish();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(surface_id: &str, frame_id: u64) -> SurfaceFrame {
        SurfaceFrame {
            surface_id: surface_id.into(),
            window_id: "window-1".into(),
            frame_id,
            width: 2,
            height: 2,
            stride: 8,
            format: data::FORMAT_ARGB8888,
            damage: Vec::new(),
            data: vec![0u8; 16],
        }
    }

    /// The hub only ever holds the latest frame: no unbounded queue can build up.
    #[test]
    fn latest_frame_wins() {
        let hub = FrameHub::new();
        hub.publish(frame("surface-1", 1));
        hub.publish(frame("surface-1", 2));
        hub.publish(frame("surface-1", 3));

        let receiver = hub.frame_receiver("surface-1").expect("receiver");
        let held = receiver.borrow();
        assert_eq!(held.as_ref().unwrap().frame_id, 3);
    }

    #[test]
    fn catalog_announces_each_surface_once() {
        let hub = FrameHub::new();
        hub.publish(frame("surface-1", 1));
        hub.publish(frame("surface-2", 1));
        hub.publish(frame("surface-1", 2));
        let catalog = hub.subscribe_catalog().borrow().clone();
        assert_eq!(
            catalog,
            vec!["surface-1".to_string(), "surface-2".to_string()]
        );
    }

    #[test]
    fn drop_surface_releases_frames() {
        let hub = FrameHub::new();
        hub.publish(frame("surface-1", 1));
        assert!(hub.frame_receiver("surface-1").is_some());

        hub.remove_surface("surface-1");
        assert!(hub.frame_receiver("surface-1").is_none());
        assert!(hub.subscribe_catalog().borrow().is_empty());
    }
}
