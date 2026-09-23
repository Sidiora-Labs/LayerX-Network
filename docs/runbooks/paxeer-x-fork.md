# Paxeer X fork validator runbook

Upgrades mainnet `hyperpax_125-1` (EVM chain id 125) to the Paxeer X binary through the
`$UPGRADE_NAME` upgrade plan (`v6.6` in `node/tags`, registered by `node/upgrades.go`). At that height the store loader
(`node/app.go` `SetStoreUpgradeHandlers`) adds the LayerX stores (`layerxcustody`, `layerxanchor`,
and the exchange, bridge and launchpad stores), the handler registered from `node/tags`
(`node/upgrades.go`) runs module migrations, and the precompiles 0x1013 custody, 0x1014 anchor,
0x1015 exchange, 0x1016 bridge and 0x1017 launchpad come live.

The target wall-clock time `$T_HALT_UTC` is set locally (see "Local values" below). The halt is
scheduled **by height only**. Block timestamps can drift from wall clock, so never use `halt-time`,
and never use `halt-height` either: only the upgrade plan makes the old binary write
`data/upgrade-info.json`, and the new binary reads that file to add the new stores. A node stopped
any other way comes up on the new binary without the stores.

This runbook assumes a manual binary swap under systemd (no cosmovisor). Operators running
cosmovisor adapt steps 5–6 of section 6 to its `upgrades/$UPGRADE_NAME/bin` layout.

## Local values

Every concrete value (hosts, homes, units, ports, target time, hashes) lives in an untracked
`.env`-style file, for example `.env.paxeer-fork` at the repository root (`.env.*` is ignored by
git). Load it with `set -a; . ./.env.paxeer-fork; set +a` before running the commands below.
Expected keys:

| Key | Meaning |
|---|---|
| `UPGRADE_NAME` | upgrade plan name (`v6.6`) |
| `COSMOS_CHAIN_ID` | `hyperpax_125-1` |
| `EVM_CHAIN_ID` | `125` |
| `PAXD_BIN` | path of the running `paxd` on the host |
| `PAXD_HOME` | node home directory on the host |
| `PAXD_HOMES` | every node home on the host, space-separated (section 0 discovery) |
| `PAXD_UNIT` | systemd unit that runs `PAXD_HOME` |
| `COMET_RPC_PORT` | local CometBFT RPC port of that home |
| `EVM_RPC_PORT` | local EVM JSON-RPC port of that home |
| `EVM_PUBLIC_TLS_PORT` | public TLS port of the reverse proxy in front of the EVM RPC, if any |
| `T_HALT_UTC` | target halt time, ISO 8601 UTC (`YYYY-MM-DDTHH:MM:SSZ`) |
| `HALT_HEIGHT` | the computed H from section 1 |
| `SNAPSHOT_SERVER_RPC` | CometBFT RPC URL of the snapshot server (section 8) |
| `STATESYNC_RPC_SERVERS` | comma-separated CometBFT RPC URLs for state sync (section 8) |

On a host with more than one home, keep one file per home or override `PAXD_HOME`, `PAXD_UNIT`,
`COMET_RPC_PORT` and `EVM_RPC_PORT` per command.

## 0. Facts to confirm first

Before anything else, map every consensus key of the active set to a host and home. Read the set
from any synced RPC:

```sh
curl -s "localhost:$COMET_RPC_PORT/validators" | jq -r '(.result // .).validators[] | "\(.address) \(.voting_power)"'
```

On every validator host run, for every home present:

```sh
for home in $PAXD_HOMES; do
  [ -f "$home/config/priv_validator_key.json" ] || continue
  echo "$(hostname) $home $(jq -r .address "$home/config/priv_validator_key.json")"
done
for unit in $(systemctl list-unit-files --no-legend 'paxd*.service' | awk '{print $1}'); do
  echo "$unit: $(systemctl show -p ExecStart --value "$unit" | grep -o 'argv\[\]=[^;]*')"
done
for home in $PAXD_HOMES; do grep -E '^laddr' "$home/config/config.toml"; done   # rpc port per home
```

Fill in the table locally (never commit the filled copy) and have a second person check it. Do not
continue until there is one row per validator in the active set, every row is filled, and no key
appears in two homes (a key running twice double-signs).

| Validator key | Host | Home | systemd unit | RPC port |
|---|---|---|---|---|
| `<validator-1>` | | | | |
| `<validator-2>` | | | | |
| `<validator-n>` | | | | |

## 1. Halt height

