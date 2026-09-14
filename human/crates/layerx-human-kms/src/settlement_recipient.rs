use super::{hex, signing_key, Store};
use crate::wire::{Error, Request, Result};
use layerx_crypto::settlement_recipient::RecipientAuthorization;

impl Store {
    pub(super) fn authorize_recipient(&mut self, request: &Request<'_>) -> Result<Vec<u8>> {
        let value = RecipientAuthorization::decode(request.evm).map_err(|_| Error::Refused)?;
        let record = self
            .state
            .records
            .get_mut(&hex(&request.binding))
            .ok_or(Error::NotFound)?;
        if request.class != 1
            || record.class != 1
            || request.reference != record.handle
            || value.binding_digest != record.binding
            || value.network_id != self.state.network
            || value.public_key != record.public
            || record
                .wallet
                .as_ref()
                .is_none_or(|wallet| wallet.address != value.recipient)
        {
            return Err(Error::Refused);
        }
        let identity =
            layerx_wire::hash::did_id_for_protocol(&value.did, 3).map_err(|_| Error::Refused)?;
        if record
            .recipient_identity
            .is_some_and(|previous| previous != identity)
        {
            return Err(Error::Conflict);
        }
        let seed = record.seed.as_ref().ok_or(Error::NotFound)?;
        let signature = signing_key(seed)?
            .sign(&value.message().map_err(|_| Error::Refused)?)
            .as_ref()
            .to_vec();
        record.recipient_identity = Some(identity);
        self.persist()?;
        Ok(signature)
    }
}
