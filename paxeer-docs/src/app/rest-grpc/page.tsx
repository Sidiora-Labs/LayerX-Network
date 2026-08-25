import { DocsLayout } from '@/components/DocsLayout'
import Link from 'next/link'

export default function RestGrpc() {
  return (
    <DocsLayout>
      <div className="page-header">
        <h1 className="page-title">REST & gRPC APIs</h1>
        <p className="page-description">
          Cosmos SDK REST and gRPC interfaces on Paxeer.
        </p>
      </div>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/api/</code> and proto definitions
      </div>

      <div className="prev-next">
        <Link href="/json-rpc-unsupported">
          <div className="prev-next-label">Previous</div>
          <div className="prev-next-title">Unsupported Methods</div>
        </Link>
        <Link href="/contracts">
          <div className="prev-next-label">Next</div>
          <div className="prev-next-title">Contracts</div>
        </Link>
      </div>
    </DocsLayout>
  )
}
