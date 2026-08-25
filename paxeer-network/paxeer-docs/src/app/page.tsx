import { DocsLayout } from '@/components/DocsLayout'
import Link from 'next/link'

export default function Home() {
  return (
    <DocsLayout>
      <div className="page-header">
        <h1 className="page-title">Paxeer Network</h1>
        <p className="page-description">
          EVM Layer 1 (chain ID 125). The settlement and custody layer for LayerX.
        </p>
      </div>

      <p>
        Paxeer is where LayerX checkpoints, custody, guarantor bonds, challenges, withdrawals, and emergency exits live. Ordinary LayerX activity stays on LayerX. Periodic checkpoints settle here so custody never leaves an L1 that can be replayed independently of the LayerX sequencer.
      </p>

      <p>
        This documentation covers the <code>paxd</code> node, EVM/RPC surface, chain modules, storage engines, and Paxeer-native contracts. The code lives in the <a href="https://github.com/Sidiora-Labs/LayerX-Protocol">Sidiora-Labs/LayerX-Protocol</a> monorepo next to LayerX so the two can be reviewed together. Co-location does not grant LayerX authority over Paxeer, or the reverse. Each side keeps its own build, release tags, and trust boundary.
      </p>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/</code> in the LayerX Protocol monorepo
      </div>

      <h2>How it sits next to LayerX</h2>

      <table>
        <thead>
          <tr>
            <th>Path</th>
            <th>Owns</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td>Repository root (<code>src/</code>, <code>include/</code>, <code>agent/</code>, <code>human/</code>, <code>platform/</code>, <code>programs/</code>, <code>interop/</code>)</td>
            <td>LayerX execution: activities, <code>402LXP</code> balances, receipts, agent and human surfaces</td>
          </tr>
          <tr>
            <td><code>contracts/</code> at the repository root</td>
            <td>LayerX settlement contracts deployed <em>on</em> Paxeer: custody, checkpoints, bonds, claims, disputes, exits</td>
          </tr>
          <tr>
            <td><code>paxeer-network/</code> (this documentation)</td>
            <td>Paxeer Network itself: <code>paxd</code>, EVM execution, JSON-RPC, chain modules, Docker, node distribution</td>
          </tr>
          <tr>
            <td><code>spec/</code></td>
            <td>Normative LayerX specifications (KVX first)</td>
          </tr>
        </tbody>
      </table>

      <p>
        <code>402LXP</code> remains the only LayerX balance writer. There is no LayerX token. Paxeer is the custody domain; Ethereum and Solana mirrors in <code>interop/</code> are archives, not settlement venues.
      </p>

      <p>
        The Cosmos-style chain identifier used by node distribution is <code>hyperpax_125-1</code> (EVM chain ID <code>125</code>). See <Link href="/admin-hpx">Admin & HPX</Link>.
      </p>

      <h2>Key Facts</h2>

      <ul>
        <li><strong>Chain ID:</strong> 125 (EVM)</li>
        <li><strong>Chain identifier:</strong> <code>hyperpax_125-1</code> (Cosmos)</li>
        <li><strong>Native gas token:</strong> PAX</li>
        <li><strong>LayerX unit:</strong> USDX (not a LayerX token)</li>
        <li><strong>LayerX L1 asset:</strong> USDL (backing USDX)</li>
        <li><strong>LayerX base fee:</strong> 5,000 µUSDX per activity (~½¢), congestion 1×–64×</li>
        <li><strong>Status:</strong> Limited beta opens September 7, 2026</li>
      </ul>

      <div className="source-note">
        <strong>Important:</strong> LayerX does <strong>not</strong> have zero fees. The marketing site paxeer.app references zero LayerX fees incorrectly. LayerX charges 5,000 µUSDX base fee per activity with 1×–64× congestion multiplier.
      </div>

      <h2>Layout</h2>

      <table>
        <thead>
          <tr>
            <th>Path</th>
            <th>Purpose</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td><code>daemon/paxd/</code></td>
            <td><code>paxd</code> node binary</td>
          </tr>
          <tr>
            <td><code>node/</code></td>
            <td>Application wiring, genesis, upgrades</td>
          </tr>
          <tr>
            <td><code>modules/</code></td>
            <td>Paxeer chain modules (<code>evm</code>, <code>epoch</code>, <code>mint</code>, <code>oracle</code>, <code>tokenfactory</code>)</td>
          </tr>
          <tr>
            <td><code>rpc/</code></td>
            <td>EVM JSON-RPC compatibility</td>
          </tr>
          <tr>
            <td><code>contracts/</code></td>
            <td>Paxeer-native Solidity (WPAX, pointers, precompile interfaces) — not the LayerX settlement contracts</td>
          </tr>
          <tr>
            <td><code>consensus/</code>, <code>sdk/</code>, <code>storage/</code></td>
            <td>Consensus, Cosmos SDK fork, storage engines</td>
          </tr>
          <tr>
            <td><code>docker/</code></td>
            <td>Local single-node and cluster compose</td>
          </tr>
          <tr>
            <td><code>hpx/</code></td>
            <td>Native <code>paxd</code> distribution and peer registry tooling</td>
          </tr>
          <tr>
            <td><code>docs/</code></td>
            <td>Subtree documentation (OpenAPI/Swagger, RPC notes)</td>
          </tr>
        </tbody>
      </table>

      <h2>Go Module</h2>

      <p>
        <code>github.com/sidiora-labs/paxeer-network</code>
      </p>

      <h2>Next Steps</h2>

      <ul>
        <li><Link href="/paxeer-vs-layerx">Understand Paxeer vs LayerX</Link></li>
        <li><Link href="/network-parameters">Review network parameters</Link></li>
        <li><Link href="/installation">Install and build Paxeer</Link></li>
        <li><Link href="/run-node">Run a node</Link></li>
      </ul>

      <div className="prev-next">
        <div></div>
        <Link href="/paxeer-vs-layerx">
          <div className="prev-next-label">Next</div>
          <div className="prev-next-title">Paxeer vs LayerX</div>
        </Link>
      </div>
    </DocsLayout>
  )
}
