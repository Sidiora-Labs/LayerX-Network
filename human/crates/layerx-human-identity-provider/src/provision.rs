use std::io::{self, Read, Write};

use serde::{Deserialize, Serialize};

use crate::{invalid, State};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    email: String,
    display_name: String,
    idempotency_key: String,
    now: u64,
}

#[derive(Serialize)]
struct Response {
    principal: String,
    did: String,
    recovery_root: [u8; 32],
    recovery_threshold: u16,
    recovery_delay_seconds: u64,
}

impl State {
    /// Executes LXIP operation 1 against exclusively held state and exports its fields.
    ///
    /// # Errors
    /// Refuses oversized or invalid input and the same state conflicts as LXIP.
    pub fn provision_owner(&mut self, input: impl Read, mut output: impl Write) -> io::Result<()> {
        let mut bytes = Vec::new();
        input.take(16_385).read_to_end(&mut bytes)?;
        if bytes.len() > 16_384 {
            return Err(invalid("owner provisioning input exceeds 16384 bytes"));
        }
        let request: Request = serde_json::from_slice(&bytes)?;
        self.ready()?;
        let fields = self.provision(&[
            request.email.into_bytes(),
            request.display_name.into_bytes(),
            request.idempotency_key.into_bytes(),
            request.now.to_be_bytes().to_vec(),
        ])?;
        let [principal, did, root, threshold, delay] = fields.as_slice() else {
            return Err(invalid("unexpected provision response"));
        };
        let response = Response {
            principal: String::from_utf8(principal.clone())
                .map_err(|_| invalid("invalid principal"))?,
            did: String::from_utf8(did.clone()).map_err(|_| invalid("invalid DID"))?,
            recovery_root: root
                .as_slice()
                .try_into()
                .map_err(|_| invalid("invalid root"))?,
            recovery_threshold: u16::from_be_bytes(
                threshold
                    .as_slice()
                    .try_into()
                    .map_err(|_| invalid("invalid threshold"))?,
            ),
            recovery_delay_seconds: u64::from_be_bytes(
                delay
                    .as_slice()
                    .try_into()
                    .map_err(|_| invalid("invalid delay"))?,
            ),
        };
        let encoded = serde_json::to_vec(&response)?;
        output.write_all(&encoded)?;
        output.write_all(b"\n")
    }
}
