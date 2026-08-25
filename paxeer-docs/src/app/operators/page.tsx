import { DocsLayout } from '@/components/DocsLayout'
import Link from 'next/link'

export default function Operators() {
  return (
    <DocsLayout>
      <div className="page-header">
        <h1 className="page-title">Operators Guide</h1>
        <p className="page-description">
          Best practices for running Paxeer validators and full nodes.
        </p>
      </div>

      <div className="source-note">
        <strong>Documentation in progress.</strong> Validator onboarding opens with mainnet launch.
      </div>

      <div className="prev-next">
        <Link href="/configuration">
          <div className="prev-next-label">Previous</div>
          <div className="prev-next-title">Configuration</div>
        </Link>
        <Link href="/consensus">
          <div className="prev-next-label">Next</div>
          <div className="prev-next-title">Consensus</div>
        </Link>
      </div>
    </DocsLayout>
  )
}
