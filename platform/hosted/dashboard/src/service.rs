use layerx_platform_webhooks::deliveries::DeliveryRecord;
use layerx_platform_webhooks::endpoints::EndpointHealth;
use layerx_platform_webhooks::events::{EndpointId, EventKind, Principal};
use layerx_platform_webhooks::HostedReader;

use crate::error::DashboardError;
use crate::gateway::Store;
use crate::model::{
    DeliverySummary, KeyView, Overview, PaymentView, ReceiptView, RequestRecord, UsageSummary,
};

pub const MAXIMUM_PAGE: usize = 200;
pub const OVERVIEW_PAGE: usize = 20;

pub struct Dashboard {
    gateway: Store,
    webhooks: HostedReader,
}

impl Dashboard {
    pub fn from_environment() -> Result<Self, String> {
        Ok(Self {
            gateway: Store::from_environment()?,
            webhooks: HostedReader::from_environment()?,
        })
    }

    pub fn ready(&self) -> bool {
        self.gateway.ready() && self.webhooks.ready()
    }

    pub fn keys(&self, principal: &Principal, now: u64) -> Result<Vec<KeyView>, DashboardError> {
        self.gateway.keys(principal, now)
    }

    pub fn usage(&self, principal: &Principal, now: u64) -> Result<UsageSummary, DashboardError> {
        self.gateway.usage(principal, now)
    }

    pub fn requests(
        &self,
        principal: &Principal,
        limit: usize,
    ) -> Result<Vec<RequestRecord>, DashboardError> {
        self.gateway.requests(principal, limit.min(MAXIMUM_PAGE))
    }

    pub fn receipt(
        &self,
        principal: &Principal,
        activity_id: &str,
        now: u64,
    ) -> Result<ReceiptView, DashboardError> {
        if activity_id.len() != 64 || !activity_id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(DashboardError::InvalidRequest);
        }
        let activity_id = activity_id.to_ascii_lowercase();
        let snapshot = self.webhooks.snapshot(principal, now, MAXIMUM_PAGE)?;
        let event = snapshot
            .events
            .iter()
            .filter(|event| event.kind() == EventKind::Payment)
            .find(|event| {
                event.facts().iter().any(|fact| {
                    fact.name() == "activity_id"
                        && fact.value() == activity_id.as_str()
                        && fact.verification().at_least(
                            layerx_platform_webhooks::events::Verification::ReceiptVerified,
                        )
                        && fact.receipt_digest().is_some()
                })
            })
            .ok_or(DashboardError::UnknownReceipt)?;
        let payment = PaymentView::of(event);
        let receipt_digest = payment
            .receipt_digest
            .clone()
            .ok_or(DashboardError::UnknownReceipt)?;
        let activity = event
            .facts()
            .iter()
            .find(|fact| fact.name() == "activity_id" && fact.value() == activity_id.as_str())
            .filter(|fact| fact.receipt_digest() == Some(receipt_digest.as_str()))
            .ok_or(DashboardError::UnknownReceipt)?;
        if !payment.settled {
            return Err(DashboardError::UnknownReceipt);
        }
        let verification = if activity
            .verification()
            .at_least(payment.settlement_verification)
        {
            payment.settlement_verification
        } else {
            activity.verification()
        };
        Ok(ReceiptView {
            activity_id,
            event: event.id().as_str().to_owned(),
            receipt_digest,
            verification,
            settled: true,
        })
    }

    pub fn endpoints(
        &self,
        principal: &Principal,
        now: u64,
    ) -> Result<Vec<EndpointHealth>, DashboardError> {
        Ok(self
            .webhooks
            .snapshot(principal, now, MAXIMUM_PAGE)?
            .endpoints)
    }

    pub fn deliveries(
        &self,
        principal: &Principal,
        endpoint: Option<&EndpointId>,
        limit: usize,
        now: u64,
    ) -> Result<Vec<DeliveryRecord>, DashboardError> {
        Ok(self
            .webhooks
            .snapshot(principal, now, limit.min(MAXIMUM_PAGE))?
            .deliveries
            .into_iter()
            .filter(|delivery| endpoint.is_none_or(|wanted| delivery.endpoint == wanted.as_str()))
            .collect())
    }

    pub fn dead_letters(
        &self,
        principal: &Principal,
        limit: usize,
        now: u64,
    ) -> Result<Vec<DeliveryRecord>, DashboardError> {
        Ok(self
            .webhooks
            .snapshot(principal, now, limit.min(MAXIMUM_PAGE))?
            .dead_letters)
    }

    pub fn payments(
        &self,
        principal: &Principal,
        limit: usize,
        now: u64,
    ) -> Result<Vec<PaymentView>, DashboardError> {
        Ok(self
            .webhooks
            .snapshot(principal, now, limit.min(MAXIMUM_PAGE))?
            .events
            .iter()
            .filter(|event| event.kind() == EventKind::Payment)
            .map(PaymentView::of)
            .collect())
    }

    pub fn overview(&self, principal: &Principal, now: u64) -> Result<Overview, DashboardError> {
        let gateway = self.gateway.snapshot(principal, now, OVERVIEW_PAGE)?;
        let webhooks = self.webhooks.snapshot(principal, now, OVERVIEW_PAGE)?;
        let payments = webhooks
            .events
            .iter()
            .filter(|event| event.kind() == EventKind::Payment)
            .map(PaymentView::of)
            .collect();
        Ok(Overview {
            principal: principal.as_str().to_owned(),
            generated_at: now,
            usage: gateway.usage,
            keys: gateway.keys,
            requests: gateway.requests,
            recent_requests: gateway.recent_requests,
            deliveries: DeliverySummary::of(&webhooks.endpoints),
            endpoints: webhooks.endpoints,
            dead_letters: webhooks.dead_letters,
            payments,
        })
    }
}
