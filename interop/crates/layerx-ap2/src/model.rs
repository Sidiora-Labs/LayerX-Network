use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{Number, Value};

use crate::error::Ap2Error;

const TEXT_LIMIT: usize = 1_024;
const URI_LIMIT: usize = 2_048;
const COLLECTION_LIMIT: usize = 512;

#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
pub struct Merchant {
    pub(crate) id: String,
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) website: Option<String>,
}

impl Merchant {
    /// Declares one deployment-pinned merchant identity used to bind verified
    /// mandate payees to execution policy.
    ///
    /// # Errors
    ///
    /// Refuses empty, oversize and control-character fields.
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        website: Option<String>,
    ) -> Result<Self, Ap2Error> {
        let merchant = Self {
            id: id.into(),
            name: name.into(),
            website,
        };
        merchant.validate()?;
        Ok(merchant)
    }

    pub(crate) fn validate(&self) -> Result<(), Ap2Error> {
        bounded(&self.id, TEXT_LIMIT)?;
        bounded(&self.name, TEXT_LIMIT)?;
        if let Some(website) = &self.website {
            bounded(website, URI_LIMIT)?;
        }
        Ok(())
    }

    pub(crate) fn matches(&self, other: &Self) -> bool {
        if !self.id.is_empty() && !other.id.is_empty() {
            self.id == other.id
        } else {
            self.name == other.name
                && self
                    .website
                    .as_ref()
                    .zip(other.website.as_ref())
                    .is_some_and(|(left, right)| left == right)
        }
    }

    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn website(&self) -> Option<&str> {
        self.website.as_deref()
    }
}

#[derive(Clone, Eq, PartialEq, Deserialize)]
pub struct PaymentAmount {
    pub(crate) amount: u128,
    pub(crate) currency: String,
}

impl PaymentAmount {
    pub(crate) fn validate(&self) -> Result<(), Ap2Error> {
        if self.amount == 0 || !currency(&self.currency) {
            return Err(Ap2Error::Malformed("payment_amount"));
        }
        Ok(())
    }

    #[must_use]
    pub const fn minor_units(&self) -> u128 {
        self.amount
    }

    #[must_use]
    pub fn currency(&self) -> &str {
        &self.currency
    }
}

#[derive(Clone, Eq, PartialEq, Deserialize)]
pub struct PaymentInstrument {
    pub(crate) id: String,
    #[serde(rename = "type")]
    pub(crate) kind: String,
    #[serde(default)]
    pub(crate) description: Option<String>,
}

impl PaymentInstrument {
    pub(crate) fn validate(&self) -> Result<(), Ap2Error> {
        bounded(&self.id, TEXT_LIMIT)?;
        bounded(&self.kind, TEXT_LIMIT)?;
        if self
            .description
            .as_ref()
            .is_some_and(|description| !valid_bounded(description, TEXT_LIMIT))
        {
            return Err(Ap2Error::Bounds);
        }
        Ok(())
    }

    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    #[must_use]
    pub fn kind(&self) -> &str {
        &self.kind
    }
}

#[derive(Clone, Eq, PartialEq, Deserialize)]
pub struct Pisp {
    #[serde(rename = "legal_name")]
    pub(crate) legal: String,
    #[serde(rename = "brand_name")]
    pub(crate) brand: String,
    #[serde(rename = "domain_name")]
    pub(crate) domain: String,
}

impl Pisp {
    pub(crate) fn validate(&self) -> Result<(), Ap2Error> {
        bounded(&self.legal, TEXT_LIMIT)?;
        bounded(&self.brand, TEXT_LIMIT)?;
        bounded(&self.domain, TEXT_LIMIT)
    }
}

#[derive(Clone, Deserialize)]
pub(crate) struct ClosedCheckoutMandate {
    pub(crate) vct: String,
    pub(crate) checkout_jwt: String,
    pub(crate) checkout_hash: String,
    #[serde(default)]
    pub(crate) iat: Option<u64>,
    #[serde(default)]
    pub(crate) exp: Option<u64>,
}

impl ClosedCheckoutMandate {
    pub(crate) fn validate(&self) -> Result<(), Ap2Error> {
        version(&self.vct, "mandate.checkout.1")?;
        bounded(&self.checkout_jwt, 128 * 1_024)?;
        digest_text(&self.checkout_hash)
    }
}

#[derive(Clone, Deserialize)]
pub(crate) struct ClosedPaymentMandate {
    pub(crate) vct: String,
    pub(crate) transaction_id: String,
    pub(crate) payee: Merchant,
    #[serde(default)]
    pub(crate) pisp: Option<Pisp>,
    pub(crate) payment_amount: PaymentAmount,
    pub(crate) payment_instrument: PaymentInstrument,
    #[serde(default)]
    pub(crate) execution_date: Option<String>,
    #[serde(default)]
    pub(crate) iat: Option<u64>,
    #[serde(default)]
    pub(crate) exp: Option<u64>,
}

