import { DocsLayout } from '@/components/DocsLayout'
import Link from 'next/link'

export default function Store() {
  return (
    <DocsLayout>
      <div className="page-header">
        <h1 className="page-title">Store Module</h1>
        <p className="page-description">
          Module-level store integration helpers.
        </p>
      </div>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/modules/store/</code>
      </div>

      <div className="prev-next">
        <Link href="/modules/oracle">
          <div className="prev-next-label">Previous</div>
          <div className="prev-next-title">Oracle Module</div>
        </Link>
        <Link href="/modules/tokenfactory">
          <div className="prev-next-label">Next</div>
          <div className="prev-next-title">Token Factory Module</div>
        </Link>
      </div>
    </DocsLayout>
  )
}
