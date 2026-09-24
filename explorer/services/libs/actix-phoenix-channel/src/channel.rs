use crate::{
    broadcaster::ChannelBroadcaster, client::ChannelClient, client_receiver::ClientReceiver,
    event::ChannelEvent, handler::ChannelHandler,
};
use async_broadcast::{InactiveReceiver, Sender};

const CHANNEL_CAP: usize = 10;

#[derive(Debug)]
pub struct ChannelCentral<CH> {
    handler: CH,
    broadcast_sender: Sender<ChannelEvent>,
    broadcast_receiver: InactiveReceiver<ChannelEvent>,
}

impl<CH> ChannelCentral<CH>
where
    CH: ChannelHandler,
{
    pub fn new(handler: CH) -> Self {
        let (mut broadcast_sender, broadcast_receiver) = async_broadcast::broadcast(CHANNEL_CAP);
        broadcast_sender.set_overflow(true);
        let broadcast_receiver = broadcast_receiver.deactivate();
        Self {
            handler,
            broadcast_sender,
            broadcast_receiver,
        }
    }

    pub fn channel_broadcaster(&self) -> ChannelBroadcaster {
        ChannelBroadcaster::new(
            self.broadcast_sender.clone(),
            self.broadcast_receiver.clone(),
        )
    }

    pub fn build_client(&self) -> (ChannelClient, ClientReceiver) {
        ChannelClient::new(
            self.broadcast_sender.clone(),
            self.broadcast_receiver.activate_cloned(),
        )
    }

    pub fn handler(&self) -> &CH {
        &self.handler
    }
}
