use std::collections::{BTreeMap, BTreeSet, VecDeque};

use p256::ecdsa::VerifyingKey;
use serde_json::Value;

use crate::error::Ap2Error;
use crate::jose::{
    split_presentation, verify_compact_jws, verify_key_bound, verify_root, KeyResolver, KeyUse,
    VerifiedSdJwt,
};
use crate::model::{
    constraint_type, parse, AgentRecurrence, AllowedMerchants, AllowedPayees,
    AllowedPaymentInstruments, AllowedPisps, AmountRange, Budget, Checkout, ClosedCheckoutMandate,
    ClosedPaymentMandate, ExecutionDate, LineItemsConstraint, OpenCheckoutMandate,
    OpenPaymentMandate, PaymentReference,
};

/// Whether the user directly signed the closed mandates or delegated bounded
/// authority through open mandates to the shopping agent.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MandateMode {
    Direct,
    Autonomous,
}

/// Durable previous-use facts for one open Payment Mandate. The reference is
/// the AP2 `sd_hash` of that exact open mandate, preventing usage facts from
/// being replayed across authorisations.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MandateUsage {
    pub mandate_reference: String,
    pub frequency: String,
    pub previous_occurrences: u64,
    pub spent_minor_units: u128,
    pub next_eligible_at: u64,
}

/// Explicit verification inputs. No verifier reads an ambient clock, nonce,
/// audience, usage ledger or ISO-4217 mapping.
#[derive(Clone, Debug)]
pub struct VerificationContext<'a> {
    pub now: u64,
    pub clock_skew_seconds: u64,
    pub expected_audience: &'a str,
    pub expected_nonce: &'a str,
    pub currency_minor_exponent: u8,
    pub usage: Option<&'a MandateUsage>,
}

impl VerificationContext<'_> {
    fn validate(&self) -> Result<(), Ap2Error> {
        if self.now == 0
            || self.clock_skew_seconds > 3_600
            || self.expected_audience.is_empty()
            || self.expected_audience.len() > 512
            || self.expected_nonce.is_empty()
            || self.expected_nonce.len() > 512
            || self.currency_minor_exponent > 18
        {
            return Err(Ap2Error::Bounds);
        }
        Ok(())
    }
}

/// Signature-verified mandates, merchant checkout and evaluated constraint
/// result. Private token state preserves the exact bindings used for receipt
/// references without exposing raw payment credentials in `Debug`.
pub struct VerifiedMandates {
    mode: MandateMode,
    checkout_token: VerifiedSdJwt,
    payment_token: VerifiedSdJwt,
    open_checkout: Option<VerifiedSdJwt>,
    open_payment: Option<VerifiedSdJwt>,
    payment_mandate: ClosedPaymentMandate,
    checkout: Checkout,
    execution_at: u64,
}

impl VerifiedMandates {
    #[must_use]
    pub const fn mode(&self) -> MandateMode {
        self.mode
    }

    #[must_use]
    pub fn payee(&self) -> &crate::model::Merchant {
        &self.payment_mandate.payee
    }

    #[must_use]
    pub fn amount(&self) -> &crate::model::PaymentAmount {
        &self.payment_mandate.payment_amount
    }

    #[must_use]
    pub fn payment_instrument(&self) -> &crate::model::PaymentInstrument {
        &self.payment_mandate.payment_instrument
    }

    #[must_use]
    pub fn transaction_id(&self) -> &str {
        &self.payment_mandate.transaction_id
    }

    #[must_use]
    pub fn checkout_id(&self) -> &str {
        &self.checkout.id
    }

    #[must_use]
    pub const fn execution_at(&self) -> u64 {
        self.execution_at
    }

    #[must_use]
    pub fn payment_receipt_reference(&self) -> String {
        self.payment_token.receipt_reference()
    }

    #[must_use]
    pub fn checkout_receipt_reference(&self) -> String {
        self.checkout_token.receipt_reference()
    }

    pub(crate) fn stable_payment_reference(&self) -> String {
        self.payment_token.issuer_jwt_hash()
    }

