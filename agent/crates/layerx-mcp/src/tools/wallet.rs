use std::future::Future;
use std::pin::pin;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};
use std::time::Duration;

use layerx_agentd::policy::{evaluate, EvaluationInput, Outcome, PolicySet};
use layerx_sdk::rpc::{Commitment, RpcError, RpcValue};
use layerx_sdk::rpc_verification::VerifiedRpcReceipt;
use layerx_sdk::wallet::{PaymentOptions, Wallet};
use layerx_types::payload::ModuleId;

use crate::server::{InvocationOutcome, Server, ServerError};
use crate::tools::write::PaymentTool;

#[derive(Debug)]
pub enum WalletToolError {
    Server(ServerError),
    Rpc(RpcError),
    Policy,
    Scope,
    Arguments,
}

pub struct PaymentExecution<'a> {
    pub wallet: &'a Wallet<'a>,
    pub options: &'a PaymentOptions,
    pub canonical_payload: &'a [u8],
    pub policy: &'a PolicySet,
    pub evaluation: &'a EvaluationInput<'a>,
}

/// # Errors
/// Preserves daemon scope, policy and budget refusal before any wallet signature or submission.
pub fn execute(
    server: &mut Server,
    core_sequence: u64,
    tool: PaymentTool,
    execution: &PaymentExecution<'_>,
) -> Result<VerifiedRpcReceipt, WalletToolError> {
    let input = execution.evaluation;
    if input.session.request.session_id != server.binding().session_id()
        || &input.session.request.tenant != server.binding().tenant()
        || input.capability.id != server.binding().capability_id()
        || input.session.request.agent.as_bytes() != execution.options.actor.as_bytes()
        || input.request.core_sequence != core_sequence
        || input.request.purpose != tool.name()
    {
        return Err(WalletToolError::Scope);
    }
    let ordinal = match tool {
        PaymentTool::Send | PaymentTool::Transfer => 5,
        PaymentTool::Create => 1,
        PaymentTool::Mint => 10,
    };
    server
        .execute_committed(
            core_sequence,
            tool.name(),
            payment_arguments(execution)?,
            |_| {
                let result = execution
                    .wallet
                    .prepare_payload(
                        ModuleId::Asset,
                        ordinal,
                        execution.canonical_payload,
                        execution.options,
                    )
                    .map_err(WalletToolError::Rpc)
                    .and_then(|prepared| {
                        let disclosure = prepared.disclosure();
                        let (amount, asset, counterparty) = match tool {
                            PaymentTool::Create => (
                                execution.options.fee_limit,
                                execution.wallet.native_asset,
                                layerx_sdk::rpc::wallet_account(
                                    &execution.options.actor,
                                    execution.wallet.native_asset,
                                    execution.wallet.native_asset,
                                )
                                .map_err(WalletToolError::Rpc)?,
                            ),
                            _ => (
                                disclosure
                                    .amounts
                                    .iter()
                                    .try_fold(0_u128, |sum, a| sum.checked_add(a.value))
                                    .ok_or(WalletToolError::Arguments)?,
                                disclosure.asset,
                                disclosure
                                    .counterparties
                                    .last()
                                    .ok_or(WalletToolError::Arguments)?
                                    .account,
                            ),
                        };
                        if input.request.amount != amount
                            || input.request.asset != asset
                            || input.request.counterparty != counterparty
                            || input.request.activity_type != ordinal
                        {
                            return Err(WalletToolError::Arguments);
                        }
                        let decision = evaluate(execution.policy, input);
                        if decision.outcome != Outcome::Allow {
                            return Err(WalletToolError::Policy);
                        }
                        run(execution.wallet.execute(prepared, execution.options))
                            .map_err(WalletToolError::Rpc)
                    });
                let outcome = classify(&result);
                (result, outcome)
            },
        )
        .map_err(WalletToolError::Server)?
}

/// # Errors
/// Applies the existing scoped read route and preserves unverified RPC read data and errors.
pub fn accounts(
    server: &mut Server,
    core_sequence: u64,
    wallet: &Wallet<'_>,
    did: &str,
) -> Result<RpcValue, WalletToolError> {
    server
        .execute_read(
            core_sequence,
            "wallet.accounts",
            did.as_bytes().to_vec(),
            |_| {
                let result = wallet.accounts(did).map_err(WalletToolError::Rpc);
                let outcome = if result.is_ok() {
                    InvocationOutcome::Completed
                } else {
                    InvocationOutcome::Failed
                };
                (result, outcome)
            },
        )
        .map_err(WalletToolError::Server)?
}

