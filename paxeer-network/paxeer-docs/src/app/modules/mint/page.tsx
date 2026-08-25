import { DocsLayout } from '@/components/DocsLayout'
import Link from 'next/link'

export default function Mint() {
  return (
    <DocsLayout>
      <div className="page-header">
        <h1 className="page-title">Mint Module</h1>
        <p className="page-description">
          Inflation and native-token minting policy.
        </p>
      </div>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/modules/mint/</code>
      </div>

      <div className="prev-next">
        <Link href="/modules/epoch">
          <div className="prev-next-label">Previous</div>
          <div className="prev-next-title">Epoch Module</div>
        </Link>
        <Link href="/modules/oracle">
          <div className="prev-next-label">Next</div>
          <div className="prev-next-title">Oracle Module</div>
        </Link>
      </div>
    </DocsLayout>
  )
}
