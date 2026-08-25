import { DocsLayout } from '@/components/DocsLayout'
import Link from 'next/link'

export default function PaxeerVsLayerX() {
  return (
    <DocsLayout>
      <div className="page-header">
        <h1 className="page-title">Paxeer vs LayerX</h1>
        <p className="page-description">
          Understanding the relationship and boundaries between Paxeer L1 and the LayerX agent channel.
        </p>
      </div>

      <h2>Two Distinct Systems</h2>

      <p>
        Paxeer and LayerX are separate systems with different purposes. They sit in the same repository for review convenience, but co-location <strong>does not grant LayerX authority over Paxeer, or the reverse</strong>. Each maintains its own build, release tags, and trust boundary.
      </p>

      <h3>Paxeer (EVM L1, Chain ID 125)</h3>

      <p>
        Paxeer is a standard EVM-compatible Layer 1 blockchain. It provides:
      </p>

      <ul>
        <li><strong>Settlement layer:</strong> Where LayerX checkpoints commit</li>
        <li><strong>Custody domain:</strong> LayerX custody contracts live here</li>
        <li><strong>Public chain:</strong> Standard EVM execution, JSON-RPC, blocks, gas (PAX)</li>
        <li><strong>Settlement contracts:</strong> Bonds, challenges, withdrawals, emergency exits</li>
        <li><strong>Standard finality:</strong> Block-based Byzantine Fault Tolerant consensus</li>
      </ul>

      <div className="source-note">
        <strong>Repository path:</strong> <code>paxeer-network/</code> (this documentation)
      </div>

      <h3>LayerX (Agent Channel)</h3>

      <p>
        LayerX is an agent-only side channel designed for high-frequency microtransactions:
      </p>

      <ul>
        <li><strong>Activity execution:</strong> <code>402LXP</code> payments, balances, receipts</li>
        <li><strong>Agent surfaces:</strong> Agent API, human control plane, SDK</li>
        <li><strong>Not a separate chain:</strong> Batches commit to Paxeer L1</li>
        <li><strong>Fee structure:</strong> 5,000 µUSDX base fee (~½¢), congestion 1×–64×</li>
        <li><strong>Settlement unit:</strong> USDX (backed by USDL on Paxeer L1)</li>
      </ul>

      <div className="source-note">
        <strong>Repository paths:</strong> Root directories <code>src/</code>, <code>include/</code>, <code>agent/</code>, <code>human/</code>, <code>platform/</code>, <code>programs/</code>, <code>interop/</code>
      </div>

      <h2>Key Distinctions</h2>

      <table>
        <thead>
          <tr>
            <th>Aspect</th>
            <th>Paxeer</th>
            <th>LayerX</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td><strong>Type</strong></td>
            <td>EVM L1 blockchain</td>
            <td>Agent-only side channel</td>
          </tr>
          <tr>
            <td><strong>Chain ID</strong></td>
            <td>125</td>
            <td>N/A (batches to Paxeer)</td>
          </tr>
          <tr>
            <td><strong>Native asset</strong></td>
            <td>PAX (gas)</td>
            <td>USDX (backed by USDL)</td>
          </tr>
          <tr>
            <td><strong>Fees</strong></td>
            <td>Standard EVM gas fees in PAX</td>
            <td>5,000 µUSDX base + congestion (NOT zero)</td>
          </tr>
          <tr>
            <td><strong>Access</strong></td>
            <td>Public EVM, anyone can transact</td>
            <td>Agent-only, managed identities</td>
          </tr>
          <tr>
            <td><strong>Block time</strong></td>
            <td>~250ms average</td>
            <td>Activity batching, minutes to L1</td>
          </tr>
          <tr>
            <td><strong>Custody</strong></td>
            <td>Custody domain (settlement contracts)</td>
            <td>Execution only (custody on Paxeer)</td>
          </tr>
          <tr>
            <td><strong>Balance writer</strong></td>
            <td>EVM state machine</td>
            <td><code>402LXP</code> (exclusive authority)</td>
          </tr>
        </tbody>
      </table>

      <h2>Settlement Contracts</h2>

      <p>
        LayerX settlement contracts are deployed <strong>on Paxeer L1</strong> at chain ID 125. These contracts handle:
      </p>

      <ul>
        <li><strong>Custody:</strong> LayerX deposits bind to Paxeer L1 contracts</li>
        <li><strong>Checkpoints:</strong> Periodic LayerX batch commitments</li>
        <li><strong>Bonds:</strong> Guarantor stakes and slashing</li>
        <li><strong>Challenges:</strong> Dispute resolution for invalid batches</li>
        <li><strong>Withdrawals:</strong> Claims to move funds from LayerX back to Paxeer</li>
        <li><strong>Emergency exits:</strong> Protocol-level safety when LayerX is degraded</li>
      </ul>

      <div className="source-note">
        <strong>Contracts path:</strong> <code>contracts/</code> at repository root (not <code>paxeer-network/contracts/</code>)
      </div>

      <p>
        The contracts at <code>paxeer-network/contracts/</code> are <strong>Paxeer-native</strong> utilities (WPAX, pointers, precompile interfaces), not LayerX settlement contracts.
      </p>

      <h2>No LayerX Token</h2>

      <p>
        <strong>There is no LayerX token.</strong> LayerX uses:
      </p>

      <ul>
        <li><strong>USDX:</strong> Unit of account on LayerX (microtransactions)</li>
        <li><strong>USDL:</strong> The Paxeer L1 asset that backs USDX</li>
        <li><strong>PAX:</strong> Paxeer gas token (separate, EVM L1 only)</li>
      </ul>

      <p>
        <code>402LXP</code> is the only balance writer on LayerX. All monetary effects go through this protocol.
      </p>

      <h2>Claim Lock: Accurate Terminology</h2>

      <p>
        When documenting or discussing the system, use precise terms:
      </p>

      <ul>
        <li><strong>Paxeer:</strong> EVM L1, chain ID 125, for LayerX custody/checkpoints/bonds/challenges/exits</li>
        <li><strong>LayerX:</strong> Agent-only side channel with 5,000 µUSDX base fee (~½¢), congestion 1×–64×</li>
        <li><strong>Never "zero fees":</strong> LayerX is not zero-fee (paxeer.app marketing is incorrect on this point)</li>
        <li><strong>Limited beta:</strong> September 7, 2026 (no public RPC yet)</li>
        <li><strong>PAX:</strong> Paxeer gas token only</li>
        <li><strong>USDX / USDL:</strong> LayerX unit and backing asset (no LayerX token)</li>
        <li><strong>Co-location ≠ shared authority:</strong> Same repo, separate systems</li>
      </ul>

      <h2>Ethereum and Solana Mirrors</h2>

      <p>
        Ethereum and Solana mirrors in <code>interop/</code> are <strong>archives</strong>, not settlement venues or custody domains. They publish batch commitments for independent verification but hold no vaults, portals, or custody semantics. <strong>Paxeer remains the exclusive custody and withdrawal guarantee.</strong>
      </p>

      <div className="prev-next">
        <Link href="/">
          <div className="prev-next-label">Previous</div>
          <div className="prev-next-title">Introduction</div>
        </Link>
        <Link href="/network-parameters">
          <div className="prev-next-label">Next</div>
          <div className="prev-next-title">Network Parameters</div>
        </Link>
      </div>
    </DocsLayout>
  )
}
