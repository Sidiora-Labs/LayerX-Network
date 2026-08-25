import { DocsLayout } from '@/components/DocsLayout'
import Link from 'next/link'

export default function WasmBinding() {
  return (
    <DocsLayout>
      <div className="page-header">
        <h1 className="page-title">WASM Bindings</h1>
        <p className="page-description">
          Custom Paxeer message types and queries for CosmWasm contracts.
        </p>
      </div>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/wasmbinding/</code>
      </div>

      <h2>Overview</h2>

      <p>
        The wasmbinding package provides Paxeer-specific extensions to the standard CosmWasm message and query interface. It allows CosmWasm contracts to interact with Paxeer modules (oracle, tokenfactory, etc.) that are not part of the base CosmWasm SDK.
      </p>

      <h2>Custom Messages</h2>

      <p>
        Standard CosmWasm supports generic messages like <code>BankMsg</code>, <code>StakingMsg</code>, and <code>WasmMsg</code>. Paxeer extends this with custom message types for chain-specific operations.
      </p>

      <p>
        Custom messages are not yet defined in the current wasmbinding implementation. Future extensions may include:
      </p>

      <ul>
        <li><strong>OracleMsg:</strong> Submit oracle votes (validator-only)</li>
        <li><strong>TokenFactoryMsg:</strong> Create, mint, burn tokenfactory denoms</li>
        <li><strong>EVMMsg:</strong> Interact with EVM contracts from CosmWasm</li>
      </ul>

      <h2>Custom Queries</h2>

      <p>
        wasmbinding currently provides first-class support for:
      </p>

      <h3>OracleExchangeRates</h3>

      <p>
        Query canonical exchange rates from the <Link href="/modules/oracle">oracle module</Link>:
      </p>

      <pre><code>{`// From Rust contract
let query_msg = QueryMsg::OracleExchangeRates { denom: "uusd".to_string() };
let rate: ExchangeRateResponse = deps.querier.query(&query_msg)?;`}</code></pre>

      <p>
        This allows CosmWasm contracts to access on-chain price feeds without relying on external oracles.
      </p>

      <h2>Integration</h2>

      <p>
        Contracts that want to use Paxeer-specific bindings must include the wasmbinding types in their Cargo dependencies:
      </p>

      <pre><code>{`[dependencies]
paxeer-bindings = { git = "https://github.com/Sidiora-Labs/LayerX-Protocol", branch = "main", subdir = "paxeer-network/wasmbinding" }`}</code></pre>

      <h2>Message Handling</h2>

      <p>
        Custom messages are routed to Paxeer modules via the CosmWasm <code>CustomMsg</code> handler. The node's <code>app.go</code> registers a message handler that decodes custom messages and dispatches them to the appropriate module keeper.
      </p>

      <h2>Query Handling</h2>

      <p>
        Custom queries are handled by the <code>CustomQuerier</code> interface. The wasmbinding package implements this interface and routes queries to Paxeer modules.
      </p>

      <h2>Limitations</h2>

      <p>
        The current wasmbinding implementation is minimal. It supports oracle queries but does not yet expose execution messages for most Paxeer modules.
      </p>

      <p>
        Contracts that need to interact with Paxeer modules beyond oracle queries should:
      </p>

      <ul>
        <li>Use standard <code>BankMsg</code>, <code>StakingMsg</code>, etc. where applicable</li>
        <li>Call Paxeer modules indirectly via the bank, staking, and distribution modules</li>
        <li>Wait for future wasmbinding extensions</li>
      </ul>

      <h2>Comparison with EVM Precompiles</h2>

      <p>
        wasmbinding provides similar functionality to <Link href="/precompiles">EVM precompiles</Link>, but for CosmWasm:
      </p>

      <table>
        <thead>
          <tr>
            <th>Feature</th>
            <th>EVM Precompiles</th>
            <th>CosmWasm Bindings</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td>Module access</td>
            <td>Fixed addresses</td>
            <td>Message types</td>
          </tr>
          <tr>
            <td>Queries</td>
            <td>Call precompile</td>
            <td>Query interface</td>
          </tr>
          <tr>
            <td>Execution</td>
            <td>Call precompile</td>
            <td>Dispatch custom message</td>
          </tr>
          <tr>
            <td>Type safety</td>
            <td>Solidity ABI</td>
            <td>Rust types</td>
          </tr>
        </tbody>
      </table>

      <h2>Example: Query Oracle</h2>

      <pre><code>{`use cosmwasm_std::{Deps, StdResult};
use paxeer_bindings::{PaxeerQuery, OracleExchangeRatesQuery, ExchangeRateResponse};

pub fn query_pax_usd_rate(deps: Deps) -> StdResult<ExchangeRateResponse> {
    let query = PaxeerQuery::OracleExchangeRates {
        query: OracleExchangeRatesQuery {
            denom: "upax".to_string(),
        },
    };
    deps.querier.query(&query.into())
}`}</code></pre>

      <h2>Next Steps</h2>

      <ul>
        <li><Link href="/wasm">Deploy CosmWasm contracts</Link></li>
        <li><Link href="/modules/oracle">Understand the oracle module</Link></li>
        <li><Link href="/precompiles">Compare with EVM precompiles</Link></li>
      </ul>

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
