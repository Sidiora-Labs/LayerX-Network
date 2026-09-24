use crate::{event::ChannelEvent, subscription_registry::SubscriptionRegistry};
use async_broadcast::Receiver as BroadcastReceiver;
use async_channel::Receiver;
use futures_lite::{Stream, StreamExt, stream::Race};
use std::{
    pin::Pin,
    task::{Context, Poll},
};

#[derive(Debug)]
pub struct ClientReceiver {
    subscriptions: SubscriptionRegistry,
    race: Pin<Box<Race<BroadcastReceiver<ChannelEvent>, Receiver<ChannelEvent>>>>,
}

impl ClientReceiver {
    pub fn new(
        individual: Receiver<ChannelEvent>,
        broadcast: BroadcastReceiver<ChannelEvent>,
        subscriptions: SubscriptionRegistry,
    ) -> Self {
        Self {
            race: Box::pin(broadcast.race(individual)),
            subscriptions,
        }
    }
}

impl Stream for ClientReceiver {
    type Item = String;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            match self.race.poll_next(cx) {
                Poll::Ready(Some(event)) if !self.subscriptions.subscribes(&event) => continue,
                Poll::Ready(Some(event)) => {
                    if let Ok(text) = event.serialize() {
                        break Poll::Ready(Some(text));
                    }
                }
                Poll::Pending => break Poll::Pending,
                Poll::Ready(None) => break Poll::Ready(None),
            }
        }
    }
}
