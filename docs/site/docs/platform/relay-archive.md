# Relay and archive nodes

The relay/archive role lets an independent operator follow an existing
LayerX network, retain its complete canonical byte history, serve public
reads, and forward users' original signed activities. It does not order
activities, execute state transitions, hold a sequencer key, or participate
in a consensus or finality quorum
(`platform/relay_archive/README.md`; `spec/layerx-beta/spec.kvx`,
requirement 15).

`layerxd --relay-archive CONFIG` replaces the native process with the
installed Python standard-library runtime. The runtime verifies signed
bootstrap and batch material through `layerx-archive-codec`. Python does
not reimplement or weaken the native codecs.

The beta task "Deliver public relay/archive nodes end to end" is marked
done in `spec/layerx-beta/spec.kvx`. The implementation trees are
`cmd/layerx-archive-codec`, `cmd/layerxd/lxp_daemon_cli.c`,
`platform/relay_archive`, and `tests/relay-archive`.

## Trust pins

An operator must obtain these values independently of every synchronization
server and put them in the configuration:

- protocol `network_id`
- SHA-256 of the exact signed genesis manifest
- sequencer identifier and Ed25519 public key
- optionally, the first and last batch authorized for that sequencer

An upstream's `/v1/sync/network` document is discovery metadata, not a
trust anchor. The daemon refuses a different network, genesis digest,
sequencer identity, signature, batch number, predecessor, committed section
root, or canonical byte encoding.

Peer discovery extends read synchronization only. Discovered peers are
never added to submission failover. The installer README names example
listen addresses and placeholder origins for documentation of the install
flags; those placeholders are not LayerX Network product hostnames.

## Operator install

The release layout, `manifest.sha256` rule, and
`platform/relay_archive/install.sh` flags are in
`platform/relay_archive/README.md`. The installer creates the unprivileged
`layerx-relay-archive` account and enables
`layerx-relay-archive.service`. Non-loopback listeners require TLS.

## Related hosted nodes

The in-cluster sequencer pod and its colocated TLS boundaries are
[Hosted node](hosted-node.md), [Hosted core](hosted-core.md), and
[Hosted authority](hosted-authority.md). Those services are the beta
cluster's node, not this independent relay/archive role.
