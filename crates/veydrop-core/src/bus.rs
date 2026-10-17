//! Fan-out of engine events: one channel for the desktop UI, one for phones.

use tokio::sync::broadcast;

use crate::model::{EngineEvent, PhoneEvent};

#[derive(Clone)]
pub struct Bus {
    events: broadcast::Sender<EngineEvent>,
    phones: broadcast::Sender<PhoneEvent>,
}

impl Default for Bus {
    fn default() -> Self {
        Self::new()
    }
}

impl Bus {
    pub fn new() -> Self {
        Self { events: broadcast::channel(512).0, phones: broadcast::channel(256).0 }
    }

    /// Sends to the desktop UI. No receivers is fine (UI not attached yet).
    pub fn emit(&self, e: EngineEvent) {
        let _ = self.events.send(e);
    }

    /// Sends to one phone session (`Some`) or to every connected phone (`None`).
    pub fn phone(&self, target: Option<&str>, kind: &'static str, data: serde_json::Value) {
        let _ = self.phones.send(PhoneEvent { target: target.map(str::to_owned), kind, data });
    }

    pub fn subscribe(&self) -> broadcast::Receiver<EngineEvent> {
        self.events.subscribe()
    }

    pub fn subscribe_phones(&self) -> broadcast::Receiver<PhoneEvent> {
        self.phones.subscribe()
    }
}
