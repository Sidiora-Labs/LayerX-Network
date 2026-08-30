//! Fresh passkey ceremonies bound to one exact operation digest.

use passkey_auth::{AuthenticationResponse, AuthenticationState, PasskeyCredential};
use serde::{Deserialize, Serialize};

use crate::store::{PrincipalScope, RowKey, Table};

use super::{
    decode_opaque, encode_opaque, find_candidate_passkey, get_json, load_passkeys, mint_identifier,
    put_json, row_key, user_handle, valid_prefixed_id, AuthError, Passkeys, RatePurpose,
    StoredPasskey,
};

const STEP_UP_ROW_PREFIX: &str = "auth-stepup-";
const STEP_UP_EVIDENCE_ROW_PREFIX: &str = "auth-stepup-evidence-";

/// Digest of the exact canonical operation a step-up ceremony confirms.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct OperationDigest([u8; 32]);

impl OperationDigest {
    const SCHEMA_PREFIX: &'static str = "opd_";

    /// Wraps the canonical operation digest.
    #[must_use]
    pub const fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Returns the canonical digest bytes.
    #[must_use]
    pub const fn bytes(self) -> [u8; 32] {
        self.0
    }

    /// Parses the strict lowercase human-api representation.
    ///
    /// # Errors
    ///
    /// Rejects a missing prefix, non-lowercase hexadecimal text, or a digest
    /// whose decoded width is not exactly 32 bytes.
    pub fn parse_schema(value: &str) -> Result<Self, AuthError> {
        let encoded = value
            .strip_prefix(Self::SCHEMA_PREFIX)
            .ok_or(AuthError::InvalidInput("invalid operation digest"))?;
        if encoded.len() != 64
            || !encoded
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(AuthError::InvalidInput("invalid operation digest"));
        }
        let mut bytes = [0_u8; 32];
        for (index, pair) in encoded.as_bytes().chunks_exact(2).enumerate() {
            bytes[index] = (hex_nibble(pair[0])? << 4) | hex_nibble(pair[1])?;
        }
        Ok(Self(bytes))
    }

    /// Renders the strict lowercase human-api representation.
    #[must_use]
    pub fn to_schema(self) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut encoded = String::with_capacity(Self::SCHEMA_PREFIX.len() + 64);
        encoded.push_str(Self::SCHEMA_PREFIX);
        for byte in self.0 {
            encoded.push(char::from(HEX[usize::from(byte >> 4)]));
            encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
        encoded
    }
}

fn hex_nibble(byte: u8) -> Result<u8, AuthError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        _ => Err(AuthError::InvalidInput("invalid operation digest")),
    }
}

/// Browser challenge for one operation-bound passkey ceremony.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StepUpChallenge {
    /// Server-side challenge identifier.
    pub challenge_id: String,
    /// Exact operation being confirmed.
    pub confirms: OperationDigest,
    /// Base64url-wrapped `WebAuthn` request-options JSON.
    pub ceremony: String,
    /// Challenge expiry.
    pub expires_at: u64,
}

/// Fresh operation-bound evidence produced only after a verified passkey
/// assertion. Fields are private so callers cannot construct evidence.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct StepUp {
    challenge_id: String,
    confirms: OperationDigest,
    passkey_id: String,
    completed_at: u64,
    expires_at: u64,
}

impl StepUp {
    /// Challenge identifier.
    #[must_use]
    pub fn challenge_id(&self) -> &str {
        &self.challenge_id
    }

    /// Bound operation digest.
    #[must_use]
    pub const fn confirms(&self) -> OperationDigest {
        self.confirms
    }

    /// Passkey that performed the fresh ceremony.
    #[must_use]
    pub fn passkey_id(&self) -> &str {
        &self.passkey_id
    }

    /// Verification time.
    #[must_use]
    pub const fn completed_at(&self) -> u64 {
        self.completed_at
    }

    /// Evidence expiry.
    #[must_use]
    pub const fn expires_at(&self) -> u64 {
        self.expires_at
    }
}

/// Descriptive alias for completed [`StepUp`] evidence.
pub type StepUpEvidence = StepUp;

#[derive(Debug, Serialize, Deserialize)]
struct StepUpRecord {
    state: AuthenticationState,
    candidate_passkeys: Vec<String>,
    confirms: OperationDigest,
    expires_at: u64,
}

