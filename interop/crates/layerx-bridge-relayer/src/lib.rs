//! `LayerX` bridge relayer: carries `PaxeerXVault` deposits on Ethereum chains
//! into `bridgeIn` on the Paxeer precompile at `0x…1016`, and Paxeer
//! `BridgeOut` burns into `PaxeerXVault.release`. With a Solana entry
//! configured it also carries deposits into the Solana custody program into
//! `bridgeIn` (see [`solana`]).
//!
//! Each instance holds exactly one attestor key, behind the remote signer; no
//! chain private key enters the process. Neither verifier aggregates
//! signatures across calls, so a threshold above one is met by instances
//! exchanging signatures through a shared cosign directory and each building
//! the same call from the lowest-address `threshold` signers.
//!
//! Submission is idempotent across instances and exactly-once per instance:
//! the append-only journal records every observed event and every signed
//! transaction's exact bytes before broadcast, so a restart rebroadcasts
//! rather than re-signs; and before and after every submission the relayer
//! reads the destination nullifier, so a second instance's call — including
//! one carrying additional signatures — either is never sent or reverts on
//! the consumed nullifier and is recorded `AlreadyBridged`, never retried.

pub mod abi;
pub mod attestation;
pub mod config;
pub mod cosign;
pub mod hex;
pub mod journal;
pub mod relayer;
pub mod rpc;
pub mod signer;
pub mod solana;
pub mod tx;
