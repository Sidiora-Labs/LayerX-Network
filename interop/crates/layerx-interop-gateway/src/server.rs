//! Transport-neutral route and evidence contracts for the executable interop
//! composition service. This module deliberately depends on no adapter crate:
//! adapters depend on the gateway core, while the executable composition crate
//! depends on both sides and therefore cannot create a Cargo cycle.

/// Ingress transports carried by the production interoperability service.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IngressTransport {
    Http,
    Mcp,
    A2a,
}

impl IngressTransport {
    /// Returns the stable wire label.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Http => "http",
            Self::Mcp => "mcp",
            Self::A2a => "a2a",
        }
    }
}

/// Protocol adapters hosted by the executable service.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HostedAdapter {
    X402,
    Ap2,
    Ucp,
    VisaTap,
    FiatCard,
    FiatBank,
    FiatRtp,
}

impl HostedAdapter {
    /// Returns the exact registered adapter identifier.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::X402 => "x402",
            Self::Ap2 => "ap2",
            Self::Ucp => "ucp",
            Self::VisaTap => "visa-tap",
            Self::FiatCard | Self::FiatBank | Self::FiatRtp => "fiat",
        }
    }

    /// Returns the external surface label. Fiat rails remain distinct at the
    /// edge even though they share one receipt-gated adapter implementation.
    #[must_use]
    pub const fn surface(self) -> &'static str {
        match self {
            Self::X402 => "x402",
            Self::Ap2 => "ap2",
            Self::Ucp => "ucp",
            Self::VisaTap => "visa-tap",
            Self::FiatCard => "fiat-card",
            Self::FiatBank => "fiat-bank",
            Self::FiatRtp => "fiat-rtp",
        }
    }
}

/// Closed executable route set. State-changing and verification-only paths
/// are separate variants so a router cannot accidentally call settlement from
/// a read-only endpoint.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InteropRoute<'a> {
    Live,
    Ready,
    AdapterMetadata,
    Resume { operation: &'a str },
    X402Supported { transport: IngressTransport },
    X402BuyerBuild { transport: IngressTransport },
    X402SellerOffer { transport: IngressTransport },
    X402Verify { transport: IngressTransport },
    X402Settle { transport: IngressTransport },
    Ap2VerifyMandates,
    Ap2Execute,
    UcpComplete,
    VisaVerifyIntent,
    VisaExecuteIntent,
    FiatCallback { adapter: HostedAdapter },
}

impl InteropRoute<'_> {
    /// Returns the adapter reached by this route, if any.
    #[must_use]
    pub const fn adapter(&self) -> Option<HostedAdapter> {
        match self {
            Self::Live | Self::Ready | Self::AdapterMetadata | Self::Resume { .. } => None,
            Self::X402Supported { .. }
            | Self::X402BuyerBuild { .. }
            | Self::X402SellerOffer { .. }
            | Self::X402Verify { .. }
            | Self::X402Settle { .. } => Some(HostedAdapter::X402),
            Self::Ap2VerifyMandates | Self::Ap2Execute => Some(HostedAdapter::Ap2),
            Self::UcpComplete => Some(HostedAdapter::Ucp),
            Self::VisaVerifyIntent | Self::VisaExecuteIntent => Some(HostedAdapter::VisaTap),
            Self::FiatCallback { adapter } => Some(*adapter),
        }
    }

    /// Whether this route may produce an economic state transition.
    #[must_use]
    pub const fn state_changing(&self) -> bool {
        matches!(
            self,
            Self::X402Settle { .. }
                | Self::Ap2Execute
                | Self::UcpComplete
                | Self::VisaExecuteIntent
                | Self::FiatCallback { .. }
        )
    }
}

/// Backing evidence required before an adapter may render a terminal outcome.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EvidencePolicy {
    LayerXReceipt,
    VerifiedMandateAndLayerXReceipt,
    TrustedAgentCredential,
    ExternalSettlementAndLayerXReceipt,
}

impl EvidencePolicy {
    /// Parses an exact deployment declaration. There is intentionally no
    /// default: an omitted or unfamiliar policy refuses service startup.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "layerx-receipt" => Some(Self::LayerXReceipt),
            "verified-mandate+layerx-receipt" => Some(Self::VerifiedMandateAndLayerXReceipt),
            "trusted-agent-credential" => Some(Self::TrustedAgentCredential),
            "external-settlement+layerx-receipt" => Some(Self::ExternalSettlementAndLayerXReceipt),
            _ => None,
        }
    }

    /// Returns the exact deployment label.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::LayerXReceipt => "layerx-receipt",
            Self::VerifiedMandateAndLayerXReceipt => "verified-mandate+layerx-receipt",
            Self::TrustedAgentCredential => "trusted-agent-credential",
            Self::ExternalSettlementAndLayerXReceipt => "external-settlement+layerx-receipt",
        }
    }
}

