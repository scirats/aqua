use crate::domain::RemoteEvent;

/// Consumer of the neutral domain event stream.
///
/// Intentionally tiny: the future transport implements this trait. There is no
/// event bus, no async runtime, and no coupling to the compositor.
pub trait RemoteEventSink {
    fn handle(&mut self, event: &RemoteEvent);
}

impl<T: RemoteEventSink + ?Sized> RemoteEventSink for Box<T> {
    fn handle(&mut self, event: &RemoteEvent) {
        (**self).handle(event);
    }
}

/// In-memory sink used by tests and the demo control channel (`list` command).
#[derive(Debug, Default)]
pub struct RecordingSink {
    pub events: Vec<RemoteEvent>,
}

impl RecordingSink {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn take(&mut self) -> Vec<RemoteEvent> {
        std::mem::take(&mut self.events)
    }

    pub fn kinds(&self) -> Vec<&'static str> {
        self.events.iter().map(RemoteEvent::kind).collect()
    }
}

impl RemoteEventSink for RecordingSink {
    fn handle(&mut self, event: &RemoteEvent) {
        self.events.push(event.clone());
    }
}