    pub(crate) fn request_material(&self) -> [&[u8]; 4] {
        [
            self.checkout_token.canonical.as_bytes(),
            self.payment_token.canonical.as_bytes(),
            self.open_checkout
                .as_ref()
                .map_or(&[], |token| token.canonical.as_bytes()),
            self.open_payment
                .as_ref()
                .map_or(&[], |token| token.canonical.as_bytes()),
        ]
    }
}

/// Verifies complete AP2 mandate presentations against application trust
/// roots and the caller's explicit time/nonce/usage context.
pub struct MandateVerifier<'a, R> {
    resolver: &'a R,
}

impl<'a, R: KeyResolver> MandateVerifier<'a, R> {
    #[must_use]
    pub const fn new(resolver: &'a R) -> Self {
        Self { resolver }
    }

    /// Verifies signatures and delegation first, then schemas, checkout
    /// binding and every disclosed constraint. No `LayerX` plane callback is
    /// reachable until this function returns a `VerifiedMandates` value.
    ///
    /// # Errors
    ///
    /// Returns a typed cryptographic, schema, binding, time or constraint
    /// refusal.
    #[allow(clippy::too_many_lines)]
    pub fn verify(
        &self,
        checkout_presentation: &str,
        payment_presentation: &str,
        context: &VerificationContext<'_>,
    ) -> Result<VerifiedMandates, Ap2Error> {
        context.validate()?;
        let checkout_segments = split_presentation(checkout_presentation)?;
        let payment_segments = split_presentation(payment_presentation)?;
        if checkout_segments.len() != payment_segments.len() {
            return Err(Ap2Error::UnsupportedDelegation);
        }
        let mode = match checkout_segments.len() {
            1 => MandateMode::Direct,
            2 => MandateMode::Autonomous,
            _ => return Err(Ap2Error::UnsupportedDelegation),
        };

        let (open_checkout, checkout_token) =
            self.verify_chain(&checkout_segments, KeyUse::CheckoutMandateIssuer, context)?;
        let (open_payment, payment_token) =
            self.verify_chain(&payment_segments, KeyUse::PaymentMandateIssuer, context)?;
        let checkout_issuer = open_checkout.as_ref().unwrap_or(&checkout_token);
        let payment_issuer = open_payment.as_ref().unwrap_or(&payment_token);
        if !same_key(
            &checkout_issuer.verification_key,
            &payment_issuer.verification_key,
        ) {
            return Err(Ap2Error::InvalidSignature);
        }
        if let (Some(checkout), Some(payment)) = (&open_checkout, &open_payment) {
            let checkout_key = checkout.confirmation_key()?;
            let payment_key = payment.confirmation_key()?;
            if !same_key(&checkout_key, &payment_key) {
                return Err(Ap2Error::InvalidKeyBinding);
            }
        }

        let checkout_mandate: ClosedCheckoutMandate =
            parse(checkout_token.effective.clone(), "checkout mandate")?;
        checkout_mandate.validate()?;
        let payment_mandate: ClosedPaymentMandate =
            parse(payment_token.effective.clone(), "payment mandate")?;
        payment_mandate.validate()?;
        check_time(
            checkout_mandate.iat,
            checkout_mandate.exp,
            context.now,
            context.clock_skew_seconds,
        )?;
        check_time(
            payment_mandate.iat,
            payment_mandate.exp,
            context.now,
            context.clock_skew_seconds,
        )?;

        let checkout_hash = checkout_token
            .hash_algorithm
            .digest_b64(checkout_mandate.checkout_jwt.as_bytes());
        if checkout_hash != checkout_mandate.checkout_hash {
            return Err(Ap2Error::CheckoutBindingMismatch);
        }
        let transaction_id = payment_token
            .hash_algorithm
            .digest_b64(checkout_mandate.checkout_jwt.as_bytes());
        if transaction_id != payment_mandate.transaction_id {
            return Err(Ap2Error::PaymentBindingMismatch);
        }

        let (_, checkout_value) = verify_compact_jws(
            &checkout_mandate.checkout_jwt,
            KeyUse::MerchantCheckout,
            self.resolver,
        )?;
        let checkout: Checkout = parse(checkout_value, "merchant checkout")?;
        checkout.validate()?;
        verify_checkout_payment_binding(&checkout, &payment_mandate)?;

        if let Some(open) = &open_checkout {
            let mandate: OpenCheckoutMandate =
                parse(open.effective.clone(), "open checkout mandate")?;
            mandate.validate()?;
            check_time(
                mandate.iat,
                mandate.exp,
                context.now,
                context.clock_skew_seconds,
            )?;
            verify_checkout_constraints(&mandate.constraints, &checkout)?;
        }
        if let Some(open) = &open_payment {
            let mandate: OpenPaymentMandate =
                parse(open.effective.clone(), "open payment mandate")?;
            mandate.validate()?;
            check_time(
                mandate.iat,
                mandate.exp,
                context.now,
                context.clock_skew_seconds,
            )?;
            let open_checkout_hash = open_checkout
                .as_ref()
                .ok_or(Ap2Error::InvalidKeyBinding)?
                .sd_hash();
            verify_payment_constraints(
                &mandate,
                &payment_mandate,
                &open_checkout_hash,
                open,
                context,
            )?;
        }

        let execution_at = execution_time(&payment_mandate, context)?;
        Ok(VerifiedMandates {
            mode,
            checkout_token,
            payment_token,
            open_checkout,
            open_payment,
            payment_mandate,
            checkout,
            execution_at,
        })
    }

