import { DocsLayout } from '@/components/DocsLayout'
import Link from 'next/link'

export default function SDK() {
  return (
    <DocsLayout>
      <div className="page-header">
        <h1 className="page-title">Cosmos SDK Fork</h1>
        <p className="page-description">
          Paxeer's in-tree Cosmos SDK fork with Paxeer-specific modifications and extensions.
        </p>
      </div>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/sdk/</code>
      </div>

      <h2>Overview</h2>

      <p>
        Paxeer vendors a fork of the Cosmos SDK directly in the monorepo under <code>paxeer-network/sdk/</code>. This is <strong>not</strong> a published Go module or npm package. It is an in-tree dependency for the <code>paxd</code> binary and Paxeer chain modules.
      </p>

      <h3>Why a Fork?</h3>

      <p>
        Paxeer maintains a fork to support:
      </p>

      <ul>
        <li>Custom storage engines (MEMIAVL, Giga)</li>
        <li>EVM integration and pointer contracts</li>
        <li>Fast iteration without waiting for upstream releases</li>
        <li>Paxeer-specific consensus and execution optimizations</li>
        <li>Co-location with Paxeer modules and WASM runtime</li>
      </ul>

      <h2>Upstream Base</h2>

      <p>
        The fork is based on Cosmos SDK but diverges significantly. It is not compatible with upstream Cosmos SDK releases without careful rebasing and testing.
      </p>

      <div className="source-note">
        <strong>Warning:</strong> Do not assume drop-in compatibility with standard Cosmos SDK tooling or libraries. Paxeer's SDK is a custom fork.
      </div>

      <h2>Directory Structure</h2>

      <table>
        <thead>
          <tr>
            <th>Path</th>
            <th>Purpose</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td><code>baseapp/</code></td>
            <td>Application framework (ABCI, routing, ante handlers)</td>
          </tr>
          <tr>
            <td><code>client/</code></td>
            <td>CLI framework and transaction builders</td>
          </tr>
          <tr>
            <td><code>server/</code></td>
            <td>Node server, gRPC, REST gateway</td>
          </tr>
          <tr>
            <td><code>types/</code></td>
            <td>Core SDK types (messages, coins, errors, context)</td>
          </tr>
          <tr>
            <td><code>x/</code></td>
            <td>Standard Cosmos modules (bank, auth, staking, gov, etc.)</td>
          </tr>
          <tr>
            <td><code>proto/</code></td>
            <td>Protobuf definitions for SDK modules</td>
          </tr>
          <tr>
            <td><code>store/</code></td>
            <td>State store abstraction and implementations</td>
          </tr>
          <tr>
            <td><code>simapp/</code></td>
            <td>Reference application for testing</td>
          </tr>
        </tbody>
      </table>

      <h2>Key Modules</h2>

      <p>
        Paxeer's SDK includes standard Cosmos modules:
      </p>

      <table>
        <thead>
          <tr>
            <th>Module</th>
            <th>Purpose</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td><code>x/auth</code></td>
            <td>Account authentication and transaction signing</td>
          </tr>
          <tr>
            <td><code>x/bank</code></td>
            <td>Token transfers and balance management</td>
          </tr>
          <tr>
            <td><code>x/staking</code></td>
            <td>Proof-of-Stake validator set and delegation</td>
          </tr>
          <tr>
            <td><code>x/distribution</code></td>
            <td>Staking rewards and fee distribution</td>
          </tr>
          <tr>
            <td><code>x/gov</code></td>
            <td>On-chain governance and proposals</td>
          </tr>
          <tr>
            <td><code>x/slashing</code></td>
            <td>Validator penalties for misbehavior</td>
          </tr>
          <tr>
            <td><code>x/crisis</code></td>
            <td>Invariant checking and chain halts</td>
          </tr>
          <tr>
            <td><code>x/evidence</code></td>
            <td>Double-sign evidence submission</td>
          </tr>
          <tr>
            <td><code>x/params</code></td>
            <td>Module parameter management</td>
          </tr>
          <tr>
            <td><code>x/upgrade</code></td>
            <td>Coordinated chain upgrades</td>
          </tr>
        </tbody>
      </table>

      <p>
        Plus Paxeer-specific modules under <code>paxeer-network/modules/</code>:
      </p>

      <ul>
        <li><code>evm</code> — EVM execution and JSON-RPC</li>
        <li><code>epoch</code> — Epoch tracking</li>
        <li><code>oracle</code> — Price feeds</li>
        <li><code>tokenfactory</code> — Custom token creation</li>
        <li><code>mint</code> — Token minting</li>
      </ul>

      <h2>BaseApp</h2>

      <p>
        The core application framework (<code>baseapp/</code>) handles:
      </p>

      <ul>
        <li>ABCI interface to Tendermint consensus</li>
        <li>Message routing to module handlers</li>
        <li>Ante handlers (gas, signatures, nonces)</li>
        <li>Transaction execution and state commits</li>
        <li>Query routing</li>
      </ul>

      <p>
        Paxeer extends BaseApp for EVM transaction routing and concurrent execution (OCC).
      </p>

      <h2>Client & Server</h2>

      <p>
        The SDK provides:
      </p>

      <ul>
        <li><strong>CLI:</strong> <code>paxd</code> command-line interface (transaction builders, queries)</li>
        <li><strong>gRPC:</strong> Module query and transaction services</li>
        <li><strong>REST:</strong> HTTP gateway (gRPC-Gateway)</li>
        <li><strong>Tendermint RPC:</strong> Block and transaction queries</li>
      </ul>

      <p>
        See <Link href="/rest-grpc">REST & gRPC</Link> for API documentation.
      </p>

      <h2>State Store</h2>

      <p>
        The SDK's <code>store/</code> abstraction is implemented by Paxeer's custom storage engines:
      </p>

      <ul>
        <li><strong>MEMIAVL:</strong> In-memory IAVL tree (legacy)</li>
        <li><strong>Giga:</strong> High-performance flat key-value store</li>
      </ul>

      <p>
        See <Link href="/storage">Storage</Link> for engine details.
      </p>

      <h2>Protobuf Definitions</h2>

      <p>
        SDK types are defined in <code>sdk/proto/</code>. Regenerate Go code with:
      </p>

      <pre><code>{`cd paxeer-network
ignite generate proto-go`}</code></pre>

      <p>
        Requires Ignite CLI v0.23.0. See <code>paxeer-network/api/README.md</code>.
      </p>

      <h2>Building Against the SDK</h2>

      <p>
        Paxeer modules import the SDK from the in-tree path:
      </p>

      <pre><code>{`import (
    sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
    "github.com/sidiora-labs/paxeer-network/sdk/store"
)`}</code></pre>

      <p>
        No <code>go.mod</code> replacement directives are needed because the SDK is part of the monorepo.
      </p>

      <h2>Makefile Targets</h2>

      <p>
        The repository Makefile includes SDK-related targets:
      </p>

      <pre><code>{`# Build paxd
make build

# Run tests
make test

# Generate proto
make proto-gen`}</code></pre>

      <h2>Documentation</h2>

      <p>
        The SDK fork includes upstream Cosmos SDK documentation under <code>sdk/docs/</code>. Note that Paxeer-specific modifications may not be fully reflected in those docs.
      </p>

      <h3>Upstream Resources</h3>

      <ul>
        <li><a href="https://docs.cosmos.network/">Cosmos SDK Docs</a> (upstream, may diverge from Paxeer)</li>
        <li><a href="https://tutorials.cosmos.network/">Cosmos Tutorials</a> (upstream concepts apply)</li>
        <li><a href="https://pkg.go.dev/github.com/cosmos/cosmos-sdk">GoDoc</a> (upstream SDK, not Paxeer's fork)</li>
      </ul>

      <div className="source-note">
        <strong>Warning:</strong> Upstream documentation describes the standard Cosmos SDK, not Paxeer's fork. Treat it as reference only.
      </div>

      <h2>Contributing to the Fork</h2>

      <p>
        Changes to <code>paxeer-network/sdk/</code> affect the entire Paxeer chain. Test thoroughly:
      </p>

      <ol>
        <li>Edit code under <code>sdk/</code></li>
        <li>Rebuild <code>paxd</code>: <code>make build</code></li>
        <li>Run unit tests: <code>make test</code></li>
        <li>Test with <Link href="/docker">Docker cluster</Link>: <code>make docker-cluster-start</code></li>
        <li>Run integration tests: <code>make test-integration</code></li>
      </ol>

      <h2>Differences from Upstream Cosmos SDK</h2>

      <p>
        Paxeer's SDK fork diverges from upstream in several areas:
      </p>

      <ul>
        <li>Custom storage backends (MEMIAVL, Giga)</li>
        <li>EVM module integration and pointer contracts</li>
        <li>Optimistic concurrency control (OCC) for parallel execution</li>
        <li>Receipt indexing and synthetic transactions</li>
        <li>Paxeer-specific ante handlers and gas metering</li>
        <li>Chain-specific upgrade logic</li>
      </ul>

      <p>
        Do not assume compatibility with standard Cosmos SDK tooling (e.g., CosmJS, Keplr) without testing.
      </p>

      <h2>Go Module Path</h2>

      <pre><code>{`github.com/sidiora-labs/paxeer-network/sdk`}</code></pre>

      <p>
        This is an internal module within the Paxeer monorepo, not a standalone published module.
      </p>

      <div className="prev-next">
        <Link href="/docker">
          <div className="prev-next-label">Previous</div>
          <div className="prev-next-title">Docker</div>
        </Link>
        <Link href="/interchain">
          <div className="prev-next-label">Next</div>
          <div className="prev-next-title">Interchain</div>
        </Link>
      </div>
    </DocsLayout>
  )
}
