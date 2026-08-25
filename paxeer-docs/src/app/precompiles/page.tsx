import { DocsLayout } from '@/components/DocsLayout'
import Link from 'next/link'

export default function Precompiles() {
  return (
    <DocsLayout>
      <div className="page-header">
        <h1 className="page-title">Precompiles</h1>
        <p className="page-description">
          Paxeer-specific precompiled contracts.
        </p>
      </div>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/precompiles/</code>
      </div>

      <div className="prev-next">
        <Link href="/modules/tokenfactory">
          <div className="prev-next-label">Previous</div>
          <div className="prev-next-title">Token Factory Module</div>
        </Link>
        <Link href="/wasm">
          <div className="prev-next-label">Next</div>
          <div className="prev-next-title">WASM</div>
        </Link>
      </div>
    </DocsLayout>
  )
}
