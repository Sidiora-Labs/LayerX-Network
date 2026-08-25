# HyperPax Node Distribution + Peer Registry (`hpx`)

Self-hosted, one-command installer for HyperPax (`hyperpax_125-1`) nodes.
Nodes run as a **native `paxd` binary under systemd** (no Docker). The binary,
`libwasmvm`, genesis, and **fullnode / validator config variants** are all
served from a single mirror on the Paxeer Cloud box. When a node finishes
setup it **registers its CometBFT node id upstream** and receives the live peer
list, so the mesh grows automatically as new nodes join.

```
operator box (m19581, 149.102.158.181)
├─ hpx-registry  (Go service, container on paxeer-cloud_default :8099)
│    ├─ serves /srv/hpx/artifacts  (paxd, lib/, genesis.json, config/*, hpx, *.sh, chain-info.json)
│    └─ registry API  /api/register · /api/peers[.txt] · /api/nodes · /api/myip
├─ Caddy (Paxeer Cloud)  get.cloud.hyperpaxeer.com  →  hpx-registry:8099  (auto TLS)
└─ /srv/hpx/data/registry.json   (persisted node registry)

each node VPS
└─ hpx setup  →  pulls artifacts → keygen → configure → POST /api/register → systemd start
```

**Mirror:** `https://node.cloud.hyperpaxeer.com`
**Seed peer (genesis validator):** `e9c56cbadc4a96b67f69dcaaa7b4691851e945ca@31.220.74.140:26656`

---

## Install a node (on each VPS)

```bash
curl -sSL https://node.cloud.hyperpaxeer.com/get-hpx.sh | sudo bash
```

This installs the `hpx` CLI and launches the interactive wizard. Or drive it
non-interactively:

```bash
HPX_TYPE=fullnode  hpx setup     # or HPX_TYPE=validator
```

The wizard: installs deps (`curl`,`jq`) → downloads `paxd` + `libwasmvm` (sha256
verified) → genesis + the chosen config variant → generates the node identity →
registers with the mirror and pulls the peer list → writes the `paxd` systemd
unit + hourly peer-refresh timer → starts the node.

### Day-2 ops

```bash
hpx status            # service + TM/EVM height + peers
hpx info              # endpoints + node id
hpx logs              # journalctl -u paxd -f
hpx update            # pull the published paxd version + restart
hpx peers show        # all nodes in the registry
hpx peers refresh     # re-pull persistent-peers + restart
hpx register          # re-announce this node
hpx remove            # uninstall (optionally wipe /root/.paxeer)
```

---

## Operator: (re)publish the package

Run on the operator box whenever `paxd` is rebuilt or chain config changes:

```bash
sudo bash /root/project-Quorum/hpx-cli/publish.sh
# then make the served hpx/scripts the canonical copy:
#   publish.sh already copies hpx, get-hpx.sh, uninstall.sh into /srv/hpx/artifacts
```

`publish.sh` assembles `/srv/hpx/artifacts/` from:
- `paxd` ← `paxeer-v3-matrix-release/build/paxd` (+ `paxd.sha256`)
- `lib/` ← `libwasmvm*.{x86_64,aarch64}.so`
- `genesis.json` ← `/root/.paxeer/config/genesis.json`
- `config/{fullnode,validator}/{config.toml,app.toml}` ← derived from the live
  config (CometBFT v1 / Pax layout: hyphenated keys, `mode` set per variant,
  `pex=true`; `moniker`/`external-address`/`persistent-peers` filled per host)
- `chain-info.json` (chain id, version, sha256, seed peer)

The `hpx-registry` container mounts `/srv/hpx/artifacts` read-only, so a
re-publish is picked up immediately (scripts/json are served `no-store`).

## Registry service

Built + run as a container on the `paxeer-cloud_default` network:

```bash
docker build -t hpx-registry:latest /root/project-Quorum/hpx-cli/registry
docker run -d --name hpx-registry --restart unless-stopped \
  --network paxeer-cloud_default \
  -v /srv/hpx/artifacts:/srv/hpx/artifacts:ro \
  -v /srv/hpx/data:/srv/hpx/data \
  -e HPX_CHAIN_ID=hyperpax_125-1 \
  -e HPX_SEED_PEERS=e9c56cbadc4a96b67f69dcaaa7b4691851e945ca@31.220.74.140:26656 \
  hpx-registry:latest
```

Caddy vhost (in `/paxeer-cloud/deploy/Caddyfile`): `get.cloud.hyperpaxeer.com`
→ `reverse_proxy hpx-registry:8099` with on-demand TLS (auto-approved because it
is a subdomain of `cloud.hyperpaxeer.com`).

### API

| Method | Path | Purpose |
|--------|------|---------|
| GET  | `/healthz` | liveness + chain id |
| GET  | `/chain-info.json` | chain id, paxd version + sha256, seed peer |
| GET  | `/paxd`, `/lib/*.so`, `/genesis.json`, `/config/<type>/<file>` | artifacts |
| GET  | `/hpx`, `/get-hpx.sh`, `/uninstall.sh` | CLI + scripts |
| GET  | `/api/myip` | caller's public IP |
| POST | `/api/register` | announce a node `{node_id,moniker,ip,type,version,p2p_port}` → returns peers |
| GET  | `/api/peers` / `/api/peers.txt` | peer list (`?self=<id>` excludes self) |
| GET  | `/api/nodes` | full registry |

Optional auth: set `HPX_REGISTER_TOKEN` on the container and pass `HPX_TOKEN`
(`X-HPX-Token`) from the CLI to gate `/api/register`.

Registry state: `/srv/hpx/data/registry.json` (de-duped by node id, seed peers
always included).