    fn verify_chain(
        &self,
        segments: &[&str],
        usage: KeyUse,
        context: &VerificationContext<'_>,
    ) -> Result<(Option<VerifiedSdJwt>, VerifiedSdJwt), Ap2Error> {
        let root = verify_root(segments[0], usage, self.resolver)?;
        check_claim_time(&root.payload, context)?;
        if segments.len() == 1 {
            return Ok((None, root));
        }
        let key = root.confirmation_key()?;
        let closed = verify_key_bound(segments[1], &key)?;
        verify_terminal_binding(&root, &closed, context)?;
        Ok((Some(root), closed))
    }
}

fn verify_terminal_binding(
    open: &VerifiedSdJwt,
    closed: &VerifiedSdJwt,
    context: &VerificationContext<'_>,
) -> Result<(), Ap2Error> {
    if !matches!(closed.header.token_type(), Some("kb+sd-jwt" | "kb-sd-jwt")) {
        return Err(Ap2Error::InvalidKeyBinding);
    }
    let payload = closed
        .payload
        .as_object()
        .ok_or(Ap2Error::Malformed("KB-SD-JWT payload"))?;
    let sd_hash = payload.get("sd_hash");
    let issuer_hash = payload.get("issuer_jwt_hash");
    if sd_hash.is_some() == issuer_hash.is_some() {
        return Err(Ap2Error::InvalidKeyBinding);
    }
    let binding = sd_hash
        .map(|value| (value, open.sd_hash()))
        .or_else(|| issuer_hash.map(|value| (value, open.issuer_jwt_hash())))
        .ok_or(Ap2Error::InvalidKeyBinding)?;
    if binding.0.as_str() != Some(&binding.1) {
        return Err(Ap2Error::InvalidKeyBinding);
    }
    if payload.get("aud").and_then(Value::as_str) != Some(context.expected_audience) {
        return Err(Ap2Error::AudienceMismatch);
    }
    if payload.get("nonce").and_then(Value::as_str) != Some(context.expected_nonce) {
        return Err(Ap2Error::NonceMismatch);
    }
    if closed.effective.get("cnf").is_some() {
        return Err(Ap2Error::InvalidKeyBinding);
    }
    check_claim_time(&closed.payload, context)
}

fn check_claim_time(value: &Value, context: &VerificationContext<'_>) -> Result<(), Ap2Error> {
    let iat = value.get("iat").and_then(Value::as_u64);
    let exp = value.get("exp").and_then(Value::as_u64);
    check_time(iat, exp, context.now, context.clock_skew_seconds)
}

fn check_time(iat: Option<u64>, exp: Option<u64>, now: u64, skew: u64) -> Result<(), Ap2Error> {
    if iat.is_some_and(|issued| issued > now.saturating_add(skew)) {
        return Err(Ap2Error::NotYetValid);
    }
    if exp.is_some_and(|expires| expires.saturating_add(skew) < now) {
        return Err(Ap2Error::Expired);
    }
    if iat
        .zip(exp)
        .is_some_and(|(issued, expires)| expires <= issued)
    {
        return Err(Ap2Error::Malformed("mandate time window"));
    }
    Ok(())
}

