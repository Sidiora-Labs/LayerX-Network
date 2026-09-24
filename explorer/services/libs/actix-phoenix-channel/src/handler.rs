use crate::{conn::ChannelConn, event::ChannelEvent};

#[allow(unused_variables)]
#[async_trait::async_trait]
pub trait ChannelHandler: Sized + Send + Sync + 'static {
    async fn join_channel(&self, conn: &ChannelConn, event: ChannelEvent);

    async fn leave_channel(&self, conn: &ChannelConn, event: ChannelEvent) {
        conn.client().allow_leave(&event, &()).await
    }

    async fn incoming_message(&self, conn: &ChannelConn, event: ChannelEvent) {}

    async fn disconnect(&self, conn: &ChannelConn) {}
}
