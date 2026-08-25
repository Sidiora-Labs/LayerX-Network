import { DocsLayout } from '@/components/DocsLayout'
import Link from 'next/link'

export default function Contracts() {
  return (
    <DocsLayout>
      <div className="page-header">
        <h1 className="page-title">Contracts</h1>
        <p className="page-description">
          Paxeer-native Solidity contracts vs LayerX settlement contracts.
        </p>
      </div>

      <div className="source-note">
        <strong>Paxeer-native:</strong> <code>paxeer-network/contracts/</code> (WPAX, pointers, precompile interfaces)
        <br />
        <strong>LayerX settlement:</strong> <code>contracts/</code> at repository root (custody, checkpoints, bonds)
      </div>

      <h2>Two Contract Trees</h2>

      <ul>
        <li><strong>paxeer-network/contracts/</strong> — Paxeer-native utilities (WPAX, pointers)</li>
        <li><strong>contracts/</strong> (root) — LayerX settlement contracts deployed on Paxeer</li>
      </ul>

      <div className="prev-next">
        <Link href="/rest-grpc">
          <div className="prev-next-label">Previous</div>
          <div className="prev-next-title">REST & gRPC</div>
        </Link>
        <Link href="/docker">
          <div className="prev-next-label">Next</div>
          <div className="prev-next-title">Docker</div>
        </Link>
      </div>
    </DocsLayout>
  )
}
