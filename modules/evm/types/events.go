package types

const (
	EventTypeAddressAssociated = "address_associated"
	EventTypePointerRegistered = "pointer_registered"
	EventTypeSigner            = "signer"
	EventTypeLayerXBound       = "layerx_bound"
	EventTypeLayerXUnbound     = "layerx_unbound"

	AttributeKeyPaxAddress     = "pax_addr"
	AttributeKeyEvmAddress     = "evm_addr"
	AttributeKeyPointerType    = "pointer_type"
	AttributeKeyPointee        = "pointee"
	AttributeKeyPointerAddress = "pointer_address"
	AttributeKeyPointerVersion = "pointer_version"
	AttributeKeyLayerXDid      = "layerx_did"
	AttributeKeyLayerXNonce    = "layerx_bind_nonce"
)