Measure the block rate on a synced node for five minutes (run on the node, or point at any synced RPC):

```sh
rpc=http://127.0.0.1:$COMET_RPC_PORT
h() { curl -s "$rpc/status" | jq -r '(.result // .).sync_info.latest_block_height'; }
h1=$(h); t1=$(date -u +%s); sleep 300; h2=$(h); t2=$(date -u +%s)
echo "h1=$h1 h2=$h2 seconds_per_block=$(echo "scale=6; ($t2-$t1)/($h2-$h1)" | bc)"
```

Formula:

```
H = current_height + round((T_halt_utc - now_utc) / measured_seconds_per_block)
T_halt_utc = $(date -u -d "$T_HALT_UTC" +%s)
```

```sh
target=$(date -u -d "$T_HALT_UTC" +%s)
spb=<seconds_per_block>
HALT_HEIGHT=$(echo "$h2 + ($target - $t2) / $spb + 0.5" | bc | cut -d. -f1)
echo "H=$HALT_HEIGHT"
```

Worked slot (fill in locally from the measurement):

| Field | Value |
|---|---|
| Measured at (UTC) | |
| h1 / h2 | |
| seconds_per_block | |
| current_height at computation | |
| now_utc (epoch) | |
| T_halt_utc (epoch) | |
| **H** | |

Re-measure 24 h before the target. If the projected time of H has drifted more than 30 minutes the
plan must be cancelled (`paxd tx gov submit-proposal cancel-software-upgrade`) and re-proposed;
never edit the height on nodes.

## 2. Proposal

Read the live governance parameters; the voting period decides how early the proposal must go in
(submit no later than `T_halt - voting_period - 12h`):

```sh
paxd q gov params -o json | jq '{voting_params, deposit_params, tally_params}'
```

Submit from a funded operator account. Every validator operator votes yes; from the tally
parameters above and the voting powers read in section 0, confirm that the pledged yes votes meet
quorum and threshold with margin (one validator missing must not sink the vote):

```sh
paxd tx gov submit-proposal software-upgrade "$UPGRADE_NAME" \
  --upgrade-height "$HALT_HEIGHT" \
  --upgrade-info '{"binaries":{"linux/amd64":"<release URL>?checksum=sha256:<SHA256>"}}' \
  --title "Paxeer X $UPGRADE_NAME" --description "Adds the LayerX custody, anchor, exchange, bridge and launchpad modules" \
  --deposit <min_deposit>uhpx --from <operator> --chain-id "$COSMOS_CHAIN_ID" --fees <fees>uhpx -b sync -y
paxd q gov proposals -o json | jq '.proposals[-1] | {proposal_id, status, plan: .content.plan}'
paxd tx gov vote <proposal_id> yes --from <operator> --chain-id "$COSMOS_CHAIN_ID" --fees <fees>uhpx -y
paxd q upgrade plan            # after it passes: name $UPGRADE_NAME, height $HALT_HEIGHT
```

## 3. Binary

Fill in locally:

| Field | Value |
|---|---|
| Source commit | |
| Release URL | |
| `paxd` sha256 | |
| `libwasmvm*.so` sha256 (only if the release changes it) | |

On every host, stage and verify before the height (never overwrite the running binary early):

```sh
install -m 0755 "paxd-$UPGRADE_NAME" "$PAXD_BIN.$UPGRADE_NAME"
sha256sum "$PAXD_BIN.$UPGRADE_NAME"                # must equal the table
cp -p "$PAXD_BIN" "$PAXD_BIN.pre-$UPGRADE_NAME"
ldd "$PAXD_BIN.$UPGRADE_NAME" | grep -i wasmvm     # compare with ldd "$PAXD_BIN"
"$PAXD_BIN.$UPGRADE_NAME" version --long | head -n 5
```

If `ldd` shows a libwasmvm that is not already on the host, install it next to the existing one
(`/usr/local/lib` or the path the current binary resolves), record its sha256 above, and run
`ldconfig`. Do not remove the old library: rollback needs it.

## 4. Rehearsal (must pass before go)

Run on a spare machine against a copy of a synced full node's data taken while that node was
stopped. The script takes every path from flags or from the env file (`--env-file`, see
`fork-rehearsal.sh --help` for the keys); it never contacts a remote host:

