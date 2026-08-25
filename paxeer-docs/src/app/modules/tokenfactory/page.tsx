import { DocsLayout } from '@/components/DocsLayout'
import Link from 'next/link'

export default function TokenFactory() {
  return (
    <DocsLayout>
      <div className="page-header">
        <h1 className="page-title">Token Factory Module</h1>
        <p className="page-description">
          Permissioned creation and management of native token denominations.
        </p>
      </div>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/modules/tokenfactory/</code>
      </div>

      <div className="prev-next">
        <Link href="/modules/store">
          <div className="prev-next-label">Previous</div>
          <div className="prev-next-title">Store Module</div>
        </Link>
        <Link href="/precompiles">
          <div className="prev-next-label">Next</div>
          <div className="prev-next-title">Precompiles</div>
        </Link>
      </div>
    </DocsLayout>
  )
}