fn verify_checkout_payment_binding(
    checkout: &Checkout,
    payment: &ClosedPaymentMandate,
) -> Result<(), Ap2Error> {
    let merchant = checkout
        .merchant
        .as_ref()
        .ok_or(Ap2Error::CheckoutBindingMismatch)?;
    if !merchant.matches(&payment.payee)
        || checkout.currency != payment.payment_amount.currency
        || checkout.final_total()? != payment.payment_amount.amount
    {
        return Err(Ap2Error::PaymentBindingMismatch);
    }
    Ok(())
}

fn verify_checkout_constraints(values: &[Value], checkout: &Checkout) -> Result<(), Ap2Error> {
    let mut line_items_seen = false;
    for value in values {
        match constraint_type(value)? {
            "checkout.allowed_merchants" => {
                let constraint: AllowedMerchants = parse(value.clone(), "allowed merchants")?;
                if constraint.allowed.is_empty() || constraint.allowed.len() > 512 {
                    return Err(Ap2Error::ConstraintViolated("checkout.allowed_merchants"));
                }
                for merchant in &constraint.allowed {
                    merchant.validate()?;
                }
                let checkout_merchant = checkout
                    .merchant
                    .as_ref()
                    .ok_or(Ap2Error::ConstraintViolated("checkout.allowed_merchants"))?;
                if !constraint
                    .allowed
                    .iter()
                    .any(|allowed| allowed.matches(checkout_merchant))
                {
                    return Err(Ap2Error::ConstraintViolated("checkout.allowed_merchants"));
                }
            }
            "checkout.line_items" => {
                if line_items_seen {
                    return Err(Ap2Error::ConstraintViolated("checkout.line_items"));
                }
                line_items_seen = true;
                let constraint: LineItemsConstraint = parse(value.clone(), "line items")?;
                verify_line_items(&constraint, checkout)?;
            }
            _ => return Err(Ap2Error::ConstraintUnsupported),
        }
    }
    if !line_items_seen {
        return Err(Ap2Error::ConstraintMissing("checkout.line_items"));
    }
    Ok(())
}

fn verify_line_items(
    constraint: &LineItemsConstraint,
    checkout: &Checkout,
) -> Result<(), Ap2Error> {
    if constraint.items.is_empty() || constraint.items.len() > 512 {
        return Err(Ap2Error::ConstraintViolated("checkout.line_items"));
    }
    let mut cart = BTreeMap::<String, u128>::new();
    for item in &checkout.line_items {
        let quantity = u128::from(item.quantity);
        let current = cart.entry(item.item.id.clone()).or_default();
        *current = current
            .checked_add(quantity)
            .ok_or(Ap2Error::ConstraintViolated("checkout.line_items"))?;
    }
    let skus: Vec<_> = cart.keys().cloned().collect();
    let source = 0;
    let requirement_offset = 1;
    let sku_offset = requirement_offset + constraint.items.len();
    let sink = sku_offset + skus.len();
    let mut graph = FlowGraph::new(sink + 1);
    let mut required_total = 0_u128;
    for (index, requirement) in constraint.items.iter().enumerate() {
        if requirement.id.is_empty()
            || requirement.id.len() > 1_024
            || requirement.quantity == 0
            || requirement.acceptable_items.is_empty()
            || requirement.acceptable_items.len() > 512
        {
            return Err(Ap2Error::ConstraintViolated("checkout.line_items"));
        }
        let quantity = u128::from(requirement.quantity);
        required_total = required_total
            .checked_add(quantity)
            .ok_or(Ap2Error::ConstraintViolated("checkout.line_items"))?;
        graph.add_edge(source, requirement_offset + index, quantity);
        let acceptable: BTreeSet<_> = requirement
            .acceptable_items
            .iter()
            .map(|item| {
                if item.id.is_empty()
                    || item.id.len() > 1_024
                    || item.title.is_empty()
                    || item.title.len() > 1_024
                {
                    Err(Ap2Error::ConstraintViolated("checkout.line_items"))
                } else {
                    Ok(item.id.as_str())
                }
            })
            .collect::<Result<_, _>>()?;
        for (sku_index, sku) in skus.iter().enumerate() {
            if acceptable.contains(sku.as_str()) {
                graph.add_edge(requirement_offset + index, sku_offset + sku_index, quantity);
            }
        }
    }
    let mut cart_total = 0_u128;
    for (index, sku) in skus.iter().enumerate() {
        let quantity = cart[sku];
        cart_total = cart_total
            .checked_add(quantity)
            .ok_or(Ap2Error::ConstraintViolated("checkout.line_items"))?;
        graph.add_edge(sku_offset + index, sink, quantity);
    }
    if cart_total != required_total || graph.maximum_flow(source, sink) != required_total {
        return Err(Ap2Error::ConstraintViolated("checkout.line_items"));
    }
    Ok(())
}

