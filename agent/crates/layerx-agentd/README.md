# layerx-agentd

`LAYERX_AGENT_MODE` selects `full` (also the default when unset) or
`human-owner`. Other values, including an empty value, are refused.

Full mode starts the Human owner and the verified Programs reader. Its Programs
probe, admission journal, protected sequencer history, replica identity and
credential checks remain required.

Human-owner mode starts only the Human owner. All existing `LAYERX_AGENT_HUMAN_*`
LNI, peer, authority, session key, store, socket and limit inputs remain required.
It does not read the Programs probe, admission journal, sequencer history or node
reader inputs. It reuses `LAYERX_AGENT_PROGRAM_LISTEN` (127.0.0.1 only) and
`LAYERX_AGENT_PROGRAM_BEARER_TOKEN` (at least 32 bytes, distinct from the Human
authority bearer) for authenticated `GET /healthz`. Other routes return 404.
Health returns 200 with `{"ready":true}` only after successful owner startup,
while its listener thread is running, and after a fresh node LNI handshake and
an authority registry read for every configured peer. Dependency failure returns
503 with `{"ready":false}`; owner termination stops the process. Health requests
retain the existing 16 KiB header bound and 10-second socket timeouts.

`LAYERX_AGENT_HUMAN_AUTHORITY_CA_DER` is required in both modes and names a file
containing the DER-encoded CA certificate for the Human authority HTTPS endpoint.
`LAYERX_AGENT_AUTHORITY_CA_DER` is additionally required in full mode and names
the DER CA file trusted by both Programs read endpoints. Empty or unreadable CA
files are refused. Clients use Native TLS with only the supplied CA as a trust
root; hostname verification remains enabled. Human authority endpoints require
HTTPS; Programs endpoints retain their existing HTTPS or loopback HTTP rule.

TLS tests generate temporary CA and server certificates using the existing
locked OpenSSL crate and run a real loopback Native TLS server. Both crates were
already in agent/Cargo.lock; they are direct test dependencies of this crate.

ureq 3.4.0 requires its `native-tls` feature to compile the Native TLS connector;
`native-tls-no-default` alone leaves HTTPS requests panicking even with an
explicit provider. Enabling the connector adds the transitive
`webpki-root-certs` package. Its bundled roots are not selected: the explicit
`RootCerts::Specific` configuration trusts only the supplied CA.
