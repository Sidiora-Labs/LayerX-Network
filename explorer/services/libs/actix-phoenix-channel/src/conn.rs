use crate::client::ChannelClient;
use actix_ws::Session;

pub struct ChannelConn {
    pub(crate) session: Session,
    pub(crate) client: ChannelClient,
}

impl ChannelConn {
    pub fn new(session: Session, client: ChannelClient) -> Self {
        Self { session, client }
    }

    pub fn client(&self) -> &ChannelClient {
        &self.client
    }

    pub fn session(&mut self) -> &mut Session {
        &mut self.session
    }
}
