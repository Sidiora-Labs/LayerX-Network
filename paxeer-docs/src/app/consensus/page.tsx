import { DocsLayout } from '@/components/DocsLayout'
import Link from 'next/link'

export default function Consensus() {
  return (
    <DocsLayout>
      <div className="page-header">
        <h1 className="page-title">Consensus</h1>
        <p className="page-description">
          Paxeer's BFT consensus based on Tendermint.
        </p>
      </div>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/consensus/</code>
      </div>

      <h2>Overview</h2>

      <p>
        Paxeer uses Byzantine Fault Tolerant (BFT) consensus based on Tendermint. This provides instant finality with no uncle blocks or reorgs.
      </p>

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
