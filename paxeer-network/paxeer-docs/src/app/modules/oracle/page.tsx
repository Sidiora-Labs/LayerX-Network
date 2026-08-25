import { DocsLayout } from '@/components/DocsLayout'
import Link from 'next/link'

export default function Oracle() {
  return (
    <DocsLayout>
      <div className="page-header">
        <h1 className="page-title">Oracle Module</h1>
        <p className="page-description">
          Validator-based exchange rate voting, weighted median price aggregation, and slashing for bad data.
        </p>
      </div>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/modules/oracle/</code>
      </div>

      <h2>Overview</h2>

      <p>
        The oracle module provides on-chain price feeds for asset exchange rates. Validators submit price observations during voting windows, and the module computes a weighted median to determine the canonical exchange rate for each asset.
      </p>

      <p>
        Validators must participate as oracles. Non-participation or submitting inaccurate data results in penalties and slashing.
      </p>

      <h2>Voting Procedure</h2>

      <p>
        The oracle operates in voting windows:
      </p>

      <ol>
        <li><strong>Vote Step:</strong> Validators submit their proposed exchange rates for the current window</li>
        <li><strong>Tally Step:</strong> At the end of the voting period, all votes are collected</li>
        <li><strong>Aggregation:</strong> A weighted median is computed using validator voting power</li>
        <li><strong>Finalization:</strong> The canonical exchange rate is stored on-chain</li>
      </ol>

      <p>
        Validators who fail to vote or whose votes deviate too far from the median have their miss count incremented.
      </p>

      <h2>Reward Band</h2>

      <p>
        The reward band defines acceptable deviation from the weighted median. Votes within the band are considered accurate; votes outside the band are penalized.
      </p>

      <p>
        Example: If the reward band is 2% and the median is $1.00, votes between $0.98 and $1.02 are rewarded. Votes outside this range increase the validator's miss count.
      </p>

      <h2>Slashing</h2>

      <p>
        Validators track a miss count:
      </p>

      <ul>
        <li><strong>Miss count increments:</strong> When a validator fails to vote or votes outside the reward band</li>
        <li><strong>Slash threshold:</strong> If a validator's miss count exceeds a threshold within a given number of voting periods, they are slashed</li>
        <li><strong>Penalty:</strong> Slashed stake is burned or redistributed, and the validator may be jailed</li>
      </ul>

      <p>
        This mechanism ensures validators provide accurate, timely price data.
      </p>

      <h2>Abstaining from Voting</h2>

      <p>
        Validators can abstain by not submitting a vote. Abstaining increments the miss count but is less severe than submitting bad data. Validators may abstain if they lack confidence in their price source or are experiencing downtime.
      </p>

      <h2>Use Cases</h2>

      <ul>
        <li><strong>PAX/USD price:</strong> For gas price estimation and fee calculations</li>
        <li><strong>USDL valuation:</strong> For LayerX settlement contracts</li>
        <li><strong>Cross-chain asset prices:</strong> For IBC transfers and interchain operations</li>
      </ul>

      <h2>State</h2>

      <p>
        The oracle module stores:
      </p>

      <ul>
        <li><strong>Exchange rates:</strong> Current canonical rate for each asset</li>
        <li><strong>Validator votes:</strong> Submitted votes for the current window</li>
        <li><strong>Miss counters:</strong> Miss count per validator</li>
      </ul>

      <h2>Messages</h2>

      <p>
        Validators submit price votes via the oracle module's message interface. Details of message types and parameters are defined in <code>modules/oracle/types/</code>.
      </p>

      <h2>Events</h2>

      <p>
        The oracle module emits events for:
      </p>

      <ul>
        <li>New voting windows</li>
        <li>Vote submissions</li>
        <li>Price aggregation results</li>
        <li>Slashing events</li>
      </ul>

      <h2>Hooks</h2>

      <p>
        Other modules can register hooks to react to new exchange rate data. For example, a derivatives module might update funding rates when oracle prices change.
      </p>

      <h2>Parameters</h2>

      <p>
        Oracle parameters (configurable via governance):
      </p>

      <ul>
        <li><strong>Vote period:</strong> Duration of each voting window</li>
        <li><strong>Reward band:</strong> Acceptable deviation from median (percentage)</li>
        <li><strong>Slash threshold:</strong> Number of misses before slashing</li>
        <li><strong>Slash window:</strong> Number of voting periods over which misses are counted</li>
        <li><strong>Min valid per window:</strong> Minimum validator participation rate</li>
      </ul>

      <h2>Queries</h2>

      <p>
        Query current exchange rates:
      </p>

      <pre><code>{`paxd q oracle exchange-rates`}</code></pre>

      <p>
        Query a specific asset rate:
      </p>

      <pre><code>{`paxd q oracle exchange-rate [denom]`}</code></pre>

      <p>
        Query validator miss counts:
      </p>

      <pre><code>{`paxd q oracle miss-counter [validator]`}</code></pre>

      <h2>Integration with Precompiles</h2>

      <p>
        The oracle module is exposed to EVM contracts via the <Link href="/precompiles">oracle precompile</Link>. Contracts can query exchange rates on-chain without relying on off-chain oracles.
      </p>

      <h2>Next Steps</h2>

      <ul>
        <li><Link href="/operators">Run a validator and submit oracle votes</Link></li>
        <li><Link href="/precompiles">Use the oracle precompile from EVM contracts</Link></li>
      </ul>

      <div className="prev-next">
        <Link href="/modules/mint">
          <div className="prev-next-label">Previous</div>
          <div className="prev-next-title">Mint Module</div>
        </Link>
        <Link href="/modules/store">
          <div className="prev-next-label">Next</div>
          <div className="prev-next-title">Store Module</div>
        </Link>
      </div>
    </DocsLayout>
  )
}
