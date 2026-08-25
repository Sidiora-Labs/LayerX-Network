import { DocsLayout } from '@/components/DocsLayout'
import Link from 'next/link'

export default function Epoch() {
  return (
    <DocsLayout>
      <div className="page-header">
        <h1 className="page-title">Epoch Module</h1>
        <p className="page-description">
          Time-based hooks and epoch lifecycle management.
        </p>
      </div>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/modules/epoch/</code>
      </div>

      <div className="prev-next">
        <Link href="/modules">
          <div className="prev-next-label">Previous</div>
          <div className="prev-next-title">Modules Overview</div>
        </Link>
        <Link href="/modules/mint">
          <div className="prev-next-label">Next</div>
          <div className="prev-next-title">Mint Module</div>
        </Link>
      </div>
    </DocsLayout>
  )
}
