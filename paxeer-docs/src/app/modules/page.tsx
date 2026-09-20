import { DocsLayout } from '@/components/DocsLayout'
import Link from 'next/link'

export default function Modules() {
  return (
    <DocsLayout>
      <div className="page-header">
        <h1 className="page-title">Paxeer Modules</h1>
        <p className="page-description">
          Paxeer-specific chain modules that extend the Cosmos SDK base.
        </p>
      </div>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/modules/README.md</code>
      </div>

      <h2>Module Overview</h2>

      <p>
        Paxeer-specific chain modules live under <code>paxeer-network/modules/</code>. These extend the Cosmos SDK with functionality unique to Paxeer's EVM L1 design.
      </p>

      <h2>Module List</h2>

      <table>
        <thead>
          <tr>
            <th>Module</th>
            <th>Purpose</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td><Link href="/evm"><code>evm</code></Link></td>
            <td>Native EVM execution, address association, receipts, pointers, and precompile integration</td>
          </tr>
          <tr>
            <td><Link href="/modules/epoch"><code>epoch</code></Link></td>
            <td>Time-based hooks and epoch lifecycle management</td>
          </tr>
          <tr>
            <td><Link href="/modules/mint"><code>mint</code></Link></td>
            <td>Inflation and native-token minting policy</td>
          </tr>
          <tr>
            <td><Link href="/modules/oracle"><code>oracle</code></Link></td>
            <td>Validator exchange-rate voting and price aggregation</td>
          </tr>
          <tr>
            <td><Link href="/modules/store"><code>store</code></Link></td>
            <td>Module-level store integration helpers</td>
          </tr>
          <tr>
            <td><Link href="/modules/tokenfactory"><code>tokenfactory</code></Link></td>
            <td>Permissioned creation and management of native token denominations</td>
          </tr>
        </tbody>
      </table>

      <h2>Framework Modules</h2>

      <p>
        Framework-provided modules (bank, staking, distribution, governance, etc.) remain under <code>sdk/x/</code>. Interchain application modules live under <code>interchain/modules/</code>.
      </p>

      <h2>EVM Module</h2>

      <p>
        The <code>evm</code> module is Paxeer's core EVM execution layer:
      </p>

      <ul>
        <li><strong>EVM execution:</strong> Runs Ethereum transactions with full opcode compatibility</li>
        <li><strong>Address association:</strong> Maps Cosmos addresses to EVM addresses</li>
        <li><strong>Receipts:</strong> Generates EVM transaction receipts</li>
        <li><strong>Pointers:</strong> Bridging between Cosmos and EVM state</li>
        <li><strong>Precompiles:</strong> Integration with Paxeer-specific precompiled contracts</li>
      </ul>

      <p>
        See <Link href="/evm">EVM documentation</Link> for details.
      </p>

      <h2>Epoch Module</h2>

      <p>
        The <code>epoch</code> module provides time-based lifecycle hooks:
      </p>

      <ul>
        <li><strong>Epoch boundaries:</strong> Define periods for validator set changes, parameter updates</li>
        <li><strong>Hooks:</strong> Other modules register callbacks to run at epoch boundaries</li>
        <li><strong>Time coordination:</strong> Synchronize periodic tasks across the chain</li>
      </ul>

      <p>
        See <Link href="/modules/epoch">Epoch documentation</Link> for details.
      </p>

      <h2>Mint Module</h2>

      <p>
        The <code>mint</code> module manages native token issuance:
      </p>

      <ul>
        <li><strong>Inflation policy:</strong> Configurable inflation rate for PAX</li>
        <li><strong>Minting schedule:</strong> Block-by-block or epoch-based minting</li>
        <li><strong>Distribution:</strong> Newly minted tokens distributed to validators and stakers</li>
      </ul>

      <p>
        See <Link href="/modules/mint">Mint documentation</Link> for details.
      </p>

      <h2>Oracle Module</h2>

      <p>
        The <code>oracle</code> module enables validator-based price feeds:
      </p>

      <ul>
        <li><strong>Exchange rate voting:</strong> Validators submit price observations</li>
        <li><strong>Aggregation:</strong> Median or weighted-average price computation</li>
        <li><strong>Slashing:</strong> Penalties for validators submitting bad data</li>
        <li><strong>Use cases:</strong> PAX/USD price for gas estimation, USDL valuation</li>
      </ul>

      <p>
        See <Link href="/modules/oracle">Oracle documentation</Link> for details.
      </p>

      <h2>Store Module</h2>

      <p>
        The <code>store</code> module provides store integration helpers:
      </p>

      <ul>
        <li><strong>Key formatting:</strong> Standardized key prefixes for module stores</li>
        <li><strong>Iterator utilities:</strong> Common patterns for range queries</li>
        <li><strong>Codec helpers:</strong> Marshaling and unmarshaling state</li>
      </ul>

      <p>
        See <Link href="/modules/store">Store documentation</Link> for details.
      </p>

      <h2>Token Factory Module</h2>

      <p>
        The <code>tokenfactory</code> module enables permissioned token creation:
      </p>

      <ul>
        <li><strong>Native denominations:</strong> Create new token types on Paxeer</li>
        <li><strong>Permissioned:</strong> Controlled by module parameters or governance</li>
        <li><strong>Management:</strong> Mint, burn, transfer hooks</li>
        <li><strong>Use cases:</strong> Wrapped assets, synthetic tokens, protocol-managed denominations</li>
      </ul>

      <p>
        See <Link href="/modules/tokenfactory">Token Factory documentation</Link> for details.
      </p>

      <h2>Module Integration</h2>

      <p>
        Modules are registered in the application through <code>node/app.go</code>:
      </p>

      <ul>
        <li><strong>Module manager:</strong> Coordinates module initialization and upgrades</li>
        <li><strong>Begin/end block:</strong> Modules register hooks for block lifecycle</li>
        <li><strong>Message routing:</strong> Module-specific transactions routed to handlers</li>
        <li><strong>Query routing:</strong> Module-specific queries routed to keepers</li>
      </ul>

      <div className="prev-next">
        <Link href="/storage">
          <div className="prev-next-label">Previous</div>
          <div className="prev-next-title">Storage</div>
        </Link>
        <Link href="/modules/epoch">
          <div className="prev-next-label">Next</div>
          <div className="prev-next-title">Epoch Module</div>
        </Link>
      </div>
    </DocsLayout>
  )
}
