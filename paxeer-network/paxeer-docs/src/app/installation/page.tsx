import { DocsLayout } from '@/components/DocsLayout'
import Link from 'next/link'

export default function Installation() {
  return (
    <DocsLayout pageTitle="Installation & Build">
      <p className="text-on-surface-variant mb-6">
        How to build and install the paxd node binary.
      </p>

      <div className="bg-surface-high border border-outline-variant rounded-lg px-4 py-3 mb-6">
        <strong className="text-on-surface">Source:</strong> <code>paxeer-network/Makefile</code> and <code>paxeer-network/README.md</code>
      </div>

      <h2>Prerequisites</h2>

      <ul>
        <li><strong>Go:</strong> 1.23 or higher</li>
        <li><strong>Make:</strong> Standard build tool</li>
        <li><strong>Git:</strong> For cloning the repository</li>
        <li><strong>GCC:</strong> Required for ledger support (optional, can be disabled with <code>LEDGER_ENABLED=false</code>)</li>
      </ul>

      <h2>Repository Location</h2>

      <p>
        Paxeer Network lives in the <a href="https://github.com/Sidiora-Labs/LayerX-Protocol">Sidiora-Labs/LayerX-Protocol</a> monorepo under <code>paxeer-network/</code>.
      </p>

      <pre><code>{`git clone https://github.com/Sidiora-Labs/LayerX-Protocol.git
cd LayerX-Protocol/paxeer-network`}</code></pre>

      <h2>Build from Monorepo Root</h2>

      <p>
        From the <strong>monorepo root</strong>, use these namespaced targets:
      </p>

      <pre><code>{`make paxeer-build    # Build paxd binary to paxeer-network/build/paxd
make paxeer-lint     # Run linters
make paxeer-test     # Run tests
make paxeer-ci       # Lint + test (CI gate)`}</code></pre>

      <p>
        <code>make monorepo-ci</code> at the repository root composes the LayerX gate with <code>make paxeer-ci</code>.
      </p>

      <h2>Build from paxeer-network/ Directory</h2>

      <p>
        From <strong>within <code>paxeer-network/</code></strong>:
      </p>

      <pre><code>{`make build    # Build to ./build/paxd
make install  # Install to \$GOPATH/bin (go install ./daemon/paxd)
make lint     # Run linters
make test     # Run tests
make ci       # Lint + test`}</code></pre>

      <h3>Build Output</h3>

      <p>
        The <code>paxd</code> binary is written to:
      </p>

      <ul>
        <li><code>./build/paxd</code> when using <code>make build</code></li>
        <li><code>$GOPATH/bin/paxd</code> when using <code>make install</code></li>
      </ul>

      <h2>Build Tags and Flags</h2>

      <h3>Ledger Support</h3>

      <p>
        Ledger hardware wallet support is enabled by default. To build without ledger:
      </p>

      <pre><code>{`make build LEDGER_ENABLED=false`}</code></pre>

      <h3>Mock Balances (Testing)</h3>

      <p>
        For testing with mock balances:
      </p>

      <pre><code>{`make install-mock-balances`}</code></pre>

      <h3>Benchmark Build</h3>

      <p>
        For benchmark runs:
      </p>

      <pre><code>{`make install-bench`}</code></pre>

      <h2>Version Information</h2>

      <p>
        The Makefile resolves version from Git tags or branch names. Paxeer releases use namespaced tags:
      </p>

      <pre><code>{`paxeer-network/vX.Y.Z`}</code></pre>

      <p>
        Version information is embedded in the binary at build time via linker flags:
      </p>

      <ul>
        <li><code>Name</code>: paxeer</li>
        <li><code>AppName</code>: paxd</li>
        <li><code>Version</code>: Resolved from tag/branch</li>
        <li><code>Commit</code>: Git commit SHA</li>
      </ul>

      <p>
        Check version:
      </p>

      <pre><code>{`paxd version`}</code></pre>

      <h2>Static Linking</h2>

      <p>
        To build a statically-linked binary:
      </p>

      <pre><code>{`make build LINK_STATICALLY=true`}</code></pre>

      <h2>Go Module</h2>

      <p>
        Paxeer is a separate Go module:
      </p>

      <pre><code>{`module github.com/sidiora-labs/paxeer-network

go 1.23`}</code></pre>

      <div className="source-note">
        <strong>Note:</strong> Paxeer maintains its own <code>go.mod</code>, <code>go.sum</code>, and dependency tree separate from the LayerX monorepo root.
      </div>

      <h2>Foundry (Contracts)</h2>

      <p>
        Paxeer-native contracts use Foundry:
      </p>

      <pre><code>{`cd paxeer-network
forge install
forge build`}</code></pre>

      <p>
        See <code>paxeer-network/contracts/README.md</code> for contract-specific instructions.
      </p>

      <div className="source-note">
        <strong>Important:</strong> <code>paxeer-network/contracts/</code> contains Paxeer-native contracts (WPAX, pointers, precompile interfaces). LayerX settlement contracts are at <code>contracts/</code> in the repository root.
      </div>

      <h2>Docker</h2>

      <p>
        Local Docker cluster targets are available. See <Link href="/docker">Docker documentation</Link> for:
      </p>

      <ul>
        <li><code>make docker-cluster-start</code></li>
        <li><code>make run-local-node</code></li>
        <li>Compose configurations in <code>docker/</code></li>
      </ul>

      <h2>Development vs Production</h2>

      <div className="source-note">
        <span className="badge badge-warning">Development Only</span>
        <p style={{ marginTop: '0.5rem' }}>
          A successful local build is development evidence. It is <strong>not authorization</strong> to deploy validators, move custody, or handle real assets.
        </p>
      </div>

      <h2>Next Steps</h2>

      <ul>
        <li><Link href="/run-node">Run a node</Link></li>
        <li><Link href="/configuration">Configure paxd</Link></li>
        <li><Link href="/docker">Use Docker for local testing</Link></li>
      </ul>

      <div className="prev-next">
        <Link href="/network-parameters">
          <div className="prev-next-label">Previous</div>
          <div className="prev-next-title">Network Parameters</div>
        </Link>
        <Link href="/run-node">
          <div className="prev-next-label">Next</div>
          <div className="prev-next-title">Run a Node</div>
        </Link>
      </div>
    </DocsLayout>
  )
}