#[allow(clippy::too_many_lines)]
fn verify_payment_constraints(
    open: &OpenPaymentMandate,
    closed: &ClosedPaymentMandate,
    open_checkout_hash: &str,
    open_token: &VerifiedSdJwt,
    context: &VerificationContext<'_>,
) -> Result<(), Ap2Error> {
    verify_preset_payment(open, closed)?;
    let mut has_reference = false;
    let mut has_recurrence = false;
    let mut has_amount_range = false;
    let mut has_budget = false;
    for value in &open.constraints {
        match constraint_type(value)? {
            "payment.allowed_payees" => {
                let constraint: AllowedPayees = parse(value.clone(), "allowed payees")?;
                if constraint.allowed.is_empty()
                    || constraint.allowed.len() > 512
                    || constraint
                        .allowed
                        .iter()
                        .any(|merchant| merchant.validate().is_err())
                    || !constraint
                        .allowed
                        .iter()
                        .any(|merchant| merchant.matches(&closed.payee))
                {
                    return Err(Ap2Error::ConstraintViolated("payment.allowed_payees"));
                }
            }
            "payment.allowed_payment_instruments" => {
                let constraint: AllowedPaymentInstruments =
                    parse(value.clone(), "allowed payment instruments")?;
                if constraint.allowed.is_empty()
                    || constraint.allowed.len() > 512
                    || constraint
                        .allowed
                        .iter()
                        .any(|instrument| instrument.validate().is_err())
                    || !constraint.allowed.iter().any(|instrument| {
                        instrument.id == closed.payment_instrument.id
                            && instrument.kind == closed.payment_instrument.kind
                    })
                {
                    return Err(Ap2Error::ConstraintViolated(
                        "payment.allowed_payment_instruments",
                    ));
                }
            }
            "payment.allowed_pisps" => {
                let constraint: AllowedPisps = parse(value.clone(), "allowed PISPs")?;
                let actual = closed
                    .pisp
                    .as_ref()
                    .ok_or(Ap2Error::ConstraintViolated("payment.allowed_pisps"))?;
                if constraint.allowed.is_empty()
                    || constraint.allowed.len() > 512
                    || constraint
                        .allowed
                        .iter()
                        .any(|pisp| pisp.validate().is_err())
                    || !constraint.allowed.iter().any(|allowed| {
                        allowed.legal == actual.legal
                            && allowed.brand == actual.brand
                            && allowed.domain == actual.domain
                    })
                {
                    return Err(Ap2Error::ConstraintViolated("payment.allowed_pisps"));
                }
            }
            "payment.amount_range" => {
                has_amount_range = true;
                let constraint: AmountRange = parse(value.clone(), "amount range")?;
                if constraint.currency != closed.payment_amount.currency
                    || constraint.max == 0
                    || constraint
                        .min
                        .is_some_and(|minimum| minimum > constraint.max)
                    || closed.payment_amount.amount > constraint.max
                    || constraint
                        .min
                        .is_some_and(|minimum| closed.payment_amount.amount < minimum)
                {
                    return Err(Ap2Error::ConstraintViolated("payment.amount_range"));
                }
            }
            "payment.reference" => {
                if has_reference {
                    return Err(Ap2Error::ConstraintViolated("payment.reference"));
                }
                has_reference = true;
                let constraint: PaymentReference = parse(value.clone(), "payment reference")?;
                if constraint.conditional_transaction_id != open_checkout_hash {
                    return Err(Ap2Error::ConstraintViolated("payment.reference"));
                }
            }
            "payment.agent_recurrence" => {
                has_recurrence = true;
                let recurrence: AgentRecurrence = parse(value.clone(), "agent recurrence")?;
                verify_recurrence(&recurrence, open_token, context)?;
            }
            "payment.budget" => {
                has_budget = true;
                let budget: Budget = parse(value.clone(), "budget")?;
                verify_budget(&budget, closed, open_token, context)?;
            }
            "payment.execution_date" => {
                let constraint: ExecutionDate = parse(value.clone(), "execution date")?;
                verify_execution_date(&constraint, closed, context)?;
            }
            _ => return Err(Ap2Error::ConstraintUnsupported),
        }
    }
    if !has_reference {
        return Err(Ap2Error::ConstraintMissing("payment.reference"));
    }
    if has_recurrence && (!has_amount_range || !has_budget) {
        return Err(Ap2Error::ConstraintMissing(
            "recurrence amount_range and budget",
        ));
    }
    if has_budget && !has_recurrence {
        return Err(Ap2Error::ConstraintMissing("payment.agent_recurrence"));
    }
    Ok(())
}

