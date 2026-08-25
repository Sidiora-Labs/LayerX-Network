import { DocsLayout } from '@/components/DocsLayout'
import Link from 'next/link'

export default function EVM() {
  return (
    <DocsLayout>
      <div className="page-header">
        <h1 className="page-title">EVM Module</h1>
        <p className="page-description">
          Native EVM execution, address association, receipts, and precompile integration.
        </p>
      </div>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/modules/evm/</code>
      </div>

      <h2>Overview</h2>

      <p>
        The EVM module provides Ethereum Virtual Machine execution on Paxeer (chain ID 125). It handles EVM transaction processing, address mapping between Cosmos and EVM formats, receipt generation, and precompile integration.
      </p>

      <div className="prev-next">
        <Link href="/engine">
          <div className="prev-next-label">Previous</div>
          <div className="prev-next-title">Engine</div>
        </Link>
        <Link href="/storage">
          <div className="prev-next-label">Next</div>
          <div className="prev-next-title">Storage</div>
        </Link>
      </div>
    </DocsLayout>
  )
}
