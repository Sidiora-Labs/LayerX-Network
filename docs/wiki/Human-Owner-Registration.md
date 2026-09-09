
Beta recovery uses three dedicated, randomly generated Ed25519 guardian keys,
threshold two. Each public key is bound to one genesis guarantor identity or the
sequencer identity in `recovery-guardian-bindings.json`, published as ConfigMap
`layerx-human-guardian-bindings`. The private seeds stay in separate Secrets
`layerx-human-guardian-guarantor-1`, `layerx-human-guardian-guarantor-2` and
`layerx-human-guardian-sequencer`. These keys are not derived from custody keys.
All three guardians are cluster-operated and non-independent. Independent
operation is a production onboarding requirement. The recovery commitment is
SHA-256 of `LX:HUMAN:RECOVERY:v1` plus NUL, big-endian u16 threshold and count,
then sorted Ed25519 public keys. Existing material is refused instead of replaced.
