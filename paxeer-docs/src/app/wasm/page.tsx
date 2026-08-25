import { DocsLayout } from '@/components/DocsLayout'
import Link from 'next/link'

export default function Wasm() {
  return (
    <DocsLayout>
      <div className="page-header">
        <h1 className="page-title">WASM</h1>
        <p className="page-description">
          WebAssembly smart contract support on Paxeer.
        </p>
      </div>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/wasm/</code>
      </div>

      <div className="prev-next">
        <Link href="/precompiles">
          <div className="prev-next-label">Previous</div>
          <div className="prev-next-title">Precompiles</div>
        </Link>
        <Link href="/wasm-runtime">
          <div className="prev-next-label">Next</div>
          <div className="prev-next-title">WASM Runtime</div>
        </Link>
      </div>
    </DocsLayout>
  )
}