/// Honest public state vocabulary used across adapter transports.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExternalState {
    Pending,
    ReversalPending,
    Refused,
    ReceiptVerified,
    Reversed,
}

impl ExternalState {
    /// Returns the stable response label.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::ReversalPending => "reversal-pending",
            Self::Refused => "refused",
            Self::ReceiptVerified => "receipt-verified",
            Self::Reversed => "reversed",
        }
    }
}

/// Parses the exact public gateway route set.
///
/// # Errors
/// Refuses unknown methods, paths, transports and unbounded operation keys.
pub fn interop_gateway_routes<'a>(
    method: &str,
    path: &'a str,
) -> Result<InteropRoute<'a>, RouteError> {
    match (method, path) {
        ("GET", "/livez") => return Ok(InteropRoute::Live),
        ("GET", "/readyz") => return Ok(InteropRoute::Ready),
        ("GET", "/v1/adapters") => return Ok(InteropRoute::AdapterMetadata),
        _ => {}
    }
    if method == "GET" {
        if let Some(operation) = path.strip_prefix("/v1/operations/") {
            if operation.len() == 64 && operation.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return Ok(InteropRoute::Resume { operation });
            }
            return Err(RouteError::InvalidOperation);
        }
    }
    let mut parts = path
        .strip_prefix("/v1/")
        .ok_or(RouteError::Unknown)?
        .split('/');
    let transport = parse_transport(parts.next().ok_or(RouteError::Unknown)?)?;
    let adapter = parts.next().ok_or(RouteError::Unknown)?;
    let tail: Vec<_> = parts.collect();
    let route = match (method, transport, adapter, tail.as_slice()) {
        ("GET", transport, "x402", ["supported"] | ["facilitator", "supported"]) => {
            InteropRoute::X402Supported { transport }
        }
        ("POST", transport, "x402", ["buyer", "build"]) => {
            InteropRoute::X402BuyerBuild { transport }
        }
        ("POST", transport, "x402", ["seller", "offer"]) => {
            InteropRoute::X402SellerOffer { transport }
        }
        ("POST", transport, "x402", ["verify"] | ["facilitator", "verify"]) => {
            InteropRoute::X402Verify { transport }
        }
        (
            "POST",
            transport,
            "x402",
            ["settle"] | ["seller", "settle"] | ["facilitator", "settle"],
        ) => InteropRoute::X402Settle { transport },
        ("POST", IngressTransport::Http, "ap2", ["mandates", "verify"]) => {
            InteropRoute::Ap2VerifyMandates
        }
        ("POST", IngressTransport::Http, "ap2", ["execute"]) => InteropRoute::Ap2Execute,
        ("POST", IngressTransport::Http, "ucp", ["checkouts", "complete"]) => {
            InteropRoute::UcpComplete
        }
        ("POST", IngressTransport::Http, "visa-tap", ["intents", "verify"]) => {
            InteropRoute::VisaVerifyIntent
        }
        ("POST", IngressTransport::Http, "visa-tap", ["intents", "execute"]) => {
            InteropRoute::VisaExecuteIntent
        }
        ("POST", IngressTransport::Http, "fiat", ["card", "callbacks"]) => {
            InteropRoute::FiatCallback {
                adapter: HostedAdapter::FiatCard,
            }
        }
        ("POST", IngressTransport::Http, "fiat", ["bank", "callbacks"]) => {
            InteropRoute::FiatCallback {
                adapter: HostedAdapter::FiatBank,
            }
        }
        ("POST", IngressTransport::Http, "fiat", ["rtp", "callbacks"]) => {
            InteropRoute::FiatCallback {
                adapter: HostedAdapter::FiatRtp,
            }
        }
        _ => return Err(RouteError::Unknown),
    };
    Ok(route)
}

fn parse_transport(value: &str) -> Result<IngressTransport, RouteError> {
    match value {
        "http" => Ok(IngressTransport::Http),
        "mcp" => Ok(IngressTransport::Mcp),
        "a2a" => Ok(IngressTransport::A2a),
        _ => Err(RouteError::UnknownTransport),
    }
}

/// Stable route refusal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RouteError {
    Unknown,
    UnknownTransport,
    InvalidOperation,
}
