import { DocsLayout } from '@/components/DocsLayout'
import Link from 'next/link'

export default function RunNode() {
  return (
    <DocsLayout>
      <div className="page-header">
        <h1 className="page-title">Run a Node</h1>
        <p className="page-description">
          How to run a Paxeer node (paxd) for validation, archiving, or RPC.
        </p>
      </div>

      <div className="source-note">
        <span className="badge badge-warning">Limited Beta</span>
        <p style={{ marginTop: '0.5rem' }}>
          LayerX limited beta opens September 7, 2026. Validator onboarding and public endpoints not yet available.
        </p>
      </div>

      <h2>Node Types</h2>

      <table>
        <thead>
          <tr>
            <th>Type</th>
            <th>Purpose</th>
            <th>Requirements</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td>Validator</td>
            <td>Participate in consensus, sign blocks</td>
            <td>Stake, uptime, security hardening</td>
          </tr>
          <tr>
            <td>Full Node</td>
            <td>Maintain full chain history, serve RPC</td>
            <td>Disk space, bandwidth</td>
          </tr>
          <tr>
            <td>Archive Node</td>
            <td>Full history + all historical state</td>
            <td>Large disk (multi-TB), high IOPS</td>
          </tr>
          <tr>
            <td>Light Client</td>
            <td>Verify headers, query full nodes</td>
            <td>Minimal resources</td>
          </tr>
        </tbody>
      </table>

      <h2>Prerequisites</h2>

      <ul>
        <li><strong>Binary:</strong> <code>paxd</code> installed (see <Link href="/installation">Installation</Link>)</li>
        <li><strong>System:</strong> Linux or macOS, 8GB+ RAM, 500GB+ disk (full node), 4+ CPU cores</li>
        <li><strong>Network:</strong> Open ports for P2P (26656) and RPC (8545, 26657)</li>
      </ul>

      <h2>Initialize Node</h2>

      <pre><code>{`paxd init <moniker> --chain-id hyperpax_125-1`}</code></pre>

      <p>
        This creates <code>~/.paxd/config/</code> and <code>~/.paxd/data/</code>.
      </p>

      <h2>Configuration</h2>

      <p>
        Key configuration files:
      </p>

      <ul>
        <li><code>~/.paxd/config/config.toml</code> — Consensus, P2P, RPC</li>
        <li><code>~/.paxd/config/app.toml</code> — Application, EVM, API</li>
        <li><code>~/.paxd/config/genesis.json</code> — Genesis state</li>
      </ul>

      <p>
        See <Link href="/configuration">Configuration</Link> for detailed parameter guide.
      </p>

      <h2>Genesis File</h2>

      <p>
        Download the genesis file for mainnet:
      </p>

      <pre><code>{`# Mainnet genesis (when available)
curl -o ~/.paxd/config/genesis.json https://...`}</code></pre>

      <div className="source-note">
        <strong>Note:</strong> Genesis URL will be published when mainnet launches.
      </div>

      <h2>Peers</h2>

      <p>
        Configure seed and persistent peers in <code>config.toml</code>:
      </p>

      <pre><code>{`[p2p]
seeds = "seed1@host:26656,seed2@host:26656"
persistent_peers = "peer1@host:26656,peer2@host:26656"`}</code></pre>

      <p>
        Peer lists will be published when mainnet is live. See <Link href="/admin-hpx">Admin & HPX</Link> for peer registry tooling.
      </p>

      <h2>Start Node</h2>

      <pre><code>{`paxd start`}</code></pre>

      <p>
        This starts the node and begins syncing from genesis or the latest snapshot.
      </p>

      <h3>Systemd Service (Recommended)</h3>

      <p>
        Create <code>/etc/systemd/system/paxd.service</code>:
      </p>

      <pre><code>{`[Unit]
Description=Paxeer Node
After=network.target

[Service]
Type=simple
User=paxeer
ExecStart=/usr/local/bin/paxd start
Restart=on-failure
RestartSec=3
LimitNOFILE=65535

[Install]
WantedBy=multi-user.target`}</code></pre>

      <p>
        Enable and start:
      </p>

      <pre><code>{`sudo systemctl daemon-reload
sudo systemctl enable paxd
sudo systemctl start paxd`}</code></pre>

      <h2>Sync Methods</h2>

      <h3>State Sync</h3>

      <p>
        Fast sync by downloading state snapshots:
      </p>

      <pre><code>{`[statesync]
enable = true
rpc_servers = "https://rpc1.example.com:26657,https://rpc2.example.com:26657"
trust_height = 1000000
trust_hash = "ABC..."`}</code></pre>

      <h3>Snapshot Restore</h3>

      <p>
        Download and extract a data snapshot:
      </p>

      <pre><code>{`paxd tendermint unsafe-reset-all
wget https://snapshots.example.com/paxeer-snapshot.tar.gz
tar -xzf paxeer-snapshot.tar.gz -C ~/.paxd/data`}</code></pre>

      <h2>Monitoring</h2>

      <h3>Check Sync Status</h3>

      <pre><code>{`paxd status | jq .SyncInfo`}</code></pre>

      <h3>Logs</h3>

      <pre><code>{`journalctl -u paxd -f`}</code></pre>

      <h3>Metrics</h3>

      <p>
        Enable Prometheus metrics in <code>config.toml</code>:
      </p>

      <pre><code>{`[instrumentation]
prometheus = true
prometheus_listen_addr = ":26660"`}</code></pre>

      <h2>Validator Setup</h2>

      <p>
        To become a validator:
      </p>

      <ol>
        <li><strong>Sync node:</strong> Wait for full sync (<code>catching_up: false</code>)</li>
        <li><strong>Create validator key:</strong> <code>paxd keys add validator</code></li>
        <li><strong>Fund account:</strong> Send PAX for stake and gas</li>
        <li><strong>Create validator:</strong> Submit <code>create-validator</code> transaction</li>
      </ol>

      <pre><code>{`paxd tx staking create-validator \\
  --amount=1000000000000000000apax \\
  --pubkey=$(paxd tendermint show-validator) \\
  --moniker="My Validator" \\
  --chain-id=hyperpax_125-1 \\
  --commission-rate="0.10" \\
  --commission-max-rate="0.20" \\
  --commission-max-change-rate="0.01" \\
  --min-self-delegation="1" \\
  --gas="auto" \\
  --from=validator`}</code></pre>

      <p>
        See <Link href="/operators">Operators Guide</Link> for validator best practices.
      </p>

      <h2>Docker</h2>

      <p>
        For local testing, use Docker compose:
      </p>

      <pre><code>{`cd paxeer-network/docker
make docker-cluster-start`}</code></pre>

      <p>
        See <Link href="/docker">Docker documentation</Link> for compose configurations.
      </p>

      <div className="prev-next">
        <Link href="/installation">
          <div className="prev-next-label">Previous</div>
          <div className="prev-next-title">Installation & Build</div>
        </Link>
        <Link href="/configuration">
          <div className="prev-next-label">Next</div>
          <div className="prev-next-title">Configuration</div>
        </Link>
      </div>
    </DocsLayout>
  )
}
