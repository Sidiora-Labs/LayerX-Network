import { DocsLayout } from '@/components/DocsLayout'
import Link from 'next/link'

export default function Consensus() {
  return (
    <DocsLayout>
      <div className="page-header">
        <h1 className="page-title">Consensus</h1>
        <p className="page-description">
          Paxeer's BFT consensus engine with Autobahn, ABCI, node management, state machine, light client, and validator key handling.
        </p>
      </div>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/consensus/</code>
      </div>

      <h2>Overview</h2>

      <p>
        Paxeer uses Byzantine Fault Tolerant (BFT) consensus derived from Tendermint with custom optimizations. The consensus layer provides instant finality with no uncle blocks or reorgs. Blocks are final once committed; there is no probabilistic finality.
      </p>

      <p>
        The consensus implementation lives in <code>paxeer-network/consensus/</code> and includes:
      </p>

      <ul>
        <li><strong>Autobahn:</strong> Custom consensus protocol implementation</li>
        <li><strong>ABCI:</strong> Application Blockchain Interface for state machine communication</li>
        <li><strong>Node:</strong> Full node orchestration and service lifecycle</li>
        <li><strong>State:</strong> Consensus state management and transitions</li>
        <li><strong>Light Client:</strong> Verification without full chain replay</li>
        <li><strong>PrivVal:</strong> Validator private key management</li>
      </ul>

      <h2>Autobahn Consensus</h2>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/consensus/autobahn/</code>
      </div>

      <p>
        Autobahn is Paxeer's consensus protocol implementation. It handles proposal, voting, commit phases, and timeout management. The protocol operates with validator committees and uses quorum certificates (QCs) to advance consensus.
      </p>

      <h3>Core Types</h3>

      <p>
        Autobahn defines consensus message types in <code>consensus/autobahn/types/</code>:
      </p>

      <ul>
        <li><strong>Proposals:</strong> <code>app_proposal.go</code>, <code>lane_proposal.go</code> — block proposals</li>
        <li><strong>Votes:</strong> <code>app_vote.go</code>, <code>lane_vote.go</code>, <code>prepare_vote.go</code>, <code>commit_vote.go</code></li>
        <li><strong>QCs:</strong> <code>app_qc.go</code>, <code>lane_qc.go</code>, <code>prepare_qc.go</code>, <code>commit_qc.go</code> — quorum certificates</li>
        <li><strong>Committee:</strong> <code>committee.go</code> — validator set and voting power</li>
        <li><strong>Timeouts:</strong> <code>timeout.go</code> — round timeout handling</li>
      </ul>

      <h3>Internal Implementation</h3>

      <p>
        The consensus reactor lives in <code>consensus/internal/consensus/</code> and <code>consensus/internal/autobahn/</code>. It coordinates proposal, prevote, precommit, and commit phases, enforces timeout rules, and routes messages between validators.
      </p>

      <h2>ABCI Integration</h2>

      <p>
        The Application Blockchain Interface connects the consensus engine to the Paxeer application. ABCI defines how the consensus layer requests state transitions, queries, and lifecycle hooks:
      </p>

      <ul>
        <li><strong>BeginBlock:</strong> Initialize block execution context</li>
        <li><strong>DeliverTx:</strong> Execute individual transactions</li>
        <li><strong>EndBlock:</strong> Finalize block, validator updates</li>
        <li><strong>Commit:</strong> Persist state, return app hash</li>
        <li><strong>CheckTx:</strong> Validate transactions for mempool admission</li>
        <li><strong>Query:</strong> Read-only state queries</li>
      </ul>

      <p>
        ABCI implementation and proxies live in <code>consensus/internal/proxy/</code>.
      </p>

      <h2>Node</h2>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/consensus/node/</code>
      </div>

      <p>
        The node package orchestrates all consensus services. A full node is defined in <code>node/node.go</code> and includes:
      </p>

      <ul>
        <li><strong>BlockStore:</strong> On-disk block storage</li>
        <li><strong>Mempool:</strong> Transaction pool and validation</li>
        <li><strong>Evidence Pool:</strong> Byzantine behavior tracking</li>
        <li><strong>P2P Router:</strong> Network message routing</li>
        <li><strong>RPC:</strong> Query and broadcast endpoints</li>
        <li><strong>Indexer:</strong> Event and transaction indexing</li>
      </ul>

      <p>
        Node initialization and service wiring is handled in <code>node/setup.go</code>. Seed node configuration lives in <code>node/seed.go</code>.
      </p>

      <h2>State Management</h2>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/consensus/internal/state/</code> and <code>consensus/state/</code>
      </div>

      <p>
        The state machine tracks consensus state across blocks:
      </p>

      <ul>
        <li><strong>Validator Set:</strong> Active validator public keys and voting power</li>
        <li><strong>App Hash:</strong> Root hash of application state</li>
        <li><strong>Last Block:</strong> Height, hash, time of previous block</li>
        <li><strong>Consensus Params:</strong> Block size, time, validator limits</li>
      </ul>

      <p>
        State is persisted in the state store and updated after each commit. State sync (<code>consensus/internal/statesync/</code>) allows fast bootstrapping by downloading state snapshots instead of replaying history.
      </p>

      <h2>Light Client</h2>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/consensus/light/</code>
      </div>

      <p>
        The light client verifies block headers and validator set changes without storing the full chain. It operates by:
      </p>

      <ul>
        <li>Trusting an initial validator set at a known height</li>
        <li>Verifying new headers with threshold signatures (2/3+ voting power)</li>
        <li>Detecting forks and Byzantine validators</li>
        <li>Providing Merkle proofs for key-value queries</li>
      </ul>

      <p>
        Light client implementation includes:
      </p>

      <ul>
        <li><strong>Provider:</strong> <code>light/provider/</code> — header and validator set source</li>
        <li><strong>Store:</strong> <code>light/store/</code> — trusted header storage</li>
        <li><strong>Proxy:</strong> <code>light/proxy/</code> — RPC proxy for light verification</li>
      </ul>

      <h2>Validator Key Management (PrivVal)</h2>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/consensus/privval/</code>
      </div>

      <p>
        PrivVal manages validator signing keys. It provides:
      </p>

      <ul>
        <li><strong>FilePV:</strong> File-based private validator (default)</li>
        <li><strong>Remote Signer:</strong> KMS integration for production validators</li>
        <li><strong>Double-Sign Protection:</strong> Tracks last signed height/round to prevent slashing</li>
      </ul>

      <p>
        The validator key is used to sign votes and proposals. Double-signing the same height/round with different blocks is detectable Byzantine behavior and results in slashing.
      </p>

      <h2>Configuration</h2>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/consensus/config/</code>
      </div>

      <p>
        Consensus configuration is defined in <code>config/config.go</code> and includes:
      </p>

      <ul>
        <li><strong>Autobahn:</strong> <code>autobahn.go</code> — Autobahn-specific parameters</li>
        <li><strong>P2P:</strong> Listen address, seeds, peers, max connections</li>
        <li><strong>Mempool:</strong> Size, cache, broadcast, tx size limits</li>
        <li><strong>Consensus:</strong> Timeouts, block size, evidence age</li>
        <li><strong>Storage:</strong> Database backend selection</li>
      </ul>

      <p>
        Configuration is read from <code>config.toml</code> (see <code>config/toml.go</code>).
      </p>

      <h2>RPC</h2>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/consensus/rpc/</code>
      </div>

      <p>
        The consensus layer exposes RPC endpoints for:
      </p>

      <ul>
        <li><strong>Broadcast:</strong> Submit transactions</li>
        <li><strong>Query:</strong> Block, validator, network info</li>
        <li><strong>Subscription:</strong> WebSocket event streams</li>
      </ul>

      <p>
        RPC implementation includes JSON-RPC (<code>rpc/jsonrpc/</code>) and core endpoints (<code>rpc/coretypes/</code>). Internal RPC coordination lives in <code>consensus/internal/rpc/</code>.
      </p>

      <h2>Network (P2P)</h2>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/consensus/internal/p2p/</code>
      </div>

      <p>
        Peer-to-peer networking handles:
      </p>

      <ul>
        <li><strong>Router:</strong> Message routing and channel management</li>
        <li><strong>PEX:</strong> Peer exchange for discovery (<code>p2p/pex/</code>)</li>
        <li><strong>Channels:</strong> Dedicated channels for consensus, mempool, evidence, block sync</li>
        <li><strong>Transport:</strong> Encrypted connections with peer authentication</li>
      </ul>

      <h2>Utilities</h2>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/consensus/libs/</code>
      </div>

      <p>
        Shared libraries for consensus components:
      </p>

      <ul>
        <li><strong>crypto:</strong> <code>libs/crypto/</code> — ed25519, hashing, merkle proofs</li>
        <li><strong>sync:</strong> <code>libs/sync/</code> — concurrency primitives</li>
        <li><strong>json:</strong> <code>libs/json/</code> — deterministic JSON encoding</li>
        <li><strong>math:</strong> <code>libs/math/</code> — safe math, fractions</li>
        <li><strong>utils:</strong> <code>libs/utils/</code> — Option types, scoped concurrency</li>
      </ul>

      <h2>Next Steps</h2>

      <ul>
        <li><Link href="/engine">Understand the EVM execution engine</Link></li>
        <li><Link href="/operators">Run a validator</Link></li>
        <li><Link href="/configuration">Configure consensus parameters</Link></li>
      </ul>

      <div className="prev-next">
        <Link href="/operators">
          <div className="prev-next-label">Previous</div>
          <div className="prev-next-title">Operators Guide</div>
        </Link>
        <Link href="/engine">
          <div className="prev-next-label">Next</div>
          <div className="prev-next-title">Engine</div>
        </Link>
      </div>
    </DocsLayout>
  )
}