fn verify_preset_payment(
    open: &OpenPaymentMandate,
    closed: &ClosedPaymentMandate,
) -> Result<(), Ap2Error> {
    if open
        .payee
        .as_ref()
        .is_some_and(|payee| !payee.matches(&closed.payee))
        || open
            .payment_amount
            .as_ref()
            .is_some_and(|amount| amount != &closed.payment_amount)
        || open
            .payment_instrument
            .as_ref()
            .is_some_and(|instrument| instrument != &closed.payment_instrument)
        || open.pisp.as_ref().is_some_and(|pisp| {
            closed.pisp.as_ref().is_none_or(|closed_pisp| {
                pisp.legal != closed_pisp.legal
                    || pisp.brand != closed_pisp.brand
                    || pisp.domain != closed_pisp.domain
            })
        })
        || open.execution_date != closed.execution_date && open.execution_date.is_some()
    {
        return Err(Ap2Error::ConstraintViolated("payment preset claim"));
    }
    Ok(())
}

fn verify_recurrence(
    recurrence: &AgentRecurrence,
    open_token: &VerifiedSdJwt,
    context: &VerificationContext<'_>,
) -> Result<(), Ap2Error> {
    if !matches!(
        recurrence.frequency.as_str(),
        "ON_DEMAND" | "DAILY" | "WEEKLY" | "BIWEEKLY" | "MONTHLY" | "QUARTERLY" | "ANNUALLY"
    ) {
        return Err(Ap2Error::ConstraintUnsupported);
    }
    let usage = verified_usage(open_token, context)?;
    if usage.frequency != recurrence.frequency
        || usage.next_eligible_at > context.now.saturating_add(context.clock_skew_seconds)
        || recurrence
            .max_occurrences
            .is_some_and(|maximum| usage.previous_occurrences >= maximum)
    {
        return Err(Ap2Error::ConstraintViolated("payment.agent_recurrence"));
    }
    Ok(())
}

fn verify_budget(
    budget: &Budget,
    closed: &ClosedPaymentMandate,
    open_token: &VerifiedSdJwt,
    context: &VerificationContext<'_>,
) -> Result<(), Ap2Error> {
    if budget.currency != closed.payment_amount.currency {
        return Err(Ap2Error::ConstraintViolated("payment.budget"));
    }
    let maximum = decimal_major_to_minor(&budget.max, context.currency_minor_exponent)?;
    let usage = verified_usage(open_token, context)?;
    let after = usage
        .spent_minor_units
        .checked_add(closed.payment_amount.amount)
        .ok_or(Ap2Error::ConstraintViolated("payment.budget"))?;
    if after > maximum {
        return Err(Ap2Error::ConstraintViolated("payment.budget"));
    }
    Ok(())
}

fn verified_usage<'a>(
    open_token: &VerifiedSdJwt,
    context: &'a VerificationContext<'a>,
) -> Result<&'a MandateUsage, Ap2Error> {
    let usage = context.usage.ok_or(Ap2Error::UsageEvidenceRequired)?;
    if usage.mandate_reference != open_token.sd_hash() {
        return Err(Ap2Error::UsageEvidenceRequired);
    }
    Ok(usage)
}