```sh
platform/hosted/paxeer/fork-rehearsal.sh --check
platform/hosted/paxeer/fork-rehearsal.sh \
  --source-data <snapshot-dir>/data --source-genesis <snapshot-dir>/config/genesis.json \
  --old-bin "$PAXD_BIN.pre-$UPGRADE_NAME" --new-bin "$PAXD_BIN.$UPGRADE_NAME" \
  --work <rehearsal-dir> --upgrade-height-offset 120 \
  --view 0x0000000000000000000000000000000000001015=<exchange view calldata> \
  --view 0x0000000000000000000000000000000000001016=<bridge view calldata> \
  --view 0x0000000000000000000000000000000000001017=<launchpad view calldata>
```

It exports state with the old binary, rewrites it to one disposable validator with the same chain
id, passes a `$UPGRADE_NAME` plan, requires the old binary to halt with
`UPGRADE "$UPGRADE_NAME" NEEDED at height: …`, resumes on the new binary, and checks: height
advances, `paxd q upgrade applied $UPGRADE_NAME` equals the plan height, every added module is in
the version map, and a view on each of 0x1013–0x1017 returns without revert. It writes
`<rehearsal-dir>/rehearsal-result.env` with both binary hashes; the new hash must match section 3.

## 5. Announcement

> **Paxeer mainnet upgrade: Paxeer X (<UPGRADE_NAME>)**
> Mainnet `hyperpax_125-1` halts for the <UPGRADE_NAME> upgrade at block **H = <H>**, expected
> around **<T_HALT_UTC> (<local time>)**. The height is authoritative; the time is an estimate.
> Node operators: install paxd <UPGRADE_NAME> (sha256 `<SHA256>`) only after your node logs
> `UPGRADE "<UPGRADE_NAME>" NEEDED at height: <H>`, then restart. Do not stop your node early and do
> not use halt-height. RPC and the EVM endpoint will be unavailable from block <H> until validators
> holding two thirds of the power are back, expected within 30 minutes. Status updates: <channel>.

Post it when the proposal enters voting, again at T−24 h and at T−1 h.

## 6. Per-validator procedure (at height H)

Order: follow the table in section 0. Consensus resumes once validators holding more than two
thirds of the voting power are up, so bring back, as fast as possible, the smallest group of
validators that crosses two thirds (compute it from the voting powers read in section 0); the rest
follow.

For each validator operator (`$PAXD_UNIT`, `$PAXD_HOME` and `$COMET_RPC_PORT` from its section 0 row):

1. Watch for the halt, do not act before it:
   ```sh
   journalctl -u "$PAXD_UNIT" -f | grep -m1 "UPGRADE \"$UPGRADE_NAME\" NEEDED at height: $HALT_HEIGHT"
   ```
2. Verify the halt and the upgrade file:
   ```sh
   curl -s "localhost:$COMET_RPC_PORT/status" | jq -r '(.result // .).sync_info.latest_block_height'   # H-1
   jq . "$PAXD_HOME/data/upgrade-info.json"        # {"name":"$UPGRADE_NAME","height":H}
   ```
3. Stop the unit: `systemctl stop "$PAXD_UNIT"`; confirm `systemctl is-active "$PAXD_UNIT"` prints `inactive`.
4. Snapshot data for rollback (local disk, while stopped):
   `cp -a --reflink=auto "$PAXD_HOME/data" "$PAXD_HOME/data.pre-$UPGRADE_NAME"` (or an LVM/ZFS snapshot).
5. Replace the binary (units on one host that share `$PAXD_BIN` are swapped once per host, before
   the first of its units restarts):
   ```sh
   install -m 0755 "$PAXD_BIN.$UPGRADE_NAME" "$PAXD_BIN"
   sha256sum "$PAXD_BIN"   # must equal section 3
   ```
   plus the libwasmvm from section 3 if the release requires it.
6. Start: `systemctl start "$PAXD_UNIT"`.
7. Verify:
   ```sh
   journalctl -u "$PAXD_UNIT" -n 200 --no-pager | grep -E "applying upgrade \"$UPGRADE_NAME\"|$UPGRADE_NAME"
   curl -s "localhost:$COMET_RPC_PORT/status" | jq -r '(.result // .).sync_info.latest_block_height'   # > H once two thirds of the power is up
   paxd q upgrade applied "$UPGRADE_NAME" --node "tcp://127.0.0.1:$COMET_RPC_PORT" -o json | jq -r .header.height   # H
   ```
   Then from the EVM endpoint of that host, one view per new precompile must return without revert:
   ```sh
   curl -s -H 'content-type: application/json' "localhost:$EVM_RPC_PORT" \
     --data '{"jsonrpc":"2.0","id":1,"method":"eth_call","params":[{"to":"0x0000000000000000000000000000000000001013","data":"0x2dfdf0b5"},"latest"]}'
   ```
   (0x1014 with `0x42cde4e8`; 0x1015–0x1017 with the calldata used in the rehearsal.)

