import { DocsLayout } from '@/components/DocsLayout'
import Link from 'next/link'

export default function AdminHpx() {
  return (
    <DocsLayout>
      <div className="page-header">
        <h1 className="page-title">Admin & HPX</h1>
        <p className="page-description">
          Native paxd distribution and peer registry tooling.
        </p>
      </div>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/hpx/</code> and <code>paxeer-network/admin/</code>
      </div>

      <h2>Chain Identifier</h2>

      <p>
        The Cosmos-style chain identifier for node distribution is <code>hyperpax_125-1</code> (EVM chain ID 125).
      </p>

      <div className="prev-next">
        <Link href="/interchain">
          <div className="prev-next-label">Previous</div>
          <div className="prev-next-title">Interchain</div>
        </Link>
        <div></div>
      </div>
    </DocsLayout>
  )
}