fn verify_execution_date(
    constraint: &ExecutionDate,
    closed: &ClosedPaymentMandate,
    context: &VerificationContext<'_>,
) -> Result<(), Ap2Error> {
    let execution = execution_time(closed, context)?;
    if let Some(not_before) = &constraint.not_before {
        if execution < parse_rfc3339(not_before)? {
            return Err(Ap2Error::ConstraintViolated("payment.execution_date"));
        }
    }
    if let Some(not_after) = &constraint.not_after {
        if execution > parse_rfc3339(not_after)? {
            return Err(Ap2Error::ConstraintViolated("payment.execution_date"));
        }
    }
    Ok(())
}

fn execution_time(
    closed: &ClosedPaymentMandate,
    context: &VerificationContext<'_>,
) -> Result<u64, Ap2Error> {
    let execution = closed
        .execution_date
        .as_deref()
        .map(parse_rfc3339)
        .transpose()?
        .unwrap_or(context.now);
    let lower = context.now.saturating_sub(context.clock_skew_seconds);
    let upper = context.now.saturating_add(context.clock_skew_seconds);
    if !(lower..=upper).contains(&execution) {
        return Err(Ap2Error::ConstraintUnsupported);
    }
    Ok(execution)
}

fn decimal_major_to_minor(value: &serde_json::Number, exponent: u8) -> Result<u128, Ap2Error> {
    let rendered = value.to_string();
    if rendered.starts_with('-') || rendered.contains(['e', 'E']) {
        return Err(Ap2Error::ConstraintUnsupported);
    }
    let (whole, fractional) = rendered.split_once('.').unwrap_or((&rendered, ""));
    if whole.is_empty()
        || !whole.bytes().all(|byte| byte.is_ascii_digit())
        || !fractional.bytes().all(|byte| byte.is_ascii_digit())
        || fractional.len() > usize::from(exponent)
    {
        return Err(Ap2Error::ConstraintUnsupported);
    }
    let scale = 10_u128
        .checked_pow(u32::from(exponent))
        .ok_or(Ap2Error::ConstraintUnsupported)?;
    let whole = whole
        .parse::<u128>()
        .map_err(|_| Ap2Error::ConstraintUnsupported)?;
    let mut fraction = fractional.parse::<u128>().unwrap_or(0);
    for _ in fractional.len()..usize::from(exponent) {
        fraction = fraction
            .checked_mul(10)
            .ok_or(Ap2Error::ConstraintUnsupported)?;
    }
    whole
        .checked_mul(scale)
        .and_then(|scaled| scaled.checked_add(fraction))
        .ok_or(Ap2Error::ConstraintUnsupported)
}

#[derive(Clone, Debug)]
struct Edge {
    to: usize,
    reverse: usize,
    capacity: u128,
}

struct FlowGraph {
    edges: Vec<Vec<Edge>>,
}

impl FlowGraph {
    fn new(nodes: usize) -> Self {
        Self {
            edges: vec![Vec::new(); nodes],
        }
    }

    fn add_edge(&mut self, from: usize, to: usize, capacity: u128) {
        let reverse_to = self.edges[to].len();
        let reverse_from = self.edges[from].len();
        self.edges[from].push(Edge {
            to,
            reverse: reverse_to,
            capacity,
        });
        self.edges[to].push(Edge {
            to: from,
            reverse: reverse_from,
            capacity: 0,
        });
    }

    fn maximum_flow(&mut self, source: usize, sink: usize) -> u128 {
        let mut total = 0_u128;
        loop {
            let mut parent: Vec<Option<(usize, usize)>> = vec![None; self.edges.len()];
            let mut queue = VecDeque::from([source]);
            parent[source] = Some((source, 0));
            while let Some(node) = queue.pop_front() {
                for (index, edge) in self.edges[node].iter().enumerate() {
                    if edge.capacity > 0 && parent[edge.to].is_none() {
                        parent[edge.to] = Some((node, index));
                        queue.push_back(edge.to);
                    }
                }
            }
            if parent[sink].is_none() {
                return total;
            }
            let mut pushed = u128::MAX;
            let mut node = sink;
            while node != source {
                let Some((previous, edge)) = parent[node] else {
                    return total;
                };
                pushed = pushed.min(self.edges[previous][edge].capacity);
                node = previous;
            }
            node = sink;
            while node != source {
                let Some((previous, edge_index)) = parent[node] else {
                    return total;
                };
                let reverse = self.edges[previous][edge_index].reverse;
                self.edges[previous][edge_index].capacity -= pushed;
                self.edges[node][reverse].capacity += pushed;
                node = previous;
            }
            total += pushed;
        }
    }
}

