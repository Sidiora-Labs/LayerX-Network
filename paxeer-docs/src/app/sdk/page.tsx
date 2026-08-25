import { DocsLayout } from '@/components/DocsLayout'
import Link from 'next/link'

export default function SDK() {
  return (
    <DocsLayout>
      <div className="page-header">
        <h1 className="page-title">SDK</h1>
        <p className="page-description">
          Cosmos SDK fork and Paxeer-specific extensions.
        </p>
      </div>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/sdk/</code>
      </div>

      <div className="prev-next">
        <Link href="/docker">
          <div className="prev-next-label">Previous</div>
          <div className="prev-next-title">Docker</div>
        </Link>
        <Link href="/interchain">
          <div className="prev-next-label">Next</div>
          <div className="prev-next-title">Interchain</div>
        </Link>
      </div>
    </DocsLayout>
  )
}
