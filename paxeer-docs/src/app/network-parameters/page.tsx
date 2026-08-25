import { DocsLayout } from '@/components/DocsLayout'
import Link from 'next/link'

export default function NetworkParameters() {
  return (
    <DocsLayout>
      <div className="page-header">
        <h1 className="page-title">Network Parameters</h1>
        <p className="page-description">
          Key parameters and configuration for Paxeer Network chain ID 125.
        </p>
      </div>

      <h2>Chain Identifiers</h2>

      <table>
        <thead>
          <tr>
            <th>Type</th>
            <th>Value</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td>EVM Chain ID</td>
            <td><code>125</code></td>
          </tr>
          <tr>
            <td>Cosmos Chain ID</td>
            <td><code>hyperpax_125-1</code></td>
          </tr>
          <tr>
            <td>Go Module</td>
            <td><code>github.com/sidiora-labs/paxeer-network</code></td>
          </tr>
        </tbody>
      </table>

      <h2>Assets</h2>

      <table>
        <thead>
          <tr>
            <th>Asset</th>
            <th>Purpose</th>
            <th>Domain</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td><strong>PAX</strong></td>
            <td>Native gas token</td>
            <td>Paxeer L1 only</td>
          </tr>
          <tr>
            <td><strong>USDX</strong></td>
            <td>LayerX settlement unit</td>
            <td>LayerX side channel</td>
          </tr>
          <tr>
            <td><strong>USDL</strong></td>
            <td>Paxeer L1 asset backing USDX</td>
            <td>Paxeer L1 custody contracts</td>
          </tr>
        </tbody>
      </table>

      <p>
        <strong>Note:</strong> There is no LayerX token. USDX is a unit of account on LayerX backed by USDL on Paxeer L1.
      </p>

      <h2>Performance Characteristics</h2>

      <table>
        <thead>
          <tr>
            <th>Metric</th>
            <th>Value</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td>Average block time</td>
            <td>~250ms</td>
          </tr>
          <tr>
            <td>Peak throughput (dual lanes)</td>
            <td>5,000 TPS</td>
          </tr>
          <tr>
            <td>Finality</td>
            <td>Instant (BFT)</td>
          </tr>
          <tr>
            <td>Consensus</td>
            <td>Byzantine Fault Tolerant (Tendermint-based)</td>
          </tr>
        </tbody>
      </table>

      <h2>LayerX Fee Structure</h2>

      <p>
        LayerX (the agent-only side channel) charges fees on activity:
      </p>

      <table>
        <thead>
          <tr>
            <th>Fee Component</th>
            <th>Value</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td>Base fee</td>
            <td>5,000 µUSDX per activity (~½¢ USD)</td>
          </tr>
          <tr>
            <td>Congestion multiplier</td>
            <td>1× to 64×</td>
          </tr>
          <tr>
            <td>Effective range</td>
            <td>5,000 µUSDX to 320,000 µUSDX</td>
          </tr>
        </tbody>
      </table>

      <div className="source-note">
        <strong>Important:</strong> LayerX does <strong>not</strong> have zero fees. The paxeer.app marketing site incorrectly states "zero LayerX fees." LayerX charges 5,000 µUSDX base fee with congestion multiplier.
      </div>

      <h2>Genesis Configuration</h2>

      <p>
        Paxeer uses standard Cosmos SDK genesis with module-specific overrides:
      </p>

      <ul>
        <li><strong>Community tax:</strong> Set to 0 in distribution module</li>
        <li><strong>Genesis import:</strong> Supports streaming import for large state</li>
        <li><strong>Module basics:</strong> EVM, epoch, mint, oracle, store, tokenfactory</li>
      </ul>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/node/genesis.go</code>
      </div>

      <h2>Consensus Parameters</h2>

      <p>
        Paxeer consensus is based on Tendermint BFT:
      </p>

      <ul>
        <li><strong>Byzantine Fault Tolerance:</strong> Up to 1/3 of validators can be faulty</li>
        <li><strong>Instant finality:</strong> No uncle blocks or reorgs</li>
        <li><strong>Dual sequencer lanes:</strong> Parallel transaction ordering</li>
        <li><strong>No PoW:</strong> Proof-of-work is not used</li>
        <li><strong>No pending state:</strong> Transactions finalize immediately</li>
      </ul>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/consensus/</code>
      </div>

      <h2>RPC Compatibility</h2>

      <p>
        Paxeer provides EVM JSON-RPC with notable distinctions from Ethereum:
      </p>

      <ul>
        <li><strong>No pending blocks:</strong> <code>pending</code> is treated as <code>latest</code></li>
        <li><strong>No uncle blocks:</strong> Instant BFT finality means no uncles</li>
        <li><strong>No trie endpoints:</strong> State is not stored in Ethereum-style tries</li>
        <li><strong>No PoW endpoints:</strong> <code>eth_mining</code>, <code>eth_hashrate</code> not supported</li>
        <li><strong>No blobs:</strong> EIP-4844 blob transactions not supported</li>
      </ul>

      <p>
        See <Link href="/json-rpc">JSON-RPC</Link> and <Link href="/json-rpc-unsupported">Unsupported Methods</Link> for details.
      </p>

      <h2>Module List</h2>

      <p>
        Paxeer-specific chain modules:
      </p>

      <ul>
        <li><strong><Link href="/evm">evm</Link>:</strong> Native EVM execution, address association, receipts, pointers, precompile integration</li>
        <li><strong><Link href="/modules/epoch">epoch</Link>:</strong> Time-based hooks and epoch lifecycle management</li>
        <li><strong><Link href="/modules/mint">mint</Link>:</strong> Inflation and native-token minting policy</li>
        <li><strong><Link href="/modules/oracle">oracle</Link>:</strong> Validator exchange-rate voting and price aggregation</li>
        <li><strong><Link href="/modules/store">store</Link>:</strong> Module-level store integration helpers</li>
        <li><strong><Link href="/modules/tokenfactory">tokenfactory</Link>:</strong> Permissioned creation and management of native token denominations</li>
      </ul>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/modules/</code>
      </div>

      <h2>Public Endpoints</h2>

      <div className="source-note">
        <span className="badge badge-warning">Limited Beta</span>
        <p style={{ marginTop: '0.5rem' }}>
          LayerX limited beta opens September 7, 2026. Public RPC endpoints are not yet available. Do not invent or document public LayerX RPC URLs.
        </p>
      </div>

      <p>
        Paxeer mainnet endpoints will be documented here once publicly available.
      </p>

      <h2>Explorer</h2>

      <p>
        PaxScan is the official block explorer for chain ID 125:
      </p>

      <ul>
        <li><strong>URL:</strong> <a href="https://paxscan.io">paxscan.io</a> (when live)</li>
        <li><strong>Chain ID:</strong> 125</li>
      </ul>

      <div className="prev-next">
        <Link href="/paxeer-vs-layerx">
          <div className="prev-next-label">Previous</div>
          <div className="prev-next-title">Paxeer vs LayerX</div>
        </Link>
        <Link href="/installation">
          <div className="prev-next-label">Next</div>
          <div className="prev-next-title">Installation & Build</div>
        </Link>
      </div>
    </DocsLayout>
  )
}