fn parse_rfc3339(value: &str) -> Result<u64, Ap2Error> {
    if value.len() < 20 || !value.is_ascii() {
        return Err(Ap2Error::Malformed("execution_date"));
    }
    let bytes = value.as_bytes();
    if bytes.get(4) != Some(&b'-')
        || bytes.get(7) != Some(&b'-')
        || bytes.get(10) != Some(&b'T')
        || bytes.get(13) != Some(&b':')
        || bytes.get(16) != Some(&b':')
    {
        return Err(Ap2Error::Malformed("execution_date"));
    }
    let year = digits(bytes, 0, 4)?;
    let month = digits(bytes, 5, 2)?;
    let day = digits(bytes, 8, 2)?;
    let hour = digits(bytes, 11, 2)?;
    let minute = digits(bytes, 14, 2)?;
    let second = digits(bytes, 17, 2)?;
    if year < 1970
        || !(1..=12).contains(&month)
        || day == 0
        || day > days_in_month(year, month)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return Err(Ap2Error::Malformed("execution_date"));
    }
    let mut index = 19;
    if bytes.get(index) == Some(&b'.') {
        index += 1;
        let start = index;
        while bytes.get(index).is_some_and(u8::is_ascii_digit) {
            index += 1;
        }
        if index == start {
            return Err(Ap2Error::Malformed("execution_date"));
        }
    }
    let offset = match bytes.get(index) {
        Some(b'Z') if index + 1 == bytes.len() => 0_i64,
        Some(sign @ (b'+' | b'-')) if index + 6 == bytes.len() => {
            if bytes.get(index + 3) != Some(&b':') {
                return Err(Ap2Error::Malformed("execution_date"));
            }
            let hours = i64::from(digits(bytes, index + 1, 2)?);
            let minutes = i64::from(digits(bytes, index + 4, 2)?);
            if hours > 23 || minutes > 59 {
                return Err(Ap2Error::Malformed("execution_date"));
            }
            let seconds = hours * 3_600 + minutes * 60;
            if *sign == b'+' {
                seconds
            } else {
                -seconds
            }
        }
        _ => return Err(Ap2Error::Malformed("execution_date")),
    };
    let days = days_from_civil(i64::from(year), i64::from(month), i64::from(day));
    let local = days
        .checked_mul(86_400)
        .and_then(|value| value.checked_add(i64::from(hour) * 3_600))
        .and_then(|value| value.checked_add(i64::from(minute) * 60))
        .and_then(|value| value.checked_add(i64::from(second)))
        .ok_or(Ap2Error::Malformed("execution_date"))?;
    u64::try_from(local - offset).map_err(|_| Ap2Error::Malformed("execution_date"))
}

fn digits(value: &[u8], start: usize, length: usize) -> Result<u32, Ap2Error> {
    let bytes = value
        .get(start..start + length)
        .ok_or(Ap2Error::Malformed("execution_date"))?;
    if !bytes.iter().all(u8::is_ascii_digit) {
        return Err(Ap2Error::Malformed("execution_date"));
    }
    bytes.iter().try_fold(0_u32, |total, byte| {
        total
            .checked_mul(10)
            .and_then(|value| value.checked_add(u32::from(byte - b'0')))
            .ok_or(Ap2Error::Malformed("execution_date"))
    })
}

const fn days_in_month(year: u32, month: u32) -> u32 {
    match month {
        2 if year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400)) => {
            29
        }
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

const fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let adjusted_year = year - if month <= 2 { 1 } else { 0 };
    let era = if adjusted_year >= 0 {
        adjusted_year
    } else {
        adjusted_year - 399
    } / 400;
    let year_of_era = adjusted_year - era * 400;
    let shifted_month = month + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * shifted_month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

fn same_key(left: &VerifyingKey, right: &VerifyingKey) -> bool {
    left.to_sec1_point(false) == right.to_sec1_point(false)
}
