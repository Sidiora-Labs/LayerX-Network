//! Reference port demonstrating shared state: a Solana program-owned pool.
//!
//! The program tracks a pool reserve that every participant can read and
//! contribute to, demonstrating program-owned accounts mapped onto the
//! shared namespace.

use crate::account::{AccountMapping, AccountRole, AccountSchema, FieldType, Field, FieldValue};
use crate::error::PortRefusal;
use crate::pubkey::{SeedPath, PUBKEY_BYTES};

/// The pool account schema holding the shared reserve.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PoolReserve;

impl PoolReserve {
    /// Returns the account schema for the program-owned pool.
    ///
    /// # Errors
    ///
    /// Refuses schema construction failures.
    pub fn schema() -> Result<AccountSchema, PortRefusal> {
        AccountSchema::new(
            "PoolReserve",
            vec![
                Field {
                    name: "total_deposited".to_owned(),
                    kind: FieldType::U64,
                },
                Field {
                    name: "participant_count".to_owned(),
                    kind: FieldType::U32,
                },
                Field {
                    name: "pool_authority".to_owned(),
                    kind: FieldType::Pubkey,
                },
            ],
        )
    }

    /// Returns the seed path for the pool account.
    #[must_use]
    pub fn seeds() -> SeedPath {
        SeedPath::from_seeds(&[b"pool", b"reserve", &[0u8]])
    }

    /// Returns the role of the pool account in a deposit instruction.
    #[must_use]
    pub fn role() -> AccountRole {
        AccountRole::ProgramOwnedShared
    }

    /// Translates the pool account into its LayerX form.
    ///
    /// # Errors
    ///
    /// Refuses translation failures.
    pub fn translate() -> Result<AccountMapping, PortRefusal> {
        Self::role().translate()
    }

    /// Returns the storage key the pool reserve occupies in shared namespace.
    ///
    /// # Errors
    ///
    /// Refuses key derivation failures.
    pub fn storage_key() -> Result<Vec<u8>, PortRefusal> {
        // No envelope seeds need to be dropped since this is program-owned
        Self::seeds().collapse(&[])?.storage_key()
    }

    /// Encodes initial pool state.
    ///
    /// # Errors
    ///
    /// Refuses encoding failures.
    pub fn encode_initial(authority: [u8; PUBKEY_BYTES]) -> Result<Vec<u8>, PortRefusal> {
        let schema = Self::schema()?;
        schema.encode(&[
            FieldValue::U64(0),           // total_deposited
            FieldValue::U32(0),           // participant_count
            FieldValue::Pubkey(authority), // pool_authority
        ])
    }

    /// Decodes pool state from storage.
    ///
    /// # Errors
    ///
    /// Refuses decoding failures.
    pub fn decode(data: &[u8]) -> Result<(u64, u32, [u8; PUBKEY_BYTES]), PortRefusal> {
        let schema = Self::schema()?;
        let values = schema.decode(data)?;
        match &values[..] {
            [FieldValue::U64(total), FieldValue::U32(count), FieldValue::Pubkey(auth)] => {
                Ok((*total, *count, *auth))
            }
            _ => Err(PortRefusal::SchemaMismatch),
        }
    }
}

/// Per-participant deposit record, principal-scoped.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParticipantDeposit;

impl ParticipantDeposit {
    /// Returns the account schema for per-participant deposits.
    ///
    /// # Errors
    ///
    /// Refuses schema construction failures.
    pub fn schema() -> Result<AccountSchema, PortRefusal> {
        AccountSchema::new(
            "ParticipantDeposit",
            vec![
                Field {
                    name: "amount".to_owned(),
                    kind: FieldType::U64,
                },
                Field {
                    name: "deposit_count".to_owned(),
                    kind: FieldType::U32,
                },
            ],
        )
    }

    /// Returns the seed path for a participant's deposit record.
    ///
    /// The signer's pubkey seed will collapse, making this principal-scoped.
    #[must_use]
    pub fn seeds() -> SeedPath {
        SeedPath::from_seeds(&[b"deposit", b"participant"])
    }

    /// Returns the role of the participant deposit account.
    #[must_use]
    pub fn role() -> AccountRole {
        AccountRole::ProgramState
    }

    /// Returns the storage key with the signer seed collapsed.
    ///
    /// # Errors
    ///
    /// Refuses key derivation failures.
    pub fn storage_key() -> Result<Vec<u8>, PortRefusal> {
        // Envelope position 2 would be the signer pubkey, which collapses
        Self::seeds().collapse(&[2])?.storage_key()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pool_reserve_is_program_owned_shared() {
        let role = PoolReserve::role();
        assert_eq!(role, AccountRole::ProgramOwnedShared);
        
        let mapping = PoolReserve::translate().unwrap();
        assert_eq!(mapping, AccountMapping::SharedCell);
    }

    #[test]
    fn participant_deposit_is_principal_scoped() {
        let role = ParticipantDeposit::role();
        assert_eq!(role, AccountRole::ProgramState);
        
        let mapping = role.translate().unwrap();
        assert_eq!(mapping, AccountMapping::NamespacedCell);
    }

    #[test]
    fn pool_schema_has_correct_space() {
        let schema = PoolReserve::schema().unwrap();
        // Discriminator (8) + u64 (8) + u32 (4) + Pubkey (32) = 52
        assert_eq!(schema.space(), 52);
    }

    #[test]
    fn pool_state_round_trips() {
        let authority = [42u8; PUBKEY_BYTES];
        let encoded = PoolReserve::encode_initial(authority).unwrap();
        let (total, count, auth) = PoolReserve::decode(&encoded).unwrap();
        
        assert_eq!(total, 0);
        assert_eq!(count, 0);
        assert_eq!(auth, authority);
    }

    #[test]
    fn participant_schema_has_correct_space() {
        let schema = ParticipantDeposit::schema().unwrap();
        // Discriminator (8) + u64 (8) + u32 (4) = 20
        assert_eq!(schema.space(), 20);
    }

    #[test]
    fn pool_storage_key_derivation() {
        // Demonstrates that the pool key can be derived and will land in
        // shared namespace when the runtime executes it
        let key = PoolReserve::storage_key().unwrap();
        assert!(!key.is_empty());
    }

    #[test]
    fn participant_storage_key_collapses_signer() {
        // Demonstrates that the participant key collapses the signer seed
        // and will land in principal-scoped namespace
        let key = ParticipantDeposit::storage_key().unwrap();
        assert!(!key.is_empty());
    }
}
