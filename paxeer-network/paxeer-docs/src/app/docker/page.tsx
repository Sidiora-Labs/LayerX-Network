import { DocsLayout } from '@/components/DocsLayout'
import Link from 'next/link'

export default function Docker() {
  return (
    <DocsLayout>
      <div className="page-header">
        <h1 className="page-title">Docker Development</h1>
        <p className="page-description">
          Local Docker clusters, state-sync RPC nodes, and monitoring for Paxeer development and testing.
        </p>
      </div>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/docker/</code>, <code>paxeer-network/Makefile</code>
      </div>

      <h2>Prerequisites</h2>

      <h3>macOS</h3>

      <p>
        Install Docker Desktop from <a href="https://docs.docker.com/desktop/install/mac-install/">docs.docker.com/desktop/install/mac-install/</a>
      </p>

      <h3>Ubuntu</h3>

      <p>
        Install Docker Engine and Docker Compose:
      </p>

      <ul>
        <li>Docker: <a href="https://docs.docker.com/engine/install/ubuntu/">docs.docker.com/engine/install/ubuntu/</a></li>
        <li>Docker Compose: <a href="https://docs.docker.com/compose/install/other/">docs.docker.com/compose/install/other/</a></li>
      </ul>

      <h2>Four-Node Local Cluster</h2>

      <p>
        The standard development setup runs a four-validator Paxeer cluster on a private network (<code>192.168.10.0/24</code>).
      </p>

      <h3>Start Cluster</h3>

      <pre><code>{`# Build and start (first time or after code changes)
make docker-cluster-start

# Quick start (skip build if paxd binary exists)
make docker-cluster-start-skipbuild`}</code></pre>

      <p>
        This launches four <code>pax-node-*</code> containers. Genesis files and logs are generated under <code>build/generated/</code>.
      </p>

      <h3>Node Ports</h3>

      <table>
        <thead>
          <tr>
            <th>Node</th>
            <th>P2P</th>
            <th>RPC</th>
            <th>gRPC</th>
            <th>EVM RPC</th>
            <th>IP</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td>node0</td>
            <td>26656-26658</td>
            <td>26657</td>
            <td>9090-9091</td>
            <td>8545-8546</td>
            <td>192.168.10.10</td>
          </tr>
          <tr>
            <td>node1</td>
            <td>26659-26661</td>
            <td>26660</td>
            <td>9092-9093</td>
            <td>8547-8548</td>
            <td>192.168.10.11</td>
          </tr>
          <tr>
            <td>node2</td>
            <td>26662-26664</td>
            <td>26663</td>
            <td>9094-9095</td>
            <td>8549-8550</td>
            <td>192.168.10.12</td>
          </tr>
          <tr>
            <td>node3</td>
            <td>26665-26667</td>
            <td>26666</td>
            <td>9096-9097</td>
            <td>8551-8552</td>
            <td>192.168.10.13</td>
          </tr>
        </tbody>
      </table>

      <h3>Monitor Logs</h3>

      <pre><code>{`# Tail logs for node 0
tail -f build/generated/logs/paxd-0.log

# View all logs
ls -l build/generated/logs/`}</code></pre>

      <h3>SSH Into Container</h3>

      <pre><code>{`# List containers
docker ps -a

# SSH into a node
docker exec -it pax-node-0 /bin/bash`}</code></pre>

      <h3>Environment Variables</h3>

      <p>
        The cluster supports environment variable configuration:
      </p>

      <table>
        <thead>
          <tr>
            <th>Variable</th>
            <th>Purpose</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td><code>NUM_ACCOUNTS</code></td>
            <td>Number of test accounts to create</td>
          </tr>
          <tr>
            <td><code>SKIP_BUILD</code></td>
            <td>Skip binary rebuild</td>
          </tr>
          <tr>
            <td><code>INVARIANT_CHECK_INTERVAL</code></td>
            <td>State invariant check frequency</td>
          </tr>
          <tr>
            <td><code>UPGRADE_VERSION_LIST</code></td>
            <td>Chain upgrade version schedule</td>
          </tr>
          <tr>
            <td><code>MOCK_BALANCES</code></td>
            <td>Use mock balance data</td>
          </tr>
          <tr>
            <td><code>GIGA_EXECUTOR</code></td>
            <td>Enable Giga executor backend</td>
          </tr>
          <tr>
            <td><code>GIGA_OCC</code></td>
            <td>Enable optimistic concurrency control</td>
          </tr>
          <tr>
            <td><code>RECEIPT_BACKEND</code></td>
            <td>Receipt storage backend</td>
          </tr>
          <tr>
            <td><code>AUTOBAHN</code></td>
            <td>Enable Autobahn optimizations</td>
          </tr>
          <tr>
            <td><code>GIGA_STORAGE</code></td>
            <td>Enable Giga storage engine</td>
          </tr>
          <tr>
            <td><code>GIGA_MIGRATE_FROM_MEMIAVL</code></td>
            <td>Migrate from MEMIAVL to Giga</td>
          </tr>
          <tr>
            <td><code>GIGA_FLATKV_ONLY</code></td>
            <td>Use flat key-value storage only</td>
          </tr>
        </tbody>
      </table>

      <h2>Single Node (Not Recommended)</h2>

      <p>
        For minimal testing only:
      </p>

      <pre><code>{`make build-docker-node && make run-local-node`}</code></pre>

      <p>
        The four-node cluster is preferred for realistic consensus behavior.
      </p>

      <h2>Compose Files</h2>

      <table>
        <thead>
          <tr>
            <th>File</th>
            <th>Purpose</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td><code>docker-compose.yml</code></td>
            <td>Four-node cluster (base configuration)</td>
          </tr>
          <tr>
            <td><code>docker-compose.monitoring.yml</code></td>
            <td>Prometheus + Grafana monitoring overlay</td>
          </tr>
          <tr>
            <td><code>docker-compose.giga-mixed.yml</code></td>
            <td>Mixed Giga/legacy storage testing overlay</td>
          </tr>
        </tbody>
      </table>

      <h2>Monitoring with Prometheus & Grafana</h2>

      <p>
        Start the cluster with monitoring containers:
      </p>

      <pre><code>{`# Start cluster + monitoring together
make docker-cluster-start-monitoring

# Stop cluster + monitoring together
make docker-cluster-stop-monitoring`}</code></pre>

      <p>
        Or run monitoring scripts independently:
      </p>

      <pre><code>{`# Start Prometheus
./docker/monitornode/scripts/start-prometheus.sh

# Start Grafana
./docker/monitornode/scripts/start-grafana.sh

# Stop
./docker/monitornode/scripts/stop-prometheus.sh
./docker/monitornode/scripts/stop-grafana.sh`}</code></pre>

      <h3>Access UIs</h3>

      <ul>
        <li><strong>Grafana:</strong> <code>http://localhost:3000</code> (login: <code>admin</code> / <code>admin</code>)</li>
        <li><strong>Prometheus:</strong> <code>http://localhost:9090</code></li>
      </ul>

      <h2>State Sync RPC Node</h2>

      <p>
        Test state-sync by starting an additional RPC node that syncs from the four-node cluster:
      </p>

      <pre><code>{`# Prerequisite: Start a 4-node cluster
make docker-cluster-start

# Wait until block height exceeds 500 (configurable via app.toml)
paxd status | jq

# Start state-sync RPC node
make run-rpc-node`}</code></pre>

      <p>
        The RPC node bootstraps from a recent snapshot instead of replaying full history.
      </p>

      <div className="source-note">
        <strong>Scripts:</strong> <code>docker/rpcnode/scripts/</code>
      </div>

      <h2>Fast Iteration & Local Development</h2>

      <p>
        Docker mounts local source directories, so you can edit code and rebuild without re-pulling dependencies:
      </p>

      <ol>
        <li>Edit code under <code>paxeer-network/</code> (modules, consensus, SDK, storage, WASM)</li>
        <li>Rebuild the node image: <code>make build-docker-node</code></li>
        <li>Restart the cluster: <code>make docker-cluster-start</code></li>
      </ol>

      <p>
        No <code>go.mod</code> replacements or sibling repositories required. The monorepo includes all dependencies.
      </p>

      <h3>Volumes</h3>

      <p>
        The compose files mount:
      </p>

      <ul>
        <li><code>$PROJECT_HOME</code> → <code>/pax-protocol/pax-chain</code> (source tree)</li>
        <li><code>$GO_PKG_PATH/mod</code> → <code>/root/go/pkg/mod</code> (Go module cache)</li>
        <li><code>$GOCACHE</code> → <code>/root/.cache/go-build</code> (Go build cache)</li>
      </ul>

      <h2>Deployment Scripts</h2>

      <p>
        Each node type has a multi-step initialization flow:
      </p>

      <h3>Local Node</h3>

      <ul>
        <li><code>step0_build.sh</code> — Build <code>paxd</code> binary</li>
        <li><code>step1_configure_init.sh</code> — Initialize node configuration</li>
        <li><code>step2_genesis.sh</code> — Generate genesis file</li>
        <li><code>step3_add_validator_to_genesis.sh</code> — Add validator to genesis</li>
        <li><code>step4_config_override.sh</code> — Override config files (ports, peers)</li>
        <li><code>step5_start_pax.sh</code> — Start <code>paxd</code></li>
        <li><code>deploy.sh</code> — Orchestrates all steps</li>
      </ul>

      <h3>RPC Node</h3>

      <ul>
        <li><code>step0_build.sh</code> — Build <code>paxd</code></li>
        <li><code>step1_configure_init.sh</code> — Initialize for state-sync</li>
        <li><code>step2_start_pax.sh</code> — Start with state-sync enabled</li>
        <li><code>deploy.sh</code> — Orchestrates RPC node setup</li>
      </ul>

      <div className="source-note">
        <strong>Scripts:</strong> <code>docker/localnode/scripts/</code>, <code>docker/rpcnode/scripts/</code>
      </div>

      <h2>Docker Image</h2>

      <p>
        The cluster uses the <code>pax-chain/localnode</code> image built from <code>paxeer-network/Dockerfile</code>. Platform defaults to <code>linux/amd64</code> but can be overridden with <code>DOCKER_PLATFORM</code>.
      </p>

      <h2>Network Configuration</h2>

      <p>
        The cluster creates a <code>localnet</code> bridge network with subnet <code>192.168.10.0/24</code>. Each node has a static IP for deterministic peer configuration.
      </p>

      <div className="prev-next">
        <Link href="/contracts">
          <div className="prev-next-label">Previous</div>
          <div className="prev-next-title">Contracts</div>
        </Link>
        <Link href="/sdk">
          <div className="prev-next-label">Next</div>
          <div className="prev-next-title">SDK</div>
        </Link>
      </div>
    </DocsLayout>
  )
}
