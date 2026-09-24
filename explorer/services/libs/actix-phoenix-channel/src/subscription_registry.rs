use crate::event::ChannelEvent;
use dashmap::DashSet;
use std::sync::Arc;

#[derive(Clone, Default, Debug)]
pub struct SubscriptionRegistry(Arc<DashSet<String>>);

impl SubscriptionRegistry {
    pub fn join(&self, topic: String) {
        self.0.insert(topic);
    }

    pub fn leave(&self, topic: &str) {
        self.0.remove(topic);
    }

    pub fn subscribes(&self, event: &ChannelEvent) -> bool {
        event.is_system_event() || self.0.contains(event.topic())
    }
}
