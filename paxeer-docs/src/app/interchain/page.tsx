import { DocsLayout } from '@/components/DocsLayout'
import Link from 'next/link'

export default function Interchain() {
  return (
    <DocsLayout>
      <div className="page-header">
        <h1 className="page-title">Interchain (IBC)</h1>
        <p className="page-description">
          Inter-Blockchain Communication protocol implementation on Paxeer.
        </p>
      </div>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/interchain/</code>
      </div>

      <h2>Overview</h2>

      <p>
        Paxeer includes an in-tree fork of <a href="https://github.com/cosmos/ibc-go">ibc-go</a>, the Cosmos SDK's Inter-Blockchain Communication (IBC) protocol implementation. This enables Paxeer to connect with other IBC-enabled chains for cross-chain asset transfers and messaging.
      </p>

      <h3>What is IBC?</h3>

      <p>
        IBC is an end-to-end, connection-oriented, stateful protocol for reliable, ordered, and authenticated communication between heterogeneous blockchains. It handles transport across different sovereign blockchains without relying on a trusted third party.
      </p>

      <h2>Location</h2>

      <p>
        The IBC implementation is vendored under <code>paxeer-network/interchain/</code> within the monorepo. This is <strong>not</strong> a standalone published module but an in-tree dependency for <code>paxd</code>.
      </p>

      <h2>Core IBC Components</h2>

      <p>
        Paxeer's IBC implementation includes the standard IBC stack:
      </p>

      <table>
        <thead>
          <tr>
            <th>Component</th>
            <th>ICS Spec</th>
            <th>Purpose</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td><strong>Client</strong></td>
            <td>ICS-02</td>
            <td>Light client verification of remote chain state</td>
          </tr>
          <tr>
            <td><strong>Connection</strong></td>
            <td>ICS-03</td>
            <td>Establish authenticated connections between chains</td>
          </tr>
          <tr>
            <td><strong>Channel</strong></td>
            <td>ICS-04</td>
            <td>Packet delivery over connections (ordered/unordered)</td>
          </tr>
          <tr>
            <td><strong>Port</strong></td>
            <td>ICS-05</td>
            <td>Module registration and packet routing</td>
          </tr>
          <tr>
            <td><strong>Commitment</strong></td>
            <td>ICS-23</td>
            <td>Cryptographic commitment proofs</td>
          </tr>
          <tr>
            <td><strong>Host</strong></td>
            <td>ICS-24</td>
            <td>Chain-specific requirements and interfaces</td>
          </tr>
        </tbody>
      </table>

      <h2>IBC Applications</h2>

      <p>
        Paxeer supports standard IBC applications:
      </p>

      <h3>Fungible Token Transfers (ICS-20)</h3>

      <p>
        The <code>transfer</code> module enables cross-chain token transfers. Assets originating on Paxeer can be sent to other IBC chains, and assets from remote chains can be received on Paxeer as IBC-wrapped tokens.
      </p>

      <h3>Interchain Accounts (ICS-27)</h3>

      <p>
        Interchain Accounts allow one chain to control an account on another chain. This enables cross-chain governance, remote staking, and other advanced use cases.
      </p>

      <h2>Light Clients</h2>

      <p>
        Paxeer includes light client implementations for verifying remote chain state:
      </p>

      <table>
        <thead>
          <tr>
            <th>Client</th>
            <th>ICS Spec</th>
            <th>Purpose</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td><strong>Tendermint</strong></td>
            <td>ICS-07</td>
            <td>Light client for Tendermint/CometBFT chains</td>
          </tr>
          <tr>
            <td><strong>Solo Machine</strong></td>
            <td>ICS-06</td>
            <td>Light client for single-signer accounts (e.g., hardware wallets)</td>
          </tr>
        </tbody>
      </table>

      <div className="source-note">
        <strong>Note:</strong> The localhost client is currently non-functional in this fork.
      </div>

      <h2>IBC Module Structure</h2>

      <p>
        The <code>interchain/</code> directory mirrors the structure of upstream <code>ibc-go</code>:
      </p>

      <table>
        <thead>
          <tr>
            <th>Path</th>
            <th>Purpose</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td><code>modules/core/</code></td>
            <td>Core IBC protocol (client, connection, channel)</td>
          </tr>
          <tr>
            <td><code>modules/apps/</code></td>
            <td>IBC applications (transfer, interchain accounts)</td>
          </tr>
          <tr>
            <td><code>modules/light-clients/</code></td>
            <td>Light client implementations</td>
          </tr>
          <tr>
            <td><code>proto/</code></td>
            <td>Protobuf definitions for IBC types</td>
          </tr>
        </tbody>
      </table>

      <h2>Using IBC on Paxeer</h2>

      <h3>Establishing a Connection</h3>

      <p>
        To connect Paxeer with another IBC-enabled chain:
      </p>

      <ol>
        <li>Create a light client for the remote chain</li>
        <li>Establish a connection using the client</li>
        <li>Create a channel over the connection</li>
        <li>Begin sending packets</li>
      </ol>

      <p>
        Use the <code>paxd tx ibc</code> CLI commands or relayer software (e.g., Hermes, Go Relayer).
      </p>

      <h3>Cross-Chain Token Transfers</h3>

      <pre><code>{`# Send 1000upax from Paxeer to another chain
paxd tx ibc-transfer transfer \\
  transfer \\
  channel-0 \\
  cosmos1recipient... \\
  1000upax \\
  --from mykey \\
  --chain-id hyperpax_125-1`}</code></pre>

      <h3>Querying IBC State</h3>

      <pre><code>{`# List all IBC clients
paxd query ibc client states

# List all connections
paxd query ibc connection connections

# List all channels
paxd query ibc channel channels`}</code></pre>

      <h2>Relayers</h2>

      <p>
        IBC requires off-chain relayer software to ferry packets between chains. Relayers monitor both chains and submit proof-carrying packets to complete cross-chain transfers.
      </p>

      <h3>Supported Relayers</h3>

      <ul>
        <li><a href="https://github.com/informalsystems/hermes">Hermes</a> (Rust)</li>
        <li><a href="https://github.com/cosmos/relayer">Go Relayer</a> (Go)</li>
      </ul>

      <p>
        Relayers must be configured with Paxeer's chain ID (<code>hyperpax_125-1</code>) and RPC endpoints.
      </p>

      <h2>IBC and EVM</h2>

      <p>
        IBC operates at the Cosmos SDK level. EVM contracts on Paxeer cannot directly send or receive IBC packets. To bridge EVM and IBC:
      </p>

      <ul>
        <li>Use pointer contracts to expose Cosmos tokens to EVM</li>
        <li>Use precompiles to call Cosmos modules from EVM</li>
        <li>Deploy bridge contracts that interact with IBC via Cosmos transactions</li>
      </ul>

      <p>
        See <Link href="/contracts">Contracts</Link> for pointer contract details.
      </p>

      <h2>IBC Protocol Versions</h2>

      <p>
        Paxeer's IBC fork is based on <code>ibc-go</code> but may diverge from upstream. Check the version in <code>interchain/go.mod</code> or the README.
      </p>

      <div className="source-note">
        <strong>Warning:</strong> Paxeer's IBC fork may not be compatible with the latest upstream <code>ibc-go</code> releases. Test cross-chain compatibility carefully.
      </div>

      <h2>Security Considerations</h2>

      <p>
        IBC is a trust-minimized protocol, but security depends on:
      </p>

      <ul>
        <li><strong>Light client correctness:</strong> Remote chain state must be correctly verified</li>
        <li><strong>Relayer liveness:</strong> Packets must be relayed promptly to avoid timeouts</li>
        <li><strong>Chain sovereignty:</strong> Each chain controls its own IBC modules and upgrades</li>
      </ul>

      <p>
        Ensure relayers are operated by trusted parties or use multiple independent relayers.
      </p>

      <h2>IBC Upgrades</h2>

      <p>
        IBC modules can be upgraded via on-chain governance. Upgrades must be coordinated with connected chains to avoid breaking connections.
      </p>

      <h2>Resources</h2>

      <ul>
        <li><a href="https://ibcprotocol.org/">IBC Protocol Website</a></li>
        <li><a href="https://github.com/cosmos/ibc">IBC Specification</a></li>
        <li><a href="https://ibc.cosmos.network/">IBC Documentation</a></li>
        <li><a href="https://github.com/cosmos/ibc-go">ibc-go Repository</a> (upstream)</li>
      </ul>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/interchain/README.md</code>
      </div>

      <div className="prev-next">
        <Link href="/sdk">
          <div className="prev-next-label">Previous</div>
          <div className="prev-next-title">SDK</div>
        </Link>
        <Link href="/admin-hpx">
          <div className="prev-next-label">Next</div>
          <div className="prev-next-title">Admin & HPX</div>
        </Link>
      </div>
    </DocsLayout>
  )
}
