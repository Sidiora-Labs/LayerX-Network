# HyperPax Node Distribution and Peer Registry (`hpx`)

HPX is the public installer, node manager, immutable artifact publisher and peer
registry for HyperPax (`hyperpax_125-1`). Nodes run a native `paxd` binary under
systemd. Generated executables, native libraries, live chain configuration,
registry data and TLS material are published outside Git.

Public origin: `https://node.hyperpaxeer.com`

## Install a node

```bash
curl -sSL https://node.hyperpaxeer.com/get-hpx.sh | sudo bash
```

The installer verifies the HPX CLI against `checksums.txt`. The setup flow then
verifies `paxd`, all three architecture-specific libwasmvm runtimes, genesis and
the selected fullnode or validator configuration before installing them.

```bash
HPX_TYPE=fullnode hpx setup
hpx status
hpx info
hpx logs
hpx update
hpx peers show
hpx peers refresh
hpx register
hpx statesync
hpx remove
```

Set `HPX_MIRROR` only when operating an explicitly trusted alternate mirror.

## Publish artifacts

Run from this monorepo after a new `paxd` or chain configuration is ready:

```bash
sudo paxeer-network/hpx/publish.sh
```

Defaults:

- `paxd`: `paxeer-network/build/paxd`
- native libraries: the architecture outputs under `paxeer-network/wasm-runtime`
  and `paxeer-network/wasm/x/wasm/artifacts`
- release identity: `paxeer-network/version.json`
- live chain configuration: `/root/.paxeer/config`, overridable with `SRC_CFG`
  or `HPX_RUNTIME_CONFIG_DIR`
- publication root: `/srv/hpx/artifacts`, overridable with
  `HPX_ARTIFACTS_ROOT`

The publisher requires the binary, all six x86-64 and AArch64 native libraries,
genesis, both configuration files and all lifecycle scripts. It stages them in
`releases/<release-id>`, writes a sorted SHA-256 manifest, then atomically moves
the `current` symlink. A failed staging run never changes the served release.

## Publish the registry runtime

Changes under `paxeer-network/hpx/registry` trigger the repository workflow
`Paxeer / HPX Registry`. It publishes revision-bound Linux executables as public
GitHub release assets and publishes the same source as a multi-architecture GHCR
image. Generated registry executables are never committed.

After the workflow publishes the revision, deploy it on the host that serves the
public origin:

```bash
sudo paxeer-network/hpx/hosting/deploy.sh
```

The deployment installs the checksum-verified release executable as an
unprivileged, loopback-only systemd service, persists registry state at
`/srv/hpx/data/registry.json`, obtains the `node.hyperpaxeer.com` certificate and
enables the Nginx reverse proxy. Set `LETSENCRYPT_EMAIL` to attach an email to the
certificate registration. Registration is public by default; set
`HPX_REGISTER_TOKEN` before deployment to require `X-HPX-Token`.

## Public surface

| Method | Path | Purpose |
|---|---|---|
| GET | `/healthz` | registry liveness, chain and source revision |
| GET | `/checksums.txt`, `/chain-info.json` | release integrity and chain metadata |
| GET | `/paxd`, `/lib/*.so`, `/genesis.json`, `/config/<type>/<file>` | declared node artifacts |
| GET | `/install`, `/get-hpx.sh`, `/hpx`, `/uninstall.sh` | lifecycle scripts and CLI |
| GET | `/api/myip` | caller's public address |
| POST | `/api/register` | announce the caller's observed public peer address |
| GET | `/api/peers`, `/api/peers.txt`, `/api/nodes` | registry discovery |
| GET | `/api/statesync` | current state-sync trust parameters |

The registry denies directory indexes and undeclared artifact paths. Nginx
overwrites forwarded-address headers and rate-limits registration before proxying
to the loopback service.
