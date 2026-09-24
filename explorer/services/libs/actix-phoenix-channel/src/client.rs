use crate::{
    client_receiver::ClientReceiver, event::ChannelEvent,
    subscription_registry::SubscriptionRegistry,
};
use actix_ws::AggregatedMessage;
use async_broadcast::{Receiver, Sender as BroadcastSender};
use async_channel::Sender;
use serde::Serialize;
use std::ops::Deref;

#[derive(Debug, Clone)]
pub struct ChannelClient {
    subscriptions: SubscriptionRegistry,
    sender: Sender<ChannelEvent>,
    broadcast_sender: BroadcastSender<ChannelEvent>,
}

impl ChannelClient {
    pub(crate) fn new(
        broadcast_sender: BroadcastSender<ChannelEvent>,
        broadcast_receiver: Receiver<ChannelEvent>,
    ) -> (Self, ClientReceiver) {
        let (sender, individual) = async_channel::unbounded();
        let subscriptions = SubscriptionRegistry::default();
        (
            Self {
                subscriptions: subscriptions.clone(),
                sender,
                broadcast_sender,
            },
            ClientReceiver::new(individual, broadcast_receiver, subscriptions),
        )
    }

    pub fn broadcast(&self, event: impl Into<ChannelEvent>) {
        let mut event = event.into();
        event.reference = None;
        if let Err(e) = self.broadcast_sender.try_broadcast(event) {
            tracing::error!("error broadcasting event: {:?}", e);
        }
    }

    pub async fn send_event(&self, event: impl Into<ChannelEvent>) {
        if let Err(e) = self.sender.send(event.into()).await {
            tracing::error!("error sending event: {:?}", e);
        }
    }

    pub async fn reply_ok(&self, event: &ChannelEvent, payload: &impl Serialize) {
        #[derive(serde::Serialize)]
        struct Reply<'a, S> {
            status: &'static str,
            response: &'a S,
        }

        self.send_event(event.build_reply(
            "phx_reply",
            &Reply {
                status: "ok",
                response: payload,
            },
        ))
        .await
    }

    pub async fn reply_error(&self, event: &ChannelEvent, error: &impl Serialize) {
        self.send_event(event.build_reply("phx_error", &error))
            .await
    }
    pub async fn allow_join(&self, event: &ChannelEvent, payload: &impl Serialize) {
        if event.event() != "phx_join" {
            tracing::error!("invalid event for allow_join: {:?}", event);
            return;
        }
        self.subscriptions.join(event.topic.to_string());
        self.reply_ok(event, payload).await;
    }

    pub async fn allow_leave(&self, event: &ChannelEvent, payload: &impl Serialize) {
        if event.event() != "phx_leave" {
            tracing::error!("invalid event for allow_leave: {:?}", event);
            return;
        }
        self.subscriptions.leave(&event.topic);
        self.reply_ok(event, payload).await;
    }

    pub fn subscriptions(&self) -> &SubscriptionRegistry {
        &self.subscriptions
    }

    pub fn deserialize(&self, message: AggregatedMessage) -> Option<ChannelEvent> {
        let string = match message {
            AggregatedMessage::Text(ref string) => Some(string.deref()),
            AggregatedMessage::Binary(ref data) => std::str::from_utf8(data).ok(),
            _ => None,
        }?;
        ChannelEvent::deserialize(string).ok()
    }
}
