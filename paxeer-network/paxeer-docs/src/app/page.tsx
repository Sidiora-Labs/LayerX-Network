import Link from 'next/link'

export default function Home() {
  return (
    <div className="min-h-screen bg-surface">
      <div className="relative overflow-hidden rounded-b-[28px] bg-gradient-to-b from-surface-lowest to-surface">
        <div className="absolute inset-0 bg-[radial-gradient(circle_at_82%_38%,rgb(40_65_184_/_0.33),transparent_27rem)]"></div>
        <div className="absolute inset-0 bg-[linear-gradient(rgb(226_232_255_/_0.035)_1px,transparent_1px),linear-gradient(90deg,rgb(226_232_255_/_0.035)_1px,transparent_1px)] bg-[size:64px_64px] [mask-image:linear-gradient(to_bottom,black,transparent_88%)]"></div>
        
        <header className="relative z-10 max-w-[1200px] mx-auto px-6 py-8 flex items-center justify-between">
          <div>
            <div className="text-lg font-medium">Paxeer Network</div>
            <div className="text-xs text-on-surface-variant font-mono uppercase tracking-wider">Chain ID 125 Technical Docs</div>
          </div>
        </header>

        <div className="relative z-10 max-w-[1200px] mx-auto px-6 py-20">
          <p className="font-mono text-xs text-ink-text uppercase tracking-[0.14em] mb-6">EVM L1 · Cosmos SDK Fork · Chain ID 125</p>
          <h1 className="text-6xl font-light leading-[0.98] tracking-[-0.04em] mb-7">
            Paxeer is where LayerX <span className="block text-ink-text font-normal">checkpoints, custody, and exits</span> live.
          </h1>
          <p className="text-lg text-on-surface-variant leading-relaxed max-w-[610px] mb-8">
            EVM chain ID 125, Cosmos identifier hyperpax_125-1. Runs <code className="text-sm">paxd</code> with PAX gas. LayerX activities charge 5,000 µUSDX (~½¢) base fee, never zero. Limited beta opens September 7, 2026.
          </p>
          <div className="flex flex-wrap gap-3">
            <Link href="/installation" className="inline-flex items-center justify-center gap-2 min-h-12 px-6 bg-primary text-on-primary rounded-full font-medium text-sm shadow-1 hover:bg-primary/90 transition-all duration-150">
              Install & Build
              <svg className="w-4 h-4" viewBox="0 0 24 24" fill="none"><path d="M7 17 17 7M8 7h9v9" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round"/></svg>
            </Link>
            <Link href="/run-node" className="inline-flex items-center justify-center gap-2 min-h-12 px-6 bg-secondary-container text-on-secondary-container rounded-full font-medium text-sm hover:bg-secondary-container/90 transition-all duration-150">
              Run a Node
            </Link>
            <Link href="/json-rpc" className="inline-flex items-center justify-center gap-2 min-h-12 px-6 bg-secondary-container text-on-secondary-container rounded-full font-medium text-sm hover:bg-secondary-container/90 transition-all duration-150">
              JSON-RPC
            </Link>
          </div>
        </div>
      </div>

      <div className="max-w-[1200px] mx-auto px-6 py-16">
        <div className="grid grid-cols-1 gap-3 mb-16 sm:grid-cols-2 lg:grid-cols-4">
          <div className="bg-surface-low rounded-lg p-6">
            <div className="text-xs text-on-surface-variant mb-3 tracking-wide">Chain ID</div>
            <div className="font-mono text-base font-medium">125</div>
          </div>
          <div className="bg-surface-low rounded-lg p-6">
            <div className="text-xs text-on-surface-variant mb-3 tracking-wide">Cosmos identifier</div>
            <div className="font-mono text-base font-medium">hyperpax_125-1</div>
          </div>
          <div className="bg-surface-low rounded-lg p-6">
            <div className="text-xs text-on-surface-variant mb-3 tracking-wide">Gas token</div>
            <div className="font-mono text-base font-medium">PAX</div>
          </div>
          <div className="bg-surface-low rounded-lg p-6">
            <div className="text-xs text-on-surface-variant mb-3 tracking-wide">LayerX base fee</div>
            <div className="font-mono text-base font-medium">5,000 µUSDX</div>
          </div>
        </div>

        <div className="mb-16">
          <h2 className="text-4xl font-normal tracking-[-0.02em] mb-4">What this chain does</h2>
          <div className="h-px bg-outline-variant mb-6"></div>
          <div className="text-on-surface-variant leading-relaxed space-y-4">
            <p>
              LayerX activities (receipts, 402LXP balances, agent/human interactions) run on LayerX. Periodic checkpoints, custody, guarantor bonds, disputes, withdrawals, and emergency exits settle on Paxeer. Custody never leaves an L1 that can be replayed independently of the LayerX sequencer.
            </p>
            <p>
              The settlement contracts live in <code>contracts/</code> at the monorepo root. They deploy <em>on</em> Paxeer. The node itself lives in <code>paxeer-network/</code>: <code>paxd</code> binary, EVM execution, JSON-RPC, chain modules, Docker compose, and HPX node distribution.
            </p>
          </div>
        </div>

        <div className="grid grid-cols-1 gap-4 mb-16 md:grid-cols-3">
          <div className="bg-surface-low rounded-lg p-6">
            <div className="w-12 h-12 rounded-md bg-surface-high text-ink-text flex items-center justify-center mb-6">
              <svg className="w-5 h-5" viewBox="0 0 24 24" fill="none"><path d="M5 5.5h14v13H5z" stroke="currentColor" strokeWidth="1.7"/><path d="m8 9 2 2-2 2M12.5 14H16" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round"/></svg>
            </div>
            <h3 className="text-xl font-medium mb-2">Node Binary</h3>
            <p className="text-sm text-on-surface-variant leading-relaxed">
              <code>paxd</code> runs Tendermint consensus, EVM state machine, and Cosmos SDK modules for epoch, mint, oracle, tokenfactory, and store.
            </p>
          </div>
          <div className="bg-surface-low rounded-lg p-6">
            <div className="w-12 h-12 rounded-md bg-surface-high text-ink-text flex items-center justify-center mb-6">
              <svg className="w-5 h-5" viewBox="0 0 24 24" fill="none"><circle cx="6" cy="12" r="2.5" stroke="currentColor" strokeWidth="1.7"/><circle cx="18" cy="7" r="2.5" stroke="currentColor" strokeWidth="1.7"/><circle cx="18" cy="17" r="2.5" stroke="currentColor" strokeWidth="1.7"/><path d="m8.3 11 7.3-3M8.3 13l7.3 3" stroke="currentColor" strokeWidth="1.7"/></svg>
            </div>
            <h3 className="text-xl font-medium mb-2">HPX Distribution</h3>
            <p className="text-sm text-on-surface-variant leading-relaxed">
              Checksum-verifying node manager, peer discovery, and state-sync bootstrap at <code>node.hyperpaxeer.com</code>.
            </p>
          </div>
          <div className="bg-surface-low rounded-lg p-6">
            <div className="w-12 h-12 rounded-md bg-surface-high text-ink-text flex items-center justify-center mb-6">
              <svg className="w-5 h-5" viewBox="0 0 24 24" fill="none"><path d="M7 17 17 7M8 7h9v9" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round"/></svg>
            </div>
            <h3 className="text-xl font-medium mb-2">EVM JSON-RPC</h3>
            <p className="text-sm text-on-surface-variant leading-relaxed">
              Standard Ethereum JSON-RPC endpoints for Web3 providers. Some methods unsupported where Cosmos semantics differ.
            </p>
          </div>
        </div>

        <div className="bg-surface-high rounded-lg p-6 mb-16">
          <div className="flex items-start justify-between gap-4">
            <div>
              <div className="text-xs text-on-surface-variant mb-2 tracking-wide">Repository layout</div>
              <code className="text-sm font-mono">paxeer-network/</code>
            </div>
          </div>
          <div className="mt-4 space-y-2 text-sm">
            <div className="flex gap-4"><code className="font-mono text-ink-text w-40">daemon/paxd/</code><span className="text-on-surface-variant">paxd node binary</span></div>
            <div className="flex gap-4"><code className="font-mono text-ink-text w-40">modules/</code><span className="text-on-surface-variant">epoch, mint, oracle, tokenfactory, store</span></div>
            <div className="flex gap-4"><code className="font-mono text-ink-text w-40">rpc/</code><span className="text-on-surface-variant">EVM JSON-RPC compatibility</span></div>
            <div className="flex gap-4"><code className="font-mono text-ink-text w-40">hpx/</code><span className="text-on-surface-variant">Node distribution and peer registry</span></div>
            <div className="flex gap-4"><code className="font-mono text-ink-text w-40">docker/</code><span className="text-on-surface-variant">Local single-node and cluster compose</span></div>
          </div>
        </div>

        <div>
          <h2 className="text-3xl font-normal tracking-[-0.02em] mb-6">Start here</h2>
          <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
            <Link href="/installation" className="group bg-surface-high rounded-lg p-5 border border-outline-variant hover:border-ink-text transition-all duration-150 hover:translate-y-[-2px]">
              <div className="text-xs text-on-surface-variant uppercase tracking-wider mb-2">Getting Started</div>
              <div className="text-lg font-medium">Installation & Build →</div>
            </Link>
            <Link href="/consensus" className="group bg-surface-high rounded-lg p-5 border border-outline-variant hover:border-ink-text transition-all duration-150 hover:translate-y-[-2px]">
              <div className="text-xs text-on-surface-variant uppercase tracking-wider mb-2">Architecture</div>
              <div className="text-lg font-medium">Consensus →</div>
            </Link>
            <Link href="/json-rpc" className="group bg-surface-high rounded-lg p-5 border border-outline-variant hover:border-ink-text transition-all duration-150 hover:translate-y-[-2px]">
              <div className="text-xs text-on-surface-variant uppercase tracking-wider mb-2">APIs</div>
              <div className="text-lg font-medium">JSON-RPC →</div>
            </Link>
            <Link href="/admin-hpx" className="group bg-surface-high rounded-lg p-5 border border-outline-variant hover:border-ink-text transition-all duration-150 hover:translate-y-[-2px]">
              <div className="text-xs text-on-surface-variant uppercase tracking-wider mb-2">Advanced</div>
              <div className="text-lg font-medium">Admin & HPX →</div>
            </Link>
          </div>
        </div>
      </div>
    </div>
  )
}
