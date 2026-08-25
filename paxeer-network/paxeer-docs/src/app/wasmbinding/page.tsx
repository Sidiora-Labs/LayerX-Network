import { DocsLayout } from '@/components/DocsLayout'
import Link from 'next/link'

export default function WasmBinding() {
  return (
    <DocsLayout>
      <div className="page-header">
        <h1 className="page-title">WASM Bindings</h1>
        <p className="page-description">
          Bindings between WASM contracts and Paxeer modules.
        </p>
      </div>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/wasmbinding/</code>
      </div>

      <div className="prev-next">
        <Link href="/wasm-runtime">
          <div className="prev-next-label">Previous</div>
          <div className="prev-next-title">WASM Runtime</div>
        </Link>
        <Link href="/json-rpc">
          <div className="prev-next-label">Next</div>
          <div className="prev-next-title">JSON-RPC API</div>
        </Link>
      </div>
    </DocsLayout>
  )
}
