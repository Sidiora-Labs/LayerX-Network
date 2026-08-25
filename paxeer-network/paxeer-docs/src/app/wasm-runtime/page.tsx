import { DocsLayout } from '@/components/DocsLayout'
import Link from 'next/link'

export default function WasmRuntime() {
  return (
    <DocsLayout>
      <div className="page-header">
        <h1 className="page-title">WASM Runtime</h1>
        <p className="page-description">
          The WASM execution runtime and VM integration.
        </p>
      </div>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/wasm-runtime/</code>
      </div>

      <div className="prev-next">
        <Link href="/wasm">
          <div className="prev-next-label">Previous</div>
          <div className="prev-next-title">WASM</div>
        </Link>
        <Link href="/wasmbinding">
          <div className="prev-next-label">Next</div>
          <div className="prev-next-title">WASM Bindings</div>
        </Link>
      </div>
    </DocsLayout>
  )
}
