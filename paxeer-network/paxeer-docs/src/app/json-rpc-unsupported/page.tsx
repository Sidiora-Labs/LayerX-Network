import { DocsLayout } from '@/components/DocsLayout'
import Link from 'next/link'

export default function JsonRpcUnsupported() {
  return (
    <DocsLayout>
      <div className="page-header">
        <h1 className="page-title">Unsupported JSON-RPC Methods</h1>
        <p className="page-description">
          Ethereum JSON-RPC methods that are explicitly unsupported on Paxeer EVM.
        </p>
      </div>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/docs/evm_jsonrpc_unsupported.md</code> and <code>paxeer-network/rpc/AGENTS.md</code>
      </div>

      <h2>Overview</h2>

      <p>
        Some Ethereum JSON-RPC methods are <strong>registered</strong> on Paxeer's EVM endpoint but return a <strong>JSON-RPC error</strong> instead of a result. This gives clients and tools a stable, documented failure (code <code>-32000</code>) rather than "method not found" (<code>-32601</code>).
      </p>

      <h2>Explicitly Unsupported Methods</h2>

      <table>
        <thead>
          <tr>
            <th>Method</th>
            <th>Typical Error Message</th>
            <th>Reason</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td><code>eth_blobBaseFee</code></td>
            <td>"blobs not supported on this chain"</td>
            <td>EIP-4844 blob transactions not supported</td>
          </tr>
          <tr>
            <td><code>eth_syncing</code></td>
            <td>"eth_syncing is not supported on Pax EVM RPC"</td>
            <td>Consensus model differs from Ethereum sync semantics</td>
          </tr>
          <tr>
            <td><code>eth_newPendingTransactionFilter</code></td>
            <td>"eth_newPendingTransactionFilter is not supported on Pax EVM RPC"</td>
            <td>Instant finality, no Ethereum-style pending tx filters</td>
          </tr>
          <tr>
            <td><code>debug_getRawBlock</code></td>
            <td>"debug_getRawBlock is not supported on Pax EVM RPC"</td>
            <td>Raw RLP block payloads not served</td>
          </tr>
          <tr>
            <td><code>debug_getRawHeader</code></td>
            <td>"debug_getRawHeader is not supported on Pax EVM RPC"</td>
            <td>Raw RLP header payloads not served</td>
          </tr>
          <tr>
            <td><code>debug_getRawReceipts</code></td>
            <td>"debug_getRawReceipts is not supported on Pax EVM RPC"</td>
            <td>Raw RLP receipt payloads not served</td>
          </tr>
          <tr>
            <td><code>debug_getRawTransaction</code></td>
            <td>"debug_getRawTransaction is not supported on Pax EVM RPC"</td>
            <td>Raw RLP transaction payloads not served</td>
          </tr>
        </tbody>
      </table>

      <h2>Error Response Format</h2>

      <p>
        Unsupported methods return a standard JSON-RPC error:
      </p>

      <pre><code>{`{
  "jsonrpc": "2.0",
  "id": 1,
  "error": {
    "code": -32000,
    "message": "blobs not supported on this chain"
  }
}`}</code></pre>

      <h2>Behavior Notes</h2>

      <h3>eth_syncing</h3>

      <p>
        Paxeer's BFT consensus model differs fundamentally from Ethereum's sync semantics. Callers should not rely on this method to determine node sync status.
      </p>

      <h3>eth_newPendingTransactionFilter</h3>

      <p>
        Paxeer has instant finality with no Ethereum-style pending state. Transactions are either included in a finalized block or not yet submitted.
      </p>

      <h3>debug_getRaw* Methods</h3>

      <p>
        Raw RLP-encoded block, header, receipt, and transaction payloads are not served on this RPC surface. Use the standard <code>eth_*</code> endpoints which return structured JSON.
      </p>

      <h3>eth_blobBaseFee</h3>

      <p>
        Paxeer does not support EIP-4844 blob transactions. This is a permanent architectural decision, not a temporary limitation.
      </p>

      <h2>Broader Compatibility Notes</h2>

      <p>
        Beyond explicitly unsupported methods, Paxeer's RPC diverges from Ethereum in several conceptual areas:
      </p>

      <h3>No Pending Blocks</h3>

      <ul>
        <li>The <code>pending</code> block tag is accepted but treated as <code>latest</code>/<code>safe</code>/<code>final</code></li>
        <li>All blocks are instantly finalized (BFT consensus)</li>
      </ul>

      <h3>No Uncle Blocks</h3>

      <ul>
        <li>Instant BFT finality means no uncle blocks</li>
        <li>Uncle-related endpoints are not supported</li>
      </ul>

      <h3>No Trie Endpoints</h3>

      <ul>
        <li>Paxeer does not store state in Ethereum-style tries</li>
        <li>Trie-related endpoints (<code>eth_getProof</code>, etc.) are not supported</li>
      </ul>

      <h3>No Proof-of-Work</h3>

      <ul>
        <li><code>eth_mining</code> and <code>eth_hashrate</code> are not supported</li>
        <li>Paxeer uses BFT consensus, not PoW</li>
      </ul>

      <p>
        See <Link href="/json-rpc">JSON-RPC API</Link> for full compatibility details.
      </p>

      <h2>Integration Coverage</h2>

      <p>
        Each unsupported method has dedicated integration test coverage:
      </p>

      <pre><code>{`integration_test/evm_module/rpc_io_test/testdata/<method>/not-supported.iox`}</code></pre>

      <p>
        This ensures the error responses remain stable and documented.
      </p>

      <div className="prev-next">
        <Link href="/json-rpc">
          <div className="prev-next-label">Previous</div>
          <div className="prev-next-title">JSON-RPC API</div>
        </Link>
        <Link href="/rest-grpc">
          <div className="prev-next-label">Next</div>
          <div className="prev-next-title">REST & gRPC</div>
        </Link>
      </div>
    </DocsLayout>
  )
}
