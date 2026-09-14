use super::*;
use layerx_crypto::rotation::OwnerRotationState;

pub(super) struct PrincipalOwner {
    pub actor: AgentDid,
    pub authority: AuthorityRef,
    pub account: AccountId,
    pub identity: super::super::agent_runtime::AgentCoreIdentity,
}

impl ProductionComponents {
    pub(super) fn principal_agent(
        &self,
        scope: &crate::store::PrincipalScope<'_>,
    ) -> Result<AgentRuntime, ApiFailure> {
        let (actor, account) = movement_principal_account(scope)?;
        let did =
            Did::new(actor.as_str().as_bytes()).map_err(|_| ApiFailure::upstream_degraded())?;
        let mut agent = self.agent.lock().map_err(|_| ApiFailure::unavailable())?;
        let asset = agent.native_fee_policy().map_err(agent_failure)?.asset_id;
        agent
            .for_subject(scope.principal(), &did, &account, asset)
            .map_err(agent_failure)
    }
}

pub(super) fn resolve_principal_owner(
    components: &ProductionComponents,
    scope: &crate::store::PrincipalScope<'_>,
    agent: &mut AgentRuntime,
) -> Result<PrincipalOwner, ApiFailure> {
    let (actor, account) = movement_principal_account(scope)?;
    let key = KeyId::new("human-primary").map_err(|_| ApiFailure::upstream_degraded())?;
    let descriptor = components
        .custody
        .describe_key(scope.principal(), &key)
        .map_err(|_| ApiFailure::forbidden())?;
    let binding = components
        .custody
        .evm_binding(scope.principal(), &key)
        .map_err(|_| ApiFailure::forbidden())?;
    if descriptor.class != KeyClass::HumanPrimary
        || binding.class() != KeyClass::HumanPrimary
        || binding.network_id() != components.network_id
        || components.protocol_version != 3
    {
        return Err(ApiFailure::forbidden());
    }
    let identity = agent
        .identity_resolve(actor.as_str())
        .map_err(agent_failure)?;
    let did = Did::new(actor.as_str().as_bytes()).map_err(|_| ApiFailure::upstream_degraded())?;
    validate_owner_identity(&did, descriptor.public_key, &identity)?;
    let authority = AuthorityRef::new(hex_bytes(&descriptor.public_key))
        .map_err(|_| ApiFailure::upstream_degraded())?;
    Ok(PrincipalOwner {
        actor,
        authority,
        account,
        identity,
    })
}

fn validate_owner_identity(
    did: &Did,
    public_key: [u8; 32],
    identity: &super::super::agent_runtime::AgentCoreIdentity,
) -> Result<(), ApiFailure> {
    let state = OwnerRotationState::decode(&identity.canonical_bytes, did)
        .map_err(|_| ApiFailure::forbidden())?;
    if identity.frozen
        || !matches!(identity.verification, 4 | 5)
        || identity.revocation_sequence != state.revocation_sequence
        || identity.head_sequence < state.observed_sequence
        || state.primary_public_key != public_key
        || !identity.authorities.contains(&(1, public_key))
    {
        return Err(ApiFailure::forbidden());
    }
    Ok(())
}

impl PrincipalOwner {
    pub fn recovery_policy(&self) -> Result<([u8; 32], u16), ApiFailure> {
        let root = self.identity.canonical_bytes[77..109]
            .try_into()
            .map_err(|_| ApiFailure::upstream_degraded())?;
        let threshold = u16::from_be_bytes(
            self.identity.canonical_bytes[109..111]
                .try_into()
                .map_err(|_| ApiFailure::upstream_degraded())?,
        );
        if root == [0; 32] || threshold == 0 {
            return Err(ApiFailure::forbidden());
        }
        Ok((root, threshold))
    }
}

#[cfg(test)]
#[path = "production_owner_tests.rs"]
mod tests;
