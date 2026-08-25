import { DocsLayout } from '@/components/DocsLayout'
import Link from 'next/link'

export default function Storage() {
  return (
    <DocsLayout>
      <div className="page-header">
        <h1 className="page-title">Storage</h1>
        <p className="page-description">
          Paxeer's storage engines and state management.
        </p>
      </div>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/storage/</code>
      </div>

      <div className="prev-next">
        <Link href="/evm">
          <div className="prev-next-label">Previous</div>
          <div className="prev-next-title">EVM</div>
        </Link>
        <Link href="/modules">
          <div className="prev-next-label">Next</div>
          <div className="prev-next-title">Modules Overview</div>
        </Link>
      </div>
    </DocsLayout>
  )
}
