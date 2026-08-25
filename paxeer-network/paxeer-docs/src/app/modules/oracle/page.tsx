import { DocsLayout } from '@/components/DocsLayout'
import Link from 'next/link'

export default function Oracle() {
  return (
    <DocsLayout>
      <div className="page-header">
        <h1 className="page-title">Oracle Module</h1>
        <p className="page-description">
          Validator exchange-rate voting and price aggregation.
        </p>
      </div>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/modules/oracle/</code>
      </div>

      <div className="prev-next">
        <Link href="/modules/mint">
          <div className="prev-next-label">Previous</div>
          <div className="prev-next-title">Mint Module</div>
        </Link>
        <Link href="/modules/store">
          <div className="prev-next-label">Next</div>
          <div className="prev-next-title">Store Module</div>
        </Link>
      </div>
    </DocsLayout>
  )
}
