import { DocsLayout } from '@/components/DocsLayout'
import Link from 'next/link'

export default function AdminHpx() {
  return (
    <DocsLayout>
      <div className="page-header">
        <h1 className="page-title">Admin & HPX</h1>
        <p className="page-description">
          Runtime administration gRPC service and HyperPax native node distribution.
        </p>
      </div>

      <div className="source-note">
        <strong>Admin:</strong> <code>paxeer-network/admin/</code><br />
        <strong>HPX:</strong> <code>paxeer-network/hpx/</code>
      </div>

      <h2>Admin gRPC Service</h2>

      <p>
        The Admin service provides runtime log level control for <code>paxd</code> without restarting the node. It runs on a dedicated loopback-only gRPC server for security.
      </p>

      <h3>Configuration</h3>

      <p>
        Enable in <code>app.toml</code>:
      </p>

      <pre><code>{`[admin_server]
admin_enabled = true
admin_address = "127.0.0.1:9095"`}</code></pre>

      <p>
        The address <strong>must</strong> be a loopback address (<code>127.0.0.1</code> or <code>::1</code>). Binding to external interfaces is rejected at startup.
      </p>

      <h3>gRPC Methods</h3>

      <table>
        <thead>
          <tr>
            <th>Method</th>
            <th>Request</th>
            <th>Response</th>
            <th>Purpose</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td><code>SetLogLevel</code></td>
            <td><code>pattern</code>, <code>level</code></td>
            <td><code>affected</code> count</td>
            <td>Change log level for loggers matching pattern</td>
          </tr>
          <tr>
            <td><code>GetLogLevel</code></td>
            <td><code>logger</code></td>
            <td><code>level</code></td>
            <td>Get current log level for a logger</td>
          </tr>
          <tr>
            <td><code>ListLoggers</code></td>
            <td><code>prefix</code> (optional)</td>
            <td>List of loggers</td>
            <td>List all loggers and their levels</td>
          </tr>
        </tbody>
      </table>

      <h3>SetLogLevel Pattern Matching</h3>

      <p>
        The <code>pattern</code> parameter supports:
      </p>

      <ul>
        <li><strong>Exact match:</strong> <code>"evm"</code> (sets log level for the EVM logger only)</li>
        <li><strong>Glob:</strong> <code>"evm*"</code> (sets log level for all loggers starting with "evm")</li>
        <li><strong>All loggers:</strong> <code>"*"</code> (sets log level for every logger)</li>
      </ul>

      <h3>Log Levels</h3>

      <table>
        <thead>
          <tr>
            <th>Level</th>
            <th>Purpose</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td><code>debug</code></td>
            <td>Verbose debugging information</td>
          </tr>
          <tr>
            <td><code>info</code></td>
            <td>General operational information</td>
          </tr>
          <tr>
            <td><code>warn</code></td>
            <td>Warning messages (potential issues)</td>
          </tr>
          <tr>
            <td><code>error</code></td>
            <td>Error messages (failures)</td>
          </tr>
        </tbody>
      </table>

      <h3>Example: Using grpcurl</h3>

      <pre><code>{`# List all loggers
grpcurl -plaintext localhost:9095 \\
  paxprotocol.paxchain.admin.v0.AdminService/ListLoggers

# Set EVM logger to debug
grpcurl -plaintext -d '{"pattern":"evm","level":"debug"}' \\
  localhost:9095 \\
  paxprotocol.paxchain.admin.v0.AdminService/SetLogLevel

# Get log level for a specific logger
grpcurl -plaintext -d '{"logger":"evm"}' \\
  localhost:9095 \\
  paxprotocol.paxchain.admin.v0.AdminService/GetLogLevel`}</code></pre>

      <h3>Security</h3>

      <p>
        The Admin service is loopback-only by design:
      </p>

      <ul>
        <li>No authentication required (loopback is trusted)</li>
        <li>Cannot be bound to external interfaces</li>
        <li>Must access from the same machine as <code>paxd</code></li>
      </ul>

      <p>
        For remote access, use SSH port forwarding:
      </p>

      <pre><code>{`ssh -L 9095:127.0.0.1:9095 user@node-ip`}</code></pre>

      <div className="source-note">
        <strong>Proto:</strong> <code>paxeer-network/api/pax/admin/v0/admin.proto</code><br />
        <strong>Implementation:</strong> <code>paxeer-network/admin/server.go</code>, <code>service.go</code>, <code>config.go</code>
      </div>

      <h2>HPX: HyperPax Node Distribution</h2>

      <p>
        HPX is the public installer, node manager, and peer registry for HyperPax (<code>hyperpax_125-1</code>). Nodes run a native <code>paxd</code> binary under systemd. All artifacts are published outside Git.
      </p>

      <h3>Chain Identifier</h3>

      <p>
        <code>hyperpax_125-1</code> (Cosmos-style, EVM chain ID <code>125</code>)
      </p>

      <h3>Install a Node</h3>

      <pre><code>{`curl -sSL https://node.hyperpaxeer.com/get-hpx.sh | sudo bash`}</code></pre>

      <p>
        The installer:
      </p>

      <ol>
        <li>Downloads and verifies the HPX CLI against <code>checksums.txt</code></li>
        <li>Places <code>hpx</code> in <code>/usr/local/bin/</code></li>
      </ol>

      <h3>Setup a Node</h3>

      <pre><code>{`# Set node type (fullnode or validator)
export HPX_TYPE=fullnode

# Run setup
hpx setup`}</code></pre>

      <p>
        The setup flow:
      </p>

      <ol>
        <li>Downloads <code>paxd</code>, <code>libwasmvm</code> runtimes, genesis, and configuration</li>
        <li>Verifies all artifacts against <code>checksums.txt</code></li>
        <li>Installs artifacts to <code>/root/.paxeer/</code></li>
        <li>Configures systemd service</li>
        <li>Starts <code>paxd</code></li>
      </ol>

      <h3>HPX CLI Commands</h3>

      <table>
        <thead>
          <tr>
            <th>Command</th>
            <th>Purpose</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td><code>hpx status</code></td>
            <td>Show node sync status</td>
          </tr>
          <tr>
            <td><code>hpx info</code></td>
            <td>Show node configuration</td>
          </tr>
          <tr>
            <td><code>hpx logs</code></td>
            <td>Tail <code>paxd</code> logs</td>
          </tr>
          <tr>
            <td><code>hpx update</code></td>
            <td>Update <code>paxd</code> and libraries</td>
          </tr>
          <tr>
            <td><code>hpx peers show</code></td>
            <td>Show known peers</td>
          </tr>
          <tr>
            <td><code>hpx peers refresh</code></td>
            <td>Fetch latest peer list from registry</td>
          </tr>
          <tr>
            <td><code>hpx register</code></td>
            <td>Announce node to public registry</td>
          </tr>
          <tr>
            <td><code>hpx statesync</code></td>
            <td>Enable state-sync for fast bootstrap</td>
          </tr>
          <tr>
            <td><code>hpx remove</code></td>
            <td>Uninstall node and clean up</td>
          </tr>
        </tbody>
      </table>

      <h3>Node Types</h3>

      <table>
        <thead>
          <tr>
            <th>Type</th>
            <th>Purpose</th>
            <th>Config</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td><code>fullnode</code></td>
            <td>Non-validating full node (RPC, indexing)</td>
            <td><code>/config/fullnode/</code></td>
          </tr>
          <tr>
            <td><code>validator</code></td>
            <td>Validating node (must register validator key)</td>
            <td><code>/config/validator/</code></td>
          </tr>
        </tbody>
      </table>

      <h3>Public Registry</h3>

      <p>
        The HPX registry at <code>https://node.hyperpaxeer.com</code> provides:
      </p>

      <table>
        <thead>
          <tr>
            <th>Endpoint</th>
            <th>Purpose</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td><code>GET /healthz</code></td>
            <td>Registry liveness, chain and source revision</td>
          </tr>
          <tr>
            <td><code>GET /checksums.txt</code></td>
            <td>SHA-256 checksums for all artifacts</td>
          </tr>
          <tr>
            <td><code>GET /chain-info.json</code></td>
            <td>Chain metadata (chain ID, genesis hash)</td>
          </tr>
          <tr>
            <td><code>GET /paxd</code></td>
            <td>Native <code>paxd</code> binary</td>
          </tr>
          <tr>
            <td><code>GET /lib/*.so</code></td>
            <td>Architecture-specific <code>libwasmvm</code> runtimes</td>
          </tr>
          <tr>
            <td><code>GET /genesis.json</code></td>
            <td>Chain genesis file</td>
          </tr>
          <tr>
            <td><code>GET /config/&lt;type&gt;/&lt;file&gt;</code></td>
            <td>Node configuration files (<code>config.toml</code>, <code>app.toml</code>)</td>
          </tr>
          <tr>
            <td><code>GET /api/myip</code></td>
            <td>Caller's public IP address</td>
          </tr>
          <tr>
            <td><code>POST /api/register</code></td>
            <td>Announce node's public peer address</td>
          </tr>
          <tr>
            <td><code>GET /api/peers</code></td>
            <td>JSON list of registered peers</td>
          </tr>
          <tr>
            <td><code>GET /api/peers.txt</code></td>
            <td>Text list of peer addresses (Tendermint format)</td>
          </tr>
          <tr>
            <td><code>GET /api/nodes</code></td>
            <td>Detailed node metadata</td>
          </tr>
          <tr>
            <td><code>GET /api/statesync</code></td>
            <td>Current state-sync trust parameters</td>
          </tr>
        </tbody>
      </table>

      <h3>Artifact Publishing</h3>

      <p>
        To publish a new <code>paxd</code> release or chain configuration:
      </p>

      <pre><code>{`# From the monorepo root
sudo paxeer-network/hpx/publish.sh`}</code></pre>

      <p>
        This script:
      </p>

      <ol>
        <li>Collects <code>paxd</code> binary from <code>build/paxd</code></li>
        <li>Collects native libraries from <code>wasm-runtime/</code> and <code>wasm/x/wasm/artifacts/</code></li>
        <li>Collects live chain configuration from <code>/root/.paxeer/config/</code> (or <code>$SRC_CFG</code>)</li>
        <li>Stages artifacts in <code>/srv/hpx/artifacts/releases/&lt;release-id&gt;/</code></li>
        <li>Generates <code>checksums.txt</code></li>
        <li>Atomically moves <code>current</code> symlink to new release</li>
      </ol>

      <p>
        Failed staging runs never change the served release.
      </p>

      <h3>Registry Runtime Deployment</h3>

      <p>
        Changes to the registry service under <code>paxeer-network/hpx/registry/</code> trigger the GitHub workflow <code>Paxeer / HPX Registry</code>, which:
      </p>

      <ol>
        <li>Builds Linux executables (x86-64, AArch64)</li>
        <li>Publishes them as GitHub release assets</li>
        <li>Publishes a multi-architecture GHCR image</li>
      </ol>

      <p>
        Deploy the registry on the public origin host:
      </p>

      <pre><code>{`sudo paxeer-network/hpx/hosting/deploy.sh`}</code></pre>

      <p>
        This script:
      </p>

      <ol>
        <li>Downloads and verifies the latest registry executable</li>
        <li>Installs it as a systemd service (loopback-only)</li>
        <li>Obtains a Let's Encrypt certificate for <code>node.hyperpaxeer.com</code></li>
        <li>Configures Nginx reverse proxy with rate-limiting</li>
      </ol>

      <p>
        Registry state is persisted at <code>/srv/hpx/data/registry.json</code>.
      </p>

      <h3>Registry Authentication</h3>

      <p>
        Registration is public by default. To require a token:
      </p>

      <pre><code>{`export HPX_REGISTER_TOKEN=your-secret-token
sudo paxeer-network/hpx/hosting/deploy.sh`}</code></pre>

      <p>
        The registry will then require <code>X-HPX-Token: your-secret-token</code> header on <code>POST /api/register</code>.
      </p>

      <h3>Using an Alternate Mirror</h3>

      <p>
        Set <code>HPX_MIRROR</code> only when operating an explicitly trusted alternate mirror:
      </p>

      <pre><code>{`export HPX_MIRROR=https://your-mirror.example.com
hpx setup`}</code></pre>

      <div className="source-note">
        <strong>Warning:</strong> The mirror must serve the same artifact structure and checksums. Untrusted mirrors can serve malicious binaries.
      </div>

      <h3>Native Libraries</h3>

      <p>
        HPX distributes architecture-specific <code>libwasmvm</code> runtimes:
      </p>

      <ul>
        <li><code>libwasmvm.x86_64.so</code> — x86-64 Linux</li>
        <li><code>libwasmvm.aarch64.so</code> — AArch64 Linux</li>
        <li><code>libwasmvm_muslc.x86_64.so</code> — x86-64 musl (Alpine)</li>
        <li><code>libwasmvm_muslc.aarch64.so</code> — AArch64 musl</li>
      </ul>

      <p>
        The installer detects the host architecture and installs the correct library.
      </p>

      <h3>State Sync</h3>

      <p>
        New nodes can bootstrap from a recent state snapshot instead of replaying full history:
      </p>

      <pre><code>{`hpx statesync`}</code></pre>

      <p>
        This fetches trust parameters from <code>/api/statesync</code> and updates <code>config.toml</code> to enable state-sync.
      </p>

      <h3>Uninstall</h3>

      <pre><code>{`hpx remove`}</code></pre>

      <p>
        This stops <code>paxd</code>, removes the systemd service, and deletes <code>/root/.paxeer/</code>.
      </p>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/hpx/README.md</code>, <code>paxeer-network/hpx/hosting/</code>
      </div>

      <div className="prev-next">
        <Link href="/interchain">
          <div className="prev-next-label">Previous</div>
          <div className="prev-next-title">Interchain</div>
        </Link>
        <div></div>
      </div>
    </DocsLayout>
  )
}
