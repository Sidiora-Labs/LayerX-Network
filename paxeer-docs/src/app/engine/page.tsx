import { DocsLayout } from '@/components/DocsLayout'
import Link from 'next/link'

export default function Engine() {
  return (
    <DocsLayout>
      <div className="page-header">
        <h1 className="page-title">Engine</h1>
        <p className="page-description">
          Paxeer's execution engine and EVM integration.
        </p>
      </div>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/engine/</code>
      </div>

      <div className="prev-next">
        <Link href="/consensus">
          <div className="prev-next-label">Previous</div>
          <div className="prev-next-title">Consensus</div>
        </Link>
        <Link href="/evm">
          <div className="prev-next-label">Next</div>
          <div className="prev-next-title">EVM</div>
        </Link>
      </div>
    </DocsLayout>
  )
}
