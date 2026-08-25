import { DocsLayout } from '@/components/DocsLayout'
import Link from 'next/link'

export default function Configuration() {
  return (
    <DocsLayout>
      <div className="page-header">
        <h1 className="page-title">Configuration</h1>
        <p className="page-description">
          Configuring paxd through config.toml, app.toml, and environment variables.
        </p>
      </div>

      <div className="source-note">
        <strong>Documentation in progress.</strong> See <code>paxeer-network/node/</code> and generated config files.
      </div>

      <div className="prev-next">
        <Link href="/run-node">
          <div className="prev-next-label">Previous</div>
          <div className="prev-next-title">Run a Node</div>
        </Link>
        <Link href="/operators">
          <div className="prev-next-label">Next</div>
          <div className="prev-next-title">Operators Guide</div>
        </Link>
      </div>
    </DocsLayout>
  )
}