impl ClosedPaymentMandate {
    pub(crate) fn validate(&self) -> Result<(), Ap2Error> {
        version(&self.vct, "mandate.payment.1")?;
        digest_text(&self.transaction_id)?;
        self.payee.validate()?;
        self.payment_amount.validate()?;
        self.payment_instrument.validate()?;
        if let Some(pisp) = &self.pisp {
            pisp.validate()?;
        }
        if self
            .execution_date
            .as_ref()
            .is_some_and(|date| !valid_bounded(date, 64))
        {
            return Err(Ap2Error::Malformed("execution_date"));
        }
        Ok(())
    }
}

#[derive(Clone, Deserialize)]
pub(crate) struct OpenCheckoutMandate {
    pub(crate) vct: String,
    pub(crate) constraints: Vec<Value>,
    pub(crate) cnf: Value,
    #[serde(default)]
    pub(crate) iat: Option<u64>,
    #[serde(default)]
    pub(crate) exp: Option<u64>,
}

impl OpenCheckoutMandate {
    pub(crate) fn validate(&self) -> Result<(), Ap2Error> {
        version(&self.vct, "mandate.checkout.open.1")?;
        collection(&self.constraints)?;
        if !self.cnf.is_object() {
            return Err(Ap2Error::InvalidKeyBinding);
        }
        Ok(())
    }
}

#[derive(Clone, Deserialize)]
pub(crate) struct OpenPaymentMandate {
    pub(crate) vct: String,
    pub(crate) constraints: Vec<Value>,
    pub(crate) cnf: Value,
    #[serde(default)]
    pub(crate) payee: Option<Merchant>,
    #[serde(default)]
    pub(crate) payment_amount: Option<PaymentAmount>,
    #[serde(default)]
    pub(crate) payment_instrument: Option<PaymentInstrument>,
    #[serde(default)]
    pub(crate) pisp: Option<Pisp>,
    #[serde(default)]
    pub(crate) execution_date: Option<String>,
    #[serde(default)]
    pub(crate) iat: Option<u64>,
    #[serde(default)]
    pub(crate) exp: Option<u64>,
}

impl OpenPaymentMandate {
    pub(crate) fn validate(&self) -> Result<(), Ap2Error> {
        version(&self.vct, "mandate.payment.open.1")?;
        collection(&self.constraints)?;
        if !self.cnf.is_object() {
            return Err(Ap2Error::InvalidKeyBinding);
        }
        if let Some(payee) = &self.payee {
            payee.validate()?;
        }
        if let Some(amount) = &self.payment_amount {
            amount.validate()?;
        }
        if let Some(instrument) = &self.payment_instrument {
            instrument.validate()?;
        }
        if let Some(pisp) = &self.pisp {
            pisp.validate()?;
        }
        if self
            .execution_date
            .as_ref()
            .is_some_and(|date| !valid_bounded(date, 64))
        {
            return Err(Ap2Error::Malformed("execution_date"));
        }
        Ok(())
    }
}

#[derive(Clone, Deserialize)]
pub(crate) struct Checkout {
    pub(crate) id: String,
    #[serde(default)]
    pub(crate) merchant: Option<Merchant>,
    pub(crate) line_items: Vec<CheckoutLineItem>,
    pub(crate) status: String,
    pub(crate) currency: String,
    pub(crate) totals: Vec<Total>,
    pub(crate) links: Vec<Value>,
}

impl Checkout {
    pub(crate) fn validate(&self) -> Result<(), Ap2Error> {
        bounded(&self.id, TEXT_LIMIT)?;
        if let Some(merchant) = &self.merchant {
            merchant.validate()?;
        }
        collection(&self.line_items)?;
        collection(&self.totals)?;
        if self.links.len() > COLLECTION_LIMIT
            || !matches!(
                self.status.as_str(),
                "incomplete"
                    | "requires_escalation"
                    | "ready_for_complete"
                    | "complete_in_progress"
                    | "completed"
                    | "canceled"
            )
            || !currency(&self.currency)
        {
            return Err(Ap2Error::Malformed("checkout"));
        }
        for item in &self.line_items {
            item.validate()?;
        }
        for total in &self.totals {
            total.validate()?;
        }
        Ok(())
    }

    pub(crate) fn final_total(&self) -> Result<u128, Ap2Error> {
        let mut totals = self.totals.iter().filter(|total| total.kind == "total");
        let total = totals.next().ok_or(Ap2Error::PaymentBindingMismatch)?;
        if totals.next().is_some() || total.amount <= 0 {
            return Err(Ap2Error::PaymentBindingMismatch);
        }
        u128::try_from(total.amount).map_err(|_| Ap2Error::PaymentBindingMismatch)
    }
}

#[derive(Clone, Deserialize)]
pub(crate) struct CheckoutLineItem {
    pub(crate) id: String,
    pub(crate) item: Item,
    pub(crate) quantity: u64,
    pub(crate) totals: Vec<Total>,
}

