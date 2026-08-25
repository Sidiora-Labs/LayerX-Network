import { DocsLayout } from '@/components/DocsLayout'
import Link from 'next/link'

export default function Docker() {
  return (
    <DocsLayout>
      <div className="page-header">
        <h1 className="page-title">Docker</h1>
        <p className="page-description">
          Local Docker clusters for Paxeer development and testing.
        </p>
      </div>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/docker/</code> and <code>docker/README.md</code>
      </div>

      <div className="prev-next">
        <Link href="/contracts">
          <div className="prev-next-label">Previous</div>
          <div className="prev-next-title">Contracts</div>
        </Link>
        <Link href="/sdk">
          <div className="prev-next-label">Next</div>
          <div className="prev-next-title">SDK</div>
        </Link>
      </div>
    </DocsLayout>
  )
}
