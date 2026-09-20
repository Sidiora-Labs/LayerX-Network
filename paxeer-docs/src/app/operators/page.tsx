import { DocsLayout } from '@/components/DocsLayout'
import Link from 'next/link'

export default function Operators() {
  return (
    <DocsLayout>
      <div className="page-header">
        <h1 className="page-title">Operators Guide</h1>
        <p className="page-description">
          Production best practices for running Paxeer validators and full nodes.
        </p>
      </div>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/docker/README.md</code>, <code>hpx/README.md</code>, <code>Makefile</code>
      </div>

      <h2>Node Types</h2>

      <table>
        <thead>
          <tr>
            <th>Type</th>
            <th>Purpose</th>
            <th>Hardware</th>
            <th>Network</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td><strong>Validator</strong></td>
            <td>Sign blocks, participate in consensus</td>
            <td>16GB RAM, 8 CPU, 1TB SSD</td>
            <td>Low latency to other validators</td>
          </tr>
          <tr>
            <td><strong>Full Node</strong></td>
            <td>Serve RPC, maintain 100k blocks</td>
            <td>32GB RAM, 16 CPU, 2TB SSD</td>
            <td>Public RPC ports (26657, 8545)</td>
          </tr>
          <tr>
            <td><strong>Archive Node</strong></td>
            <td>Keep complete history</td>
            <td>64GB RAM, 32 CPU, 8TB+ NVMe</td>
            <td>High bandwidth</td>
          </tr>
          <tr>
            <td><strong>Seed Node</strong></td>
            <td>Peer discovery (1000 connections)</td>
            <td>8GB RAM, 4 CPU, 500GB SSD</td>
            <td>High bandwidth, public P2P</td>
          </tr>
        </tbody>
      </table>

      <h2>Production Installation (HPX)</h2>

      <p>
        HPX is the official node installer and peer registry for Paxeer mainnet (<code>hyperpax_125-1</code>). It manages <code>paxd</code> under systemd with checksum verification.
      </p>

      <h3>Install HPX CLI</h3>

      <pre><code>{`curl -sSL https://node.hyperpaxeer.com/get-hpx.sh | sudo bash`}</code></pre>

      <p>
        The installer verifies the HPX CLI against <code>checksums.txt</code> published at <code>node.hyperpaxeer.com</code>.
      </p>

      <h3>Setup Full Node</h3>

      <pre><code>{`HPX_TYPE=fullnode hpx setup`}</code></pre>

      <p>
        This downloads and verifies:
      </p>

      <ul>
        <li><code>paxd</code> binary</li>
        <li>libwasmvm runtimes (x86-64 and AArch64)</li>
        <li><code>genesis.json</code></li>
        <li>Configuration files (<code>config.toml</code>, <code>app.toml</code>)</li>
        <li>Systemd service</li>
      </ul>

      <h3>Setup Validator</h3>

      <pre><code>{`HPX_TYPE=validator hpx setup`}</code></pre>

      <p>
        Validator setup includes minimal configuration (API/gRPC disabled for security).
      </p>

      <h3>HPX Commands</h3>

      <pre><code>{`hpx status          # Node status
hpx info            # Node and chain info
hpx logs            # Follow systemd logs
hpx update          # Update to latest release
hpx peers show      # List known peers
hpx peers refresh   # Refresh peer list from registry
hpx register        # Register node in public peer registry
hpx statesync       # Get state-sync parameters
hpx remove          # Uninstall node`}</code></pre>

      <div className="source-note">
        <strong>Source:</strong> <code>hpx/README.md:10-31</code>
      </div>

      <h2>Docker Cluster (Local Development)</h2>

      <h3>Prerequisites</h3>

      <p>
        <strong>macOS:</strong> <a href="https://docs.docker.com/desktop/install/mac-install/">Docker Desktop</a>
      </p>

      <p>
        <strong>Ubuntu:</strong>
      </p>

      <pre><code>{`# Docker Engine
https://docs.docker.com/engine/install/ubuntu/#install-using-the-repository

# Docker Compose
https://docs.docker.com/compose/install/other/`}</code></pre>

      <h3>Start 4-Node Cluster</h3>

      <p>
        From the <code>paxeer-network/</code> directory:
      </p>

      <pre><code>{`# First time or rebuild:
make docker-cluster-start

# Skip build (if build/paxd exists):
make docker-cluster-start-skipbuild`}</code></pre>

      <p>
        This starts a 4-node cluster with:
      </p>

      <ul>
        <li>Logs in <code>build/generated/logs/paxd-0.log</code>, <code>paxd-1.log</code>, etc.</li>
        <li>Genesis and config in <code>build/generated/</code></li>
        <li>Containers named <code>pax-node-0</code>, <code>pax-node-1</code>, etc.</li>
      </ul>

      <h3>Monitor Logs</h3>

      <pre><code>{`tail -f build/generated/logs/paxd-0.log`}</code></pre>

      <h3>Access Container</h3>

      <pre><code>{`docker ps -a
docker exec -it pax-node-0 /bin/bash`}</code></pre>

      <h3>Start Single Node (Not Recommended)</h3>

      <pre><code>{`make build-docker-node && make run-local-node`}</code></pre>

      <div className="source-note">
        <strong>Source:</strong> <code>docker/README.md:23-54</code>
      </div>

      <h2>Monitoring (Prometheus + Grafana)</h2>

      <h3>Cluster with Monitoring</h3>

      <pre><code>{`make docker-cluster-start-monitoring`}</code></pre>

      <p>
        This starts the 4-node cluster plus Prometheus and Grafana containers.
      </p>

      <h3>Grafana Access</h3>

      <ul>
        <li><strong>URL:</strong> http://localhost:3000</li>
        <li><strong>Login:</strong> admin / admin</li>
      </ul>

      <h3>Stop Monitoring</h3>

      <pre><code>{`make docker-cluster-stop-monitoring`}</code></pre>

      <h3>Standalone Scripts</h3>

      <pre><code>{`# Start Prometheus
./docker/monitornode/scripts/start-prometheus.sh

# Start Grafana
./docker/monitornode/scripts/start-grafana.sh

# Stop
./docker/monitornode/scripts/stop-prometheus.sh
./docker/monitornode/scripts/stop-grafana.sh`}</code></pre>

      <div className="source-note">
        <strong>Source:</strong> <code>docker/README.md:55-82</code>
      </div>

      <h2>State Sync RPC Node</h2>

      <p>
        State sync allows rapid bootstrapping from a snapshot instead of replaying history.
      </p>

      <h3>Requirements</h3>

      <ul>
        <li>4-node cluster running</li>
        <li>Latest block height {'>'} 500</li>
      </ul>

      <h3>Start State Sync Node</h3>

      <pre><code>{`# Start cluster
make docker-cluster-start

# Wait for height > 500
paxd status | jq

# Start state sync node
make run-rpc-node`}</code></pre>

      <div className="source-note">
        <strong>Source:</strong> <code>docker/README.md:84-95</code>
      </div>

      <h2>Data Directories</h2>

      <p>
        Default paxd home: <code>~/.paxd/</code>
      </p>

      <table>
        <thead>
          <tr>
            <th>Path</th>
            <th>Contents</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td><code>config/</code></td>
            <td>config.toml, app.toml, genesis.json, keys</td>
          </tr>
          <tr>
            <td><code>data/</code></td>
            <td>State database, block data</td>
          </tr>
          <tr>
            <td><code>wasm/</code></td>
            <td>Compiled WASM contracts (if using WASM module)</td>
          </tr>
        </tbody>
      </table>

      <h3>Keyring</h3>

      <p>
        Paxeer uses Cosmos SDK keyring for account management:
      </p>

      <pre><code>{`paxd keys add validator
paxd keys list
paxd keys show validator -a`}</code></pre>

      <h2>Validator Operations</h2>

      <h3>Create Validator</h3>

      <pre><code>{`paxd tx staking create-validator \\
  --amount=1000000000000000000uhpx \\
  --pubkey=\$(paxd tendermint show-validator) \\
  --moniker="My Validator" \\
  --chain-id=hyperpax_125-1 \\
  --commission-rate="0.10" \\
  --commission-max-rate="0.20" \\
  --commission-max-change-rate="0.01" \\
  --min-self-delegation="1" \\
  --gas="auto" \\
  --from=validator`}</code></pre>

      <h3>Edit Validator</h3>

      <pre><code>{`paxd tx staking edit-validator \\
  --moniker="New Name" \\
  --website="https://example.com" \\
  --details="Validator description" \\
  --from=validator`}</code></pre>

      <h3>Unjail Validator</h3>

      <pre><code>{`paxd tx slashing unjail \\
  --from=validator \\
  --chain-id=hyperpax_125-1`}</code></pre>

      <h3>Query Validator Info</h3>

      <pre><code>{`paxd query staking validator \$(paxd keys show validator --bech val -a)`}</code></pre>

      <h2>Security Best Practices</h2>

      <h3>Validator Security</h3>

      <ul>
        <li><strong>Sentry nodes:</strong> Run validators behind sentry full nodes</li>
        <li><strong>Firewall:</strong> Block all ports except P2P from sentries</li>
        <li><strong>Key management:</strong> Use hardware security module (HSM) or remote signer</li>
        <li><strong>Monitoring:</strong> Alert on missed blocks, uptime {'<'} 95%</li>
        <li><strong>Backups:</strong> Backup <code>priv_validator_key.json</code> securely offline</li>
      </ul>

      <h3>Network Security</h3>

      <pre><code>{`# Firewall rules (validator behind sentry)
# P2P from sentry IPs only
ufw allow from SENTRY_IP to any port 26656

# Deny all other P2P
ufw deny 26656

# No public RPC on validators
# No public API on validators`}</code></pre>

      <h2>Peer Management</h2>

      <h3>HPX Registry</h3>

      <pre><code>{`# Register your node
hpx register

# Refresh peer list
hpx peers refresh

# View known peers
hpx peers show`}</code></pre>

      <h3>Manual Peer Configuration</h3>

      <p>
        Edit <code>config.toml</code>:
      </p>

      <pre><code>{`[p2p]
# Seed nodes (bootstrap peer discovery)
seeds = "node-id@host:26656,node-id2@host:26656"

# Persistent peers (always maintain connection)
persistent-peers = "node-id@host:26656"

# Private peer IDs (never gossip these)
private-peer-ids = "node-id1,node-id2"`}</code></pre>

      <h3>Get Node ID</h3>

      <pre><code>{`paxd tendermint show-node-id`}</code></pre>

      <h2>Backup and Recovery</h2>

      <h3>Critical Files</h3>

      <table>
        <thead>
          <tr>
            <th>File</th>
            <th>Purpose</th>
            <th>Backup Priority</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td><code>priv_validator_key.json</code></td>
            <td>Validator signing key</td>
            <td>Critical (secure offline)</td>
          </tr>
          <tr>
            <td><code>node_key.json</code></td>
            <td>P2P identity</td>
            <td>Medium</td>
          </tr>
          <tr>
            <td><code>priv_validator_state.json</code></td>
            <td>Last signed block height</td>
            <td>High (prevent double sign)</td>
          </tr>
        </tbody>
      </table>

      <h3>Snapshot Recovery</h3>

      <pre><code>{`# Stop node
sudo systemctl stop paxd

# Reset data (keep config)
paxd tendermint unsafe-reset-all

# Restore from snapshot
wget https://snapshots.example.com/paxeer-snapshot.tar.gz
tar -xzf paxeer-snapshot.tar.gz -C ~/.paxd/data

# Restart
sudo systemctl start paxd`}</code></pre>

      <h2>Upgrade Procedures</h2>

      <h3>Binary Upgrade</h3>

      <pre><code>{`# Stop node
sudo systemctl stop paxd

# Backup current binary
cp \$(which paxd) ~/paxd.backup

# Install new binary
make install  # or HPX update

# Verify version
paxd version

# Restart
sudo systemctl start paxd`}</code></pre>

      <h3>Coordinated Upgrade</h3>

      <p>
        For breaking changes, upgrades are coordinated by governance proposals with halt-height:
      </p>

      <ol>
        <li>Chain halts at specified height</li>
        <li>Operators upgrade binary</li>
        <li>Network resumes with new version</li>
      </ol>

      <h2>Troubleshooting</h2>

      <h3>Node Not Syncing</h3>

      <pre><code>{`# Check sync status
paxd status | jq .SyncInfo

# Check peers
paxd status | jq .SyncInfo.num_peers

# Check logs
journalctl -u paxd -f`}</code></pre>

      <h3>Validator Jailed</h3>

      <pre><code>{`# Check why jailed
paxd query slashing signing-info \$(paxd tendermint show-validator)

# Unjail (after fixing issue)
paxd tx slashing unjail --from=validator`}</code></pre>

      <h3>High Memory Usage</h3>

      <ul>
        <li>Reduce <code>concurrency-workers</code> in app.toml</li>
        <li>Enable pruning if running full node</li>
        <li>Reduce <code>cache-size</code> settings</li>
      </ul>

      <div className="prev-next">
        <Link href="/configuration">
          <div className="prev-next-label">Previous</div>
          <div className="prev-next-title">Configuration</div>
        </Link>
        <Link href="/consensus">
          <div className="prev-next-label">Next</div>
          <div className="prev-next-title">Consensus</div>
        </Link>
      </div>
    </DocsLayout>
  )
}
