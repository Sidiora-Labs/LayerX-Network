import { DocsLayout } from '@/components/DocsLayout'
import Link from 'next/link'

export default function Mint() {
  return (
    <DocsLayout>
      <div className="page-header">
        <h1 className="page-title">Mint Module</h1>
        <p className="page-description">
          Scheduled native token inflation, daily distribution, and governance-controlled minting policy.
        </p>
      </div>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/modules/mint/</code>
      </div>

      <h2>Overview</h2>

      <p>
        The mint module creates new PAX tokens according to a predefined schedule. It enables scheduled token release structures that distribute tokens over time. The goal is to reach a deflationary state where no more inflation occurs and the network relies solely on transaction fees.
      </p>

      <p>
        Minting occurs daily (UTC), incentivizing users to stake tokens for longer durations to earn rewards.
      </p>

      <h2>Minting Mechanism</h2>

      <p>
        The mint module operates on a daily distribution model:
      </p>

      <ol>
        <li>A <code>total_mint_amount</code> is defined for a period between <code>start_date</code> and <code>end_date</code></li>
        <li>Each day, the module calculates <code>daily_mint_amount = remaining_mint_amount / days_remaining</code></li>
        <li>Tokens are minted and sent to the <code>fee_collector</code> account</li>
        <li>The distribution module distributes collected fees to stakers</li>
      </ol>

      <h3>Example</h3>

      <p>
        If <code>total_mint_amount = 1,000,000</code> tokens and the period is 100 days:
      </p>

      <ul>
        <li>Initial daily mint: <code>1,000,000 / 100 = 10,000</code> tokens/day</li>
        <li>After 50 days: <code>500,000 remaining / 50 days = 10,000</code> tokens/day (unchanged if no downtime)</li>
        <li>If the chain was down for 1 day at day 50: <code>500,000 remaining / 49 days = 10,204</code> tokens/day (adjusted)</li>
      </ul>

      <p>
        Network outages automatically adjust the daily rate to meet the total mint amount by the end date.
      </p>

      <h2>State</h2>

      <h3>Minter</h3>

      <p>
        The minter stores current inflation state:
      </p>

      <pre><code>{`type Minter struct {
    StartDate           string  // The day where the mint begins
    EndDate             string  // The day where the mint ends
    Denom               string  // Denom for the coins minted (default uhpx)
    TotalMintAmount     uint64  // Total amount to be minted
    RemainingMintAmount uint64  // Remaining amount to be minted
    LastMintAmount      uint64  // Last amount minted (usually from the day before)
    LastMintDate        string  // Last day minted
    LastMintHeight      uint64  // The height of the last mint
}`}</code></pre>

      <p>
        Query the current minter:
      </p>

      <pre><code>{`paxd q mint minter`}</code></pre>

      <h3>Params</h3>

      <p>
        Parameters define the minting schedule:
      </p>

      <pre><code>{`type Params struct {
    MintDenom            string                   // Type of coin to mint
    TokenReleaseSchedule []ScheduledTokenRelease  // List of token release schedules
}

type ScheduledTokenRelease struct {
    StartDate          string  // The day where the mint begins
    EndDate            string  // The day where the mint ends
    TokenReleaseAmount uint64  // Total amount to be minted
}`}</code></pre>

      <p>
        Multiple release schedules can be defined, allowing sequential minting periods.
      </p>

      <h2>Begin-Block</h2>

      <p>
        At the end of each epoch (default 60s), the mint module:
      </p>

      <ol>
        <li>Checks if it's the minting start date</li>
        <li>Calculates the daily mint amount</li>
        <li>Mints tokens to the <code>fee_collector</code> account</li>
        <li>Updates <code>LastMintAmount</code>, <code>LastMintDate</code>, and <code>RemainingMintAmount</code></li>
      </ol>

      <p>
        Epoch hooks trigger the minting check. See <Link href="/modules/epoch">Epoch module</Link> for how epochs work.
      </p>

      <h2>Governance</h2>

      <h3>Update Minter Proposal</h3>

      <p>
        The minter can be updated via governance to change the mint schedule:
      </p>

      <pre><code>{`{
  "title": "Update Minter",
  "description": "Adjust mint schedule",
  "minter": {
    "start_date": "2023-10-05",
    "end_date": "2023-11-22",
    "denom": "uhpx",
    "total_mint_amount": 100000
  }
}`}</code></pre>

      <p>
        Submit the proposal:
      </p>

      <pre><code>{`paxd tx gov submit-proposal update-minter ./minter_prop.json \\
  --deposit 20pax --from admin -b block -y \\
  --gas 200000 --fees 2000uhpx`}</code></pre>

      <p>
        Before the proposal:
      </p>

      <pre><code>{`> paxd q mint minter
denom: uhpx
end_date: "2023-04-30"
last_mint_amount: "333333333333"
last_mint_date: "2023-04-27"
last_mint_height: "0"
remaining_mint_amount: "666666666666"
start_date: "2023-04-27"
total_mint_amount: "999999999999"`}</code></pre>

      <p>
        After the proposal passes:
      </p>

      <pre><code>{`> paxd q mint minter
denom: uhpx
end_date: "2023-11-22"
last_mint_amount: "0"
last_mint_date: ""
last_mint_height: "0"
remaining_mint_amount: "0"
start_date: "2023-10-05"
total_mint_amount: "100000"`}</code></pre>

      <h3>Update Params Proposal</h3>

      <p>
        Parameters can be updated to define new token release schedules:
      </p>

      <pre><code>{`{
  "title": "Param Change Proposal",
  "description": "Update token release schedule",
  "changes": [
    {
      "subspace": "mint",
      "key": "MintDenom",
      "value": "uhpx"
    },
    {
      "subspace": "mint",
      "key": "TokenReleaseSchedule",
      "value": [
        {
          "token_release_amount": 500,
          "start_date": "2023-10-01",
          "end_date": "2023-10-30"
        },
        {
          "token_release_amount": 1000,
          "start_date": "2023-11-01",
          "end_date": "2023-11-30"
        }
      ]
    }
  ]
}`}</code></pre>

      <p>
        Submit:
      </p>

      <pre><code>{`paxd tx gov submit-proposal param-change ./param_change_prop.json \\
  --from admin -b block -y --gas 200000 --fees 200000uhpx`}</code></pre>

      <div className="source-note">
        <strong>Note:</strong> Changes to <code>total_mint_amount</code> or <code>remaining_mint_amount</code> after the start date do not affect tokens already minted.
      </div>

      <h2>Events</h2>

      <h3>Mint Event</h3>

      <p>
        Emitted on each successful mint:
      </p>

      <ul>
        <li><strong>mint_date:</strong> Date of the mint</li>
        <li><strong>mint_epoch:</strong> Epoch number when the mint occurred</li>
        <li><strong>amount:</strong> Amount minted</li>
      </ul>

      <h2>Metrics</h2>

      <p>
        The mint module emits a <code>pax_mint_coins&#123;denom&#125;</code> Prometheus metric on each successful mint event.
      </p>

      <h2>Next Steps</h2>

      <ul>
        <li><Link href="/modules/epoch">Understand epoch coordination</Link></li>
        <li><Link href="/modules/oracle">Learn about the oracle module</Link></li>
      </ul>

      <div className="prev-next">
        <Link href="/modules/epoch">
          <div className="prev-next-label">Previous</div>
          <div className="prev-next-title">Epoch Module</div>
        </Link>
        <Link href="/modules/oracle">
          <div className="prev-next-label">Next</div>
          <div className="prev-next-title">Oracle Module</div>
        </Link>
      </div>
    </DocsLayout>
  )
}