Record per validator, locally: halt log seen (Y/N), upgrade-info.json OK, sha256 OK, restarted at
(UTC), first signed height after H.

## 7. Rollback

Only if the new binary cannot produce blocks past H after every validator ran it and the fix is
not available within the agreed window (default 2 h). All validators must roll back together.

1. `systemctl stop "$PAXD_UNIT"` on every validator and full node.
2. Restore data: `rm -rf "$PAXD_HOME/data" && mv "$PAXD_HOME/data.pre-$UPGRADE_NAME" "$PAXD_HOME/data"` (keep
   `priv_validator_state.json` from the snapshot; never move it backwards past a height this key
   signed on the new binary — if any validator signed H on the new binary, the snapshot's state file
   is behind and double-signing is possible: stop and coordinate before continuing).
3. Restore the binary: `install -m 0755 "$PAXD_BIN.pre-$UPGRADE_NAME" "$PAXD_BIN"` (and the
   old libwasmvm if it was replaced).
4. Start the old binary past the plan with the skip flag, added to the unit's `ExecStart` through a
   drop-in: `paxd start --home "$PAXD_HOME" --unsafe-skip-upgrades "$HALT_HEIGHT"`. Every validator
   must use the same H.
5. Once blocks advance, cancel the plan by governance (`cancel-software-upgrade`) and remove the
   drop-in after the cancellation passes.

## 8. Full nodes and RPC fleet

Synced full nodes (`<full-node-1>` … `<full-node-n>`, listed locally) follow the section 6 steps
1–7 at H, after the validators. A TLS reverse proxy in front of the EVM RPC
(`$EVM_PUBLIC_TLS_PORT` → `$EVM_RPC_PORT`) needs no change; during the swap put each host's
upstream in maintenance (for nginx: mark it `down` and `nginx -s reload`) and restore it once
`eth_blockNumber` returns a height above H.

Lagging or stalled nodes are not upgraded in place:

- Before H: pick one synced node (`<snapshot-server>`, `$SNAPSHOT_SERVER_RPC`) as snapshot server:
  `app.toml` `snapshot-interval = 1000`, `snapshot-keep-recent = 2`; restart it once, early.
- After H, once that node serves a snapshot above H: stop each lagging node, install the
  `$UPGRADE_NAME` binary, back up its `priv_validator_key.json`/`node_key.json`, clear `data/` (keep
  `priv_validator_state.json` as `{"height":"0","round":0,"step":0}`), and enable state sync in
  `config.toml`:
  ```toml
  [statesync]
  enable = true
  rpc-servers = "<STATESYNC_RPC_SERVERS>"   # at least two, e.g. <full-node-rpc-1>,<full-node-rpc-2>
  trust-height = <recent height above H, multiple of the snapshot interval>
  trust-hash = "<curl -s $SNAPSHOT_SERVER_RPC/block?height=<trust-height> | jq -r '(.result // .).block_id.hash'>"
  ```
  Start, wait for `catching_up: false`, then set `enable = false`.
- Fallback when state sync fails: stop a synced node after H, copy its `data/` (without
  `priv_validator_state.json`) to the lagging node with rsync, start both.
- A lagging node that is still below H on the old binary halts at H with the same log line; swap its
  binary then, as in section 6.

## 9. Go / no-go (T−2 h, and again at H−100)

- [ ] Section 0 table complete and checked by a second person; no key in two homes.
- [ ] Proposal passed; `paxd q upgrade plan` shows `$UPGRADE_NAME` at the height announced.
- [ ] Rehearsal passed on the release binary (`rehearsal-result.env` new_bin_sha256 equals section 3).
- [ ] `sha256sum "$PAXD_BIN.$UPGRADE_NAME"` matches section 3 on every validator and full node host.
- [ ] Validators holding more than two thirds of the voting power, plus margin, confirmed with the
      new binary staged and operators on call before H.
- [ ] Data snapshot space available on every validator host (`df -h` ≥ size of `data/`).
- [ ] Snapshot server configured for the lagging-node resync.
- [ ] Rollback binary (`$PAXD_BIN.pre-$UPGRADE_NAME`) present on every host.

No-go on any unchecked item: cancel the plan by governance before H and re-announce.