/// # Errors
/// Applies the existing scoped balance route and preserves RPC errors.
pub fn balance(
    server: &mut Server,
    core_sequence: u64,
    wallet: &Wallet<'_>,
    did: &str,
    asset: [u8; 32],
) -> Result<RpcValue, WalletToolError> {
    let mut arguments = did.as_bytes().to_vec();
    arguments.extend_from_slice(&asset);
    server
        .execute_read(core_sequence, "wallet.balance", arguments, |_| {
            let result = wallet.balance(did, asset).map_err(WalletToolError::Rpc);
            let outcome = if result.is_ok() {
                InvocationOutcome::Completed
            } else {
                InvocationOutcome::Failed
            };
            (result, outcome)
        })
        .map_err(WalletToolError::Server)?
}

/// # Errors
/// Retains scoped tracking authorization and requires the requested verified commitment.
pub fn wait(
    server: &mut Server,
    core_sequence: u64,
    wallet: &Wallet<'_>,
    activity: [u8; 32],
    commitment: Commitment,
    timeout: Duration,
) -> Result<VerifiedRpcReceipt, WalletToolError> {
    let mut arguments = activity.to_vec();
    arguments.extend_from_slice(commitment.as_str().as_bytes());
    server
        .execute_committed(core_sequence, "activity.wait", arguments, |_| {
            let result = wallet
                .wait_for(activity, commitment, timeout)
                .map_err(WalletToolError::Rpc);
            let outcome = classify(&result);
            (result, outcome)
        })
        .map_err(WalletToolError::Server)?
}
fn payment_arguments(execution: &PaymentExecution<'_>) -> Result<Vec<u8>, WalletToolError> {
    let payload_length =
        u64::try_from(execution.canonical_payload.len()).map_err(|_| WalletToolError::Arguments)?;
    let actor_length =
        u64::try_from(execution.options.actor.len()).map_err(|_| WalletToolError::Arguments)?;
    let mut arguments = payload_length.to_be_bytes().to_vec();
    arguments.extend_from_slice(execution.canonical_payload);
    arguments.extend_from_slice(&actor_length.to_be_bytes());
    arguments.extend_from_slice(execution.options.actor.as_bytes());
    arguments.extend_from_slice(&execution.options.idempotency_key);
    arguments.extend_from_slice(&execution.options.fee_limit.to_be_bytes());
    arguments.extend_from_slice(&execution.options.not_before.to_be_bytes());
    arguments.extend_from_slice(&execution.options.not_after.to_be_bytes());
    arguments.extend_from_slice(execution.options.commitment.as_str().as_bytes());
    arguments.extend_from_slice(&execution.options.wait_timeout.as_millis().to_be_bytes());
    Ok(arguments)
}

fn classify(result: &Result<VerifiedRpcReceipt, WalletToolError>) -> InvocationOutcome {
    match result {
        Ok(receipt) => {
            if receipt
                .receipt()
                .protocol()
                .is_some_and(|p| p.result_code() == 0)
            {
                InvocationOutcome::Completed
            } else {
                InvocationOutcome::Refused
            }
        }
        Err(WalletToolError::Rpc(RpcError::Pending { .. } | RpcError::Transport)) => {
            InvocationOutcome::Unknown
        }
        Err(WalletToolError::Policy | WalletToolError::Scope | WalletToolError::Arguments) => {
            InvocationOutcome::Refused
        }
        Err(_) => InvocationOutcome::Failed,
    }
}
struct ThreadWake(std::thread::Thread);
impl Wake for ThreadWake {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.unpark();
    }
}
fn run<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
    let mut context = Context::from_waker(&waker);
    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(value) => return value,
            Poll::Pending => std::thread::park(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pending_and_refused_wallet_results_never_complete() {
        assert_eq!(
            classify(&Err(WalletToolError::Rpc(RpcError::Pending {
                activity_id: [1; 32]
            }))),
            InvocationOutcome::Unknown
        );
        assert_eq!(
            classify(&Err(WalletToolError::Policy)),
            InvocationOutcome::Refused
        );
        assert_eq!(
            classify(&Err(WalletToolError::Rpc(RpcError::Verification))),
            InvocationOutcome::Failed
        );
    }
}
