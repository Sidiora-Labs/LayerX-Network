import { DocsLayout } from '@/components/DocsLayout'
import Link from 'next/link'

export default function Wasm() {
  return (
    <DocsLayout>
      <div className="page-header">
        <h1 className="page-title">WASM</h1>
        <p className="page-description">
          WebAssembly smart contract support via CosmWasm on Paxeer.
        </p>
      </div>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/wasm/</code>
      </div>

      <h2>Overview</h2>

      <p>
        Paxeer supports CosmWasm smart contracts alongside native EVM execution. CosmWasm contracts are written in Rust, compiled to WebAssembly (WASM), and deployed to the chain. They run in a sandboxed VM and interact with Cosmos modules via a message-passing interface.
      </p>

      <p>
        CosmWasm provides an alternative to EVM contracts with different trade-offs:
      </p>

      <table>
        <thead>
          <tr>
            <th>Feature</th>
            <th>EVM</th>
            <th>CosmWasm</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td>Language</td>
            <td>Solidity, Vyper</td>
            <td>Rust</td>
          </tr>
          <tr>
            <td>Compilation target</td>
            <td>EVM bytecode</td>
            <td>WebAssembly</td>
          </tr>
          <tr>
            <td>Standard library</td>
            <td>Ethereum ABI</td>
            <td>CosmWasm SDK</td>
          </tr>
          <tr>
            <td>State model</td>
            <td>Key-value (256-bit keys)</td>
            <td>Key-value (arbitrary keys)</td>
          </tr>
          <tr>
            <td>Module integration</td>
            <td>Precompiles</td>
            <td>Native messages</td>
          </tr>
          <tr>
            <td>Gas model</td>
            <td>EVM gas</td>
            <td>Cosmos gas</td>
          </tr>
        </tbody>
      </table>

      <h2>CosmWasm Integration</h2>

      <p>
        Paxeer integrates the <code>x/wasm</code> module from CosmWasm to support WASM contracts. The module provides:
      </p>

      <ul>
        <li><strong>Contract deployment:</strong> Upload and instantiate WASM contracts</li>
        <li><strong>Execution:</strong> Call contract entry points</li>
        <li><strong>Queries:</strong> Read contract state</li>
        <li><strong>Migrations:</strong> Upgrade contract code</li>
        <li><strong>Admin:</strong> Grant admin privileges for contract management</li>
      </ul>

      <h2>Contract Lifecycle</h2>

      <h3>1. Upload Code</h3>

      <p>
        Upload a compiled WASM binary:
      </p>

      <pre><code>{`paxd tx wasm store contract.wasm --from deployer --gas auto`}</code></pre>

      <p>
        Returns a <code>code_id</code> for the uploaded code.
      </p>

      <h3>2. Instantiate Contract</h3>

      <p>
        Create a contract instance from uploaded code:
      </p>

      <pre><code>{`paxd tx wasm instantiate [code_id] '{"init_msg": {...}}' \\
  --label "My Contract" --from deployer --gas auto`}</code></pre>

      <p>
        Returns a contract address.
      </p>

      <h3>3. Execute</h3>

      <p>
        Call a contract method:
      </p>

      <pre><code>{`paxd tx wasm execute [contract_address] '{"method": {...}}' \\
  --from caller --gas auto`}</code></pre>

      <h3>4. Query</h3>

      <p>
        Read contract state:
      </p>

      <pre><code>{`paxd q wasm contract-state smart [contract_address] '{"query": {...}}'`}</code></pre>

      <h3>5. Migrate</h3>

      <p>
        Upgrade contract code (requires admin):
      </p>

      <pre><code>{`paxd tx wasm migrate [contract_address] [new_code_id] '{"migrate_msg": {...}}' \\
  --from admin --gas auto`}</code></pre>

      <h2>Message Interface</h2>

      <p>
        CosmWasm contracts interact with the chain via messages:
      </p>

      <ul>
        <li><strong>BankMsg:</strong> Send tokens</li>
        <li><strong>StakingMsg:</strong> Delegate, undelegate</li>
        <li><strong>DistributionMsg:</strong> Withdraw rewards</li>
        <li><strong>WasmMsg:</strong> Call other WASM contracts</li>
        <li><strong>Custom:</strong> Chain-specific messages (see <Link href="/wasmbinding">wasmbinding</Link>)</li>
      </ul>

      <h2>Storage Model</h2>

      <p>
        CosmWasm contracts have isolated key-value storage. Keys are arbitrary byte strings (not limited to 256 bits like EVM). The WASM VM handles state reads/writes via host functions.
      </p>

      <h2>Gas Metering</h2>

      <p>
        WASM execution is gas-metered at the instruction level. Gas costs are calibrated to match Cosmos SDK gas units. The <Link href="/wasm-runtime">WASM runtime</Link> instruments the WASM bytecode to charge gas for memory, compute, and storage operations.
      </p>

      <h2>Permissions</h2>

      <p>
        Code upload and instantiation can be restricted via governance:
      </p>

      <ul>
        <li><strong>Nobody:</strong> No one can upload (WASM disabled)</li>
        <li><strong>Everybody:</strong> Any account can upload (permissionless)</li>
        <li><strong>Specific addresses:</strong> Whitelist of authorized uploaders</li>
      </ul>

      <p>
        Check current permissions:
      </p>

      <pre><code>{`paxd q wasm params`}</code></pre>

      <h2>Interoperability with EVM</h2>

      <p>
        CosmWasm contracts and EVM contracts can interact via:
      </p>

      <ul>
        <li><strong>wasmd precompile:</strong> EVM contracts call WASM contracts via the <Link href="/precompiles">wasmd precompile</Link></li>
        <li><strong>Native modules:</strong> Both contract types can interact with bank, staking, oracle modules</li>
      </ul>

      <p>
        Direct WASM-to-EVM calls are not supported. Contracts must route through shared modules.
      </p>

      <h2>Contract Examples</h2>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/example/cosmwasm/</code>
      </div>

      <p>
        Example CosmWasm contracts for Paxeer live in <code>example/cosmwasm/</code>.
      </p>

      <h2>Testing</h2>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/integration_test/wasm_module/</code>
      </div>

      <p>
        Integration tests for the WASM module live in <code>integration_test/wasm_module/</code>.
      </p>

      <h2>Next Steps</h2>

      <ul>
        <li><Link href="/wasm-runtime">Understand the WASM runtime</Link></li>
        <li><Link href="/wasmbinding">Review custom Paxeer bindings</Link></li>
        <li><Link href="/precompiles">Use the wasmd precompile from EVM</Link></li>
      </ul>

      <div className="prev-next">
        <Link href="/precompiles">
          <div className="prev-next-label">Previous</div>
          <div className="prev-next-title">Precompiles</div>
        </Link>
        <Link href="/wasm-runtime">
          <div className="prev-next-label">Next</div>
          <div className="prev-next-title">WASM Runtime</div>
        </Link>
      </div>
    </DocsLayout>
  )
}
