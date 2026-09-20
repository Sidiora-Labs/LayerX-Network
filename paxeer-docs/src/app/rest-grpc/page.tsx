import { DocsLayout } from '@/components/DocsLayout'
import Link from 'next/link'

export default function RestGrpc() {
  return (
    <DocsLayout>
      <div className="page-header">
        <h1 className="page-title">REST & gRPC APIs</h1>
        <p className="page-description">
          Cosmos SDK REST and gRPC interfaces for Paxeer chain modules and administration.
        </p>
      </div>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/api/</code> proto definitions, <code>paxeer-network/docs/</code>
      </div>

      <h2>Overview</h2>

      <p>
        Paxeer exposes Cosmos SDK-style gRPC and REST (gRPC-Gateway) endpoints for chain modules. These APIs complement the <Link href="/json-rpc">JSON-RPC interface</Link> and provide access to chain state, module parameters, and administrative operations.
      </p>

      <h3>Protocol Buffers</h3>

      <p>
        All services are defined in <code>paxeer-network/api/</code> using Protocol Buffers v3. The proto files generate:
      </p>

      <ul>
        <li>Go types under <code>github.com/sidiora-labs/paxeer-network/modules/*/types</code></li>
        <li>gRPC server and client interfaces</li>
        <li>REST endpoints via <code>google.api.http</code> annotations</li>
        <li>OpenAPI/Swagger documentation</li>
      </ul>

      <h3>Code Generation</h3>

      <p>
        Regenerate Go code from proto files:
      </p>

      <pre><code>{`ignite generate proto-go`}</code></pre>

      <p>
        Requires Ignite CLI v0.23.0. See <code>paxeer-network/api/README.md</code> for installation.
      </p>

      <h2>EVM Module Query Service</h2>

      <p>
        The EVM module provides address mapping, static calls, and pointer queries.
      </p>

      <h3>Address Mapping</h3>

      <table>
        <thead>
          <tr>
            <th>Method</th>
            <th>REST Endpoint</th>
            <th>Purpose</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td><code>PaxAddressByEVMAddress</code></td>
            <td><code>GET /pax-protocol/paxchain/evm/pax_address</code></td>
            <td>Get Cosmos address from EVM address</td>
          </tr>
          <tr>
            <td><code>EVMAddressByPaxAddress</code></td>
            <td><code>GET /pax-protocol/paxchain/evm/evm_address</code></td>
            <td>Get EVM address from Cosmos address</td>
          </tr>
        </tbody>
      </table>

      <h3>Static Calls</h3>

      <table>
        <thead>
          <tr>
            <th>Method</th>
            <th>REST Endpoint</th>
            <th>Purpose</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td><code>StaticCall</code></td>
            <td><code>GET /pax-protocol/paxchain/evm/static_call</code></td>
            <td>Execute read-only contract call</td>
          </tr>
        </tbody>
      </table>

      <h3>Pointer Queries</h3>

      <p>
        Pointers bridge Cosmos-native assets (CW20/CW721/CW1155) to EVM addresses:
      </p>

      <table>
        <thead>
          <tr>
            <th>Method</th>
            <th>REST Endpoint</th>
            <th>Purpose</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td><code>Pointer</code></td>
            <td><code>GET /pax-protocol/paxchain/evm/pointer</code></td>
            <td>Get EVM pointer for Cosmos contract</td>
          </tr>
          <tr>
            <td><code>Pointee</code></td>
            <td><code>GET /pax-protocol/paxchain/evm/pointee</code></td>
            <td>Get Cosmos contract from EVM pointer</td>
          </tr>
          <tr>
            <td><code>PointerVersion</code></td>
            <td><code>GET /pax-protocol/paxchain/evm/pointer_version</code></td>
            <td>Get current pointer contract version</td>
          </tr>
        </tbody>
      </table>

      <div className="source-note">
        <strong>Proto:</strong> <code>paxeer-network/api/evm/query.proto</code>
      </div>

      <h2>Epoch Module Query Service</h2>

      <p>
        The Epoch module tracks the current chain epoch for time-based operations:
      </p>

      <table>
        <thead>
          <tr>
            <th>Method</th>
            <th>REST Endpoint</th>
            <th>Purpose</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td><code>Epoch</code></td>
            <td><code>GET /pax-protocol/paxchain/epoch/epoch</code></td>
            <td>Get current epoch number and metadata</td>
          </tr>
          <tr>
            <td><code>Params</code></td>
            <td><code>GET /pax-protocol/paxchain/epoch/params</code></td>
            <td>Get epoch module parameters</td>
          </tr>
        </tbody>
      </table>

      <div className="source-note">
        <strong>Proto:</strong> <code>paxeer-network/api/epoch/query.proto</code>
      </div>

      <h2>Oracle Module Query Service</h2>

      <p>
        The Oracle module provides exchange rates, price feeds, and validator vote tracking:
      </p>

      <h3>Exchange Rates & Prices</h3>

      <table>
        <thead>
          <tr>
            <th>Method</th>
            <th>REST Endpoint</th>
            <th>Purpose</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td><code>ExchangeRate</code></td>
            <td><code>GET /pax-protocol/pax-chain/oracle/denoms/{`{denom}`}/exchange_rate</code></td>
            <td>Get exchange rate for a denom</td>
          </tr>
          <tr>
            <td><code>ExchangeRates</code></td>
            <td><code>GET /pax-protocol/pax-chain/oracle/denoms/exchange_rates</code></td>
            <td>Get all exchange rates</td>
          </tr>
          <tr>
            <td><code>Actives</code></td>
            <td><code>GET /pax-protocol/pax-chain/oracle/denoms/actives</code></td>
            <td>List all active denoms</td>
          </tr>
          <tr>
            <td><code>VoteTargets</code></td>
            <td><code>GET /pax-protocol/pax-chain/oracle/denoms/vote_targets</code></td>
            <td>List vote target denoms</td>
          </tr>
          <tr>
            <td><code>PriceSnapshotHistory</code></td>
            <td><code>GET /pax-protocol/pax-chain/oracle/denoms/price_snapshot_history</code></td>
            <td>Get historical price snapshots</td>
          </tr>
          <tr>
            <td><code>Twaps</code></td>
            <td><code>GET /pax-protocol/pax-chain/oracle/denoms/twaps/{`{lookback_seconds}`}</code></td>
            <td>Get time-weighted average prices</td>
          </tr>
        </tbody>
      </table>

      <h3>Validator Vote Tracking</h3>

      <table>
        <thead>
          <tr>
            <th>Method</th>
            <th>REST Endpoint</th>
            <th>Purpose</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td><code>FeederDelegation</code></td>
            <td><code>GET /pax-protocol/pax-chain/oracle/validators/{`{validator_addr}`}/feeder</code></td>
            <td>Get feeder delegation for validator</td>
          </tr>
          <tr>
            <td><code>VotePenaltyCounter</code></td>
            <td><code>GET /pax-protocol/pax-chain/oracle/validators/{`{validator_addr}`}/vote_penalty_counter</code></td>
            <td>Get oracle miss counter</td>
          </tr>
          <tr>
            <td><code>SlashWindow</code></td>
            <td><code>GET /pax-protocol/pax-chain/oracle/slash_window</code></td>
            <td>Get slash window information</td>
          </tr>
          <tr>
            <td><code>Params</code></td>
            <td><code>GET /pax-protocol/pax-chain/oracle/params</code></td>
            <td>Get oracle module parameters</td>
          </tr>
        </tbody>
      </table>

      <div className="source-note">
        <strong>Proto:</strong> <code>paxeer-network/api/oracle/query.proto</code>
      </div>

      <h2>TokenFactory Module Query Service</h2>

      <p>
        The TokenFactory module manages custom token creation and metadata:
      </p>

      <table>
        <thead>
          <tr>
            <th>Method</th>
            <th>REST Endpoint</th>
            <th>Purpose</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td><code>Params</code></td>
            <td><code>GET /pax-protocol/paxchain/tokenfactory/params</code></td>
            <td>Get tokenfactory parameters</td>
          </tr>
          <tr>
            <td><code>DenomAuthorityMetadata</code></td>
            <td><code>GET /pax-protocol/paxchain/tokenfactory/denoms/{`{denom}`}/authority_metadata</code></td>
            <td>Get denom authority metadata</td>
          </tr>
          <tr>
            <td><code>DenomMetadata</code></td>
            <td><code>GET /pax-protocol/paxchain/tokenfactory/denoms/metadata</code></td>
            <td>Get denom metadata</td>
          </tr>
          <tr>
            <td><code>DenomsFromCreator</code></td>
            <td><code>GET /pax-protocol/paxchain/tokenfactory/denoms_from_creator/{`{creator}`}</code></td>
            <td>List denoms created by an address</td>
          </tr>
          <tr>
            <td><code>DenomAllowList</code></td>
            <td><code>GET /pax-protocol/paxchain/tokenfactory/denoms/allow_list</code></td>
            <td>Get denom allow list</td>
          </tr>
        </tbody>
      </table>

      <div className="source-note">
        <strong>Proto:</strong> <code>paxeer-network/api/tokenfactory/query.proto</code>
      </div>

      <h2>Mint Module Query Service</h2>

      <p>
        The Mint module exposes minting parameters and state:
      </p>

      <table>
        <thead>
          <tr>
            <th>Method</th>
            <th>REST Endpoint</th>
            <th>Purpose</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td><code>Params</code></td>
            <td><code>GET /paxchain/mint/v1beta1/params</code></td>
            <td>Get mint module parameters</td>
          </tr>
          <tr>
            <td><code>Minter</code></td>
            <td><code>GET /paxchain/mint/v1beta1/minter</code></td>
            <td>Get minter state (start/end dates, amounts)</td>
          </tr>
        </tbody>
      </table>

      <div className="source-note">
        <strong>Proto:</strong> <code>paxeer-network/api/mint/v1beta1/query.proto</code>
      </div>

      <h2>Admin gRPC Service</h2>

      <p>
        The Admin service provides runtime log level control. It runs on a separate loopback-only gRPC server (default <code>127.0.0.1:9095</code>):
      </p>

      <table>
        <thead>
          <tr>
            <th>Method</th>
            <th>Purpose</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td><code>SetLogLevel</code></td>
            <td>Change log level for loggers matching a pattern</td>
          </tr>
          <tr>
            <td><code>GetLogLevel</code></td>
            <td>Get current log level for a logger</td>
          </tr>
          <tr>
            <td><code>ListLoggers</code></td>
            <td>List all registered loggers and their levels</td>
          </tr>
        </tbody>
      </table>

      <p>
        Enable in <code>app.toml</code>:
      </p>

      <pre><code>{`[admin_server]
admin_enabled = true
admin_address = "127.0.0.1:9095"`}</code></pre>

      <div className="source-note">
        <strong>Proto:</strong> <code>paxeer-network/api/pax/admin/v0/admin.proto</code><br />
        <strong>Implementation:</strong> <code>paxeer-network/admin/</code>
      </div>

      <h2>OpenAPI / Swagger Documentation</h2>

      <p>
        Paxeer generates OpenAPI documentation from proto annotations. To regenerate:
      </p>

      <pre><code>{`./scripts/update-swagger-ui-statik.sh`}</code></pre>

      <p>
        This generates <code>docs/swagger-ui/swagger.yml</code> and embeds it in <code>docs/swagger/statik.go</code>.
      </p>

      <h3>Serving Swagger UI</h3>

      <p>
        Enable in <code>app.toml</code>:
      </p>

      <pre><code>{`[api]
enable = true
swagger = true`}</code></pre>

      <p>
        Access at <code>http://&lt;node-ip&gt;:&lt;port&gt;/swagger/</code>.
      </p>

      <div className="source-note">
        <strong>See:</strong> <code>paxeer-network/docs/README.md</code> for generation instructions
      </div>

      <h2>gRPC Endpoints</h2>

      <p>
        gRPC services are exposed on the Cosmos SDK gRPC port (default <code>9090</code>). Use any gRPC client (e.g., <code>grpcurl</code>):
      </p>

      <pre><code>{`grpcurl -plaintext localhost:9090 list
grpcurl -plaintext localhost:9090 paxprotocol.paxchain.evm.Query/Pointer`}</code></pre>

      <h2>REST Gateway</h2>

      <p>
        REST endpoints are served via gRPC-Gateway on the API port (default <code>1317</code>). All gRPC methods have corresponding REST endpoints defined by <code>google.api.http</code> annotations in the proto files.
      </p>

      <h2>Transaction Services</h2>

      <p>
        Each module also defines a <code>Msg</code> service for transactions (e.g., <code>evm/tx.proto</code>, <code>epoch/tx.proto</code>). These are not query endpoints but message types for state-changing operations. Submit via standard Cosmos SDK transaction APIs.
      </p>

      <div className="prev-next">
        <Link href="/json-rpc-unsupported">
          <div className="prev-next-label">Previous</div>
          <div className="prev-next-title">Unsupported Methods</div>
        </Link>
        <Link href="/contracts">
          <div className="prev-next-label">Next</div>
          <div className="prev-next-title">Contracts</div>
        </Link>
      </div>
    </DocsLayout>
  )
}