impl Passkeys {
    /// Starts a fresh passkey ceremony for one exact operation. A valid
    /// passkey-authenticated browser session and CSRF token are required to
    /// request the ceremony.
    pub fn begin_step_up(
        &self,
        scope: &mut PrincipalScope<'_>,
        access_token: &str,
        csrf_token: &str,
        confirms: OperationDigest,
        now: u64,
    ) -> Result<StepUpChallenge, AuthError> {
        Self::authorize_base_mutation(scope, access_token, csrf_token, now)?;
        self.check_rate_limit(scope, RatePurpose::StepUp, now)?;
        let passkeys = load_passkeys(scope)?;
        if passkeys.is_empty() {
            return Err(AuthError::NoPasskeys);
        }
        let credentials: Vec<PasskeyCredential> = passkeys
            .iter()
            .map(|(_, passkey)| passkey.credential.clone())
            .collect();
        let (challenge, state) = self
            .webauthn
            .start_authentication_with_creds_for_user(&user_handle(scope), &credentials);
        let challenge_id = mint_identifier("chg_")?;
        let expires_at = now.saturating_add(self.config.ceremony_ttl_secs);
        put_json(
            scope,
            Table::Journeys,
            row_key(STEP_UP_ROW_PREFIX, &challenge_id)?,
            now,
            &StepUpRecord {
                state,
                candidate_passkeys: passkeys
                    .iter()
                    .map(|(_, passkey)| passkey.record.passkey_id.clone())
                    .collect(),
                confirms,
                expires_at,
            },
        )?;
        Ok(StepUpChallenge {
            challenge_id,
            confirms,
            ceremony: encode_opaque(&challenge)?,
            expires_at,
        })
    }

    /// Finishes an operation-bound ceremony and persists the evidence under
    /// this principal, so copying the returned record to another principal
    /// or changing its digest cannot authorize anything.
    pub fn finish_step_up(
        &self,
        scope: &mut PrincipalScope<'_>,
        challenge_id: &str,
        credential: &str,
        now: u64,
    ) -> Result<StepUpEvidence, AuthError> {
        self.check_rate_limit(scope, RatePurpose::StepUp, now)?;
        if !valid_prefixed_id(challenge_id, "chg_") {
            return Err(AuthError::InvalidInput("invalid step-up identifier"));
        }
        let challenge_key = row_key(STEP_UP_ROW_PREFIX, challenge_id)?;
        let challenge = get_json::<StepUpRecord>(scope, Table::Journeys, &challenge_key)?
            .ok_or(AuthError::ChallengeNotFound)?;
        if now > challenge.expires_at {
            scope.remove(Table::Journeys, &challenge_key)?;
            return Err(AuthError::ChallengeExpired);
        }
        let response: AuthenticationResponse = decode_opaque(credential)?;
        let (passkey_key, mut stored): (RowKey, StoredPasskey) =
            find_candidate_passkey(scope, &challenge.candidate_passkeys, &response.id)?;
        let outcome = self
            .webauthn
            .finish_authentication(&challenge.state, &response, &stored.credential)
            .map_err(AuthError::Passkey)?;
        if !outcome.user_verified {
            return Err(AuthError::Passkey(passkey_auth::Error::UserNotVerified));
        }
        stored.credential.counter = outcome.new_counter;
        stored.record.last_used_at = Some(now);
        put_json(scope, Table::Cache, passkey_key, now, &stored)?;
        scope.remove(Table::Journeys, &challenge_key)?;
        let evidence = StepUpEvidence {
            challenge_id: challenge_id.to_owned(),
            confirms: challenge.confirms,
            passkey_id: stored.record.passkey_id,
            completed_at: now,
            expires_at: now.saturating_add(self.config.step_up_ttl_secs),
        };
        put_json(
            scope,
            Table::Cache,
            row_key(STEP_UP_EVIDENCE_ROW_PREFIX, challenge_id)?,
            now,
            &evidence,
        )?;
        Ok(evidence)
    }

    pub(super) fn validate_step_up(
        scope: &PrincipalScope<'_>,
        evidence: &StepUpEvidence,
        expected: OperationDigest,
        now: u64,
    ) -> Result<(), AuthError> {
        if evidence.confirms != expected {
            return Err(AuthError::StepUpMismatch);
        }
        if now > evidence.expires_at {
            return Err(AuthError::StepUpExpired);
        }
        let key = row_key(STEP_UP_EVIDENCE_ROW_PREFIX, &evidence.challenge_id)?;
        let stored = get_json::<StepUpEvidence>(scope, Table::Cache, &key)?
            .ok_or(AuthError::StepUpMismatch)?;
        if stored != *evidence {
            return Err(AuthError::StepUpMismatch);
        }
        Ok(())
    }
}
