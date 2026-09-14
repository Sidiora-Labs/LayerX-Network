use super::agent_runtime::AgentLifecycleContext;
use super::backend::ApiFailure;
use layerx_intents::{Intent, IntentKind, NativeBudgetAmend};

pub(super) fn intent(
    context: &AgentLifecycleContext,
    amount: u128,
    currency: &str,
    now: u64,
) -> Result<Intent, ApiFailure> {
    if amount == 0
        || amount > context.seed.amount_ceiling
        || context.seed.currency != currency
        || context.state == 4
        || amount < context.spent
        || context.active_budget_id == [0; 32]
    {
        return Err(ApiFailure::invalid_request(Some("monthly_limit")));
    }
    let expiry_ms = context
        .seed
        .period_start
        .checked_add(context.seed.budget_expiry_seconds)
        .and_then(|value| value.checked_mul(1_000))
        .ok_or_else(ApiFailure::upstream_degraded)?;
    let now_ms = now
        .checked_mul(1_000)
        .ok_or_else(ApiFailure::upstream_degraded)?;
    if now_ms >= expiry_ms {
        return Err(ApiFailure::invalid_request(Some("monthly_limit")));
    }
    let amendment = NativeBudgetAmend {
        budget_id: context.active_budget_id,
        per_period_limit: amount,
        carry_cap: 0,
        expiry_ms,
        rollover: 1,
    };
    amendment
        .payload()
        .map_err(|_| ApiFailure::upstream_degraded())?;
    Ok(Intent::v3(IntentKind::NativeBudgetAmend(amendment)))
}
