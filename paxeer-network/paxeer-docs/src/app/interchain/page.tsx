import { DocsLayout } from '@/components/DocsLayout'
import Link from 'next/link'

export default function Interchain() {
  return (
    <DocsLayout>
      <div className="page-header">
        <h1 className="page-title">Interchain</h1>
        <p className="page-description">
          IBC and cross-chain communication on Paxeer.
        </p>
      </div>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/interchain/</code>
      </div>

      <div className="prev-next">
        <Link href="/sdk">
          <div className="prev-next-label">Previous</div>
          <div className="prev-next-title">SDK</div>
        </Link>
        <Link href="/admin-hpx">
          <div className="prev-next-label">Next</div>
          <div className="prev-next-title">Admin & HPX</div>
        </Link>
      </div>
    </DocsLayout>
  )
}