impl CheckoutLineItem {
    fn validate(&self) -> Result<(), Ap2Error> {
        bounded(&self.id, TEXT_LIMIT)?;
        self.item.validate()?;
        if self.quantity == 0 {
            return Err(Ap2Error::Malformed("line item quantity"));
        }
        collection(&self.totals)?;
        for total in &self.totals {
            total.validate()?;
        }
        Ok(())
    }
}

#[derive(Clone, Deserialize)]
pub(crate) struct Item {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) price: u128,
}

impl Item {
    fn validate(&self) -> Result<(), Ap2Error> {
        bounded(&self.id, TEXT_LIMIT)?;
        bounded(&self.title, TEXT_LIMIT)?;
        let _ = self.price;
        Ok(())
    }
}

#[derive(Clone, Deserialize)]
pub(crate) struct Total {
    #[serde(rename = "type")]
    pub(crate) kind: String,
    pub(crate) amount: i128,
}

impl Total {
    fn validate(&self) -> Result<(), Ap2Error> {
        bounded(&self.kind, 64)
    }
}

#[derive(Clone, Deserialize)]
pub(crate) struct AllowedMerchants {
    pub(crate) allowed: Vec<Merchant>,
}

#[derive(Clone, Deserialize)]
pub(crate) struct LineItemsConstraint {
    pub(crate) items: Vec<LineItemRequirement>,
}

#[derive(Clone, Deserialize)]
pub(crate) struct LineItemRequirement {
    pub(crate) id: String,
    pub(crate) acceptable_items: Vec<ConstraintItem>,
    pub(crate) quantity: u64,
}

#[derive(Clone, Deserialize)]
pub(crate) struct ConstraintItem {
    pub(crate) id: String,
    pub(crate) title: String,
}

#[derive(Clone, Deserialize)]
pub(crate) struct AmountRange {
    pub(crate) currency: String,
    pub(crate) max: u128,
    #[serde(default)]
    pub(crate) min: Option<u128>,
}

#[derive(Clone, Deserialize)]
pub(crate) struct AllowedPayees {
    pub(crate) allowed: Vec<Merchant>,
}

#[derive(Clone, Deserialize)]
pub(crate) struct AllowedPaymentInstruments {
    pub(crate) allowed: Vec<PaymentInstrument>,
}

#[derive(Clone, Deserialize)]
pub(crate) struct AllowedPisps {
    pub(crate) allowed: Vec<Pisp>,
}

#[derive(Clone, Deserialize)]
pub(crate) struct PaymentReference {
    pub(crate) conditional_transaction_id: String,
}

#[derive(Clone, Deserialize)]
pub(crate) struct AgentRecurrence {
    pub(crate) frequency: String,
    #[serde(default)]
    pub(crate) max_occurrences: Option<u64>,
}

#[derive(Clone, Deserialize)]
pub(crate) struct Budget {
    pub(crate) max: Number,
    pub(crate) currency: String,
}

#[derive(Clone, Deserialize)]
pub(crate) struct ExecutionDate {
    #[serde(default)]
    pub(crate) not_before: Option<String>,
    #[serde(default)]
    pub(crate) not_after: Option<String>,
}

pub(crate) fn parse<T: DeserializeOwned>(value: Value, field: &'static str) -> Result<T, Ap2Error> {
    serde_json::from_value(value).map_err(|_| Ap2Error::Malformed(field))
}

pub(crate) fn constraint_type(value: &Value) -> Result<&str, Ap2Error> {
    value
        .get("type")
        .and_then(Value::as_str)
        .filter(|value| valid_bounded(value, 96))
        .ok_or(Ap2Error::Malformed("constraint type"))
}

fn collection<T>(value: &[T]) -> Result<(), Ap2Error> {
    if value.is_empty() || value.len() > COLLECTION_LIMIT {
        Err(Ap2Error::Bounds)
    } else {
        Ok(())
    }
}

fn digest_text(value: &str) -> Result<(), Ap2Error> {
    if value.len() < 43 || value.len() > 86 || !value.bytes().all(is_base64url) {
        Err(Ap2Error::Malformed("digest"))
    } else {
        Ok(())
    }
}

fn version(value: &str, expected: &str) -> Result<(), Ap2Error> {
    if value == expected {
        Ok(())
    } else {
        Err(Ap2Error::UnsupportedMandateVersion)
    }
}

fn currency(value: &str) -> bool {
    value.len() == 3 && value.bytes().all(|byte| byte.is_ascii_uppercase())
}

fn bounded(value: &str, limit: usize) -> Result<(), Ap2Error> {
    if valid_bounded(value, limit) {
        Ok(())
    } else {
        Err(Ap2Error::Bounds)
    }
}

fn valid_bounded(value: &str, limit: usize) -> bool {
    !value.is_empty() && value.len() <= limit && !value.bytes().any(|byte| byte.is_ascii_control())
}

fn is_base64url(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_'
}
