import { DocsLayout } from '@/components/DocsLayout'
import Link from 'next/link'

export default function EVM() {
  return (
    <DocsLayout>
      <div className="page-header">
        <h1 className="page-title">EVM Module</h1>
        <p className="page-description">
          Native EVM execution, address association, receipts, pointers, and precompile integration on chain ID 125.
        </p>
      </div>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/modules/evm/</code>
      </div>

      <h2>Overview</h2>

      <p>
        The EVM module (<code>modules/evm/</code>) provides full Ethereum Virtual Machine execution on Paxeer. It runs as a Cosmos SDK module and integrates with the <Link href="/engine">execution engine</Link> to process EVM transactions with chain ID <strong>125</strong>.
      </p>

      <p>
        The module handles:
      </p>

      <ul>
        <li><strong>EVM Transaction Processing:</strong> Execute Ethereum-format transactions</li>
        <li><strong>Address Association:</strong> Map Cosmos bech32 addresses to EVM hex addresses</li>
        <li><strong>Receipt Generation:</strong> Create transaction receipts with logs and status</li>
        <li><strong>Pointers:</strong> Link ERC-20 tokens to native Cosmos denoms</li>
        <li><strong>Precompiles:</strong> Expose Cosmos modules via EVM precompiled contracts</li>
      </ul>

      <h2>Message Types</h2>

      <p>
        The EVM module handles <code>MsgEthereumTx</code>, which wraps a standard Ethereum transaction:
      </p>

      <ul>
        <li>Legacy transactions (type 0)</li>
        <li>EIP-2930 access list transactions (type 1)</li>
        <li>EIP-1559 dynamic fee transactions (type 2)</li>
      </ul>

      <p>
        Transactions are submitted via the JSON-RPC <code>eth_sendRawTransaction</code> endpoint or the Cosmos SDK <code>MsgEthereumTx</code> message.
      </p>

      <h2>Address Association</h2>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/engine/deps/xevm/keeper/address.go</code>
      </div>

      <p>
        Paxeer addresses exist in two formats:
      </p>

      <ul>
        <li><strong>Cosmos:</strong> <code>pax1...</code> (bech32, 42 chars)</li>
        <li><strong>EVM:</strong> <code>0x...</code> (hex, 20 bytes)</li>
      </ul>

      <p>
        The EVM module maintains a bidirectional mapping between these formats. When a Cosmos account sends an EVM transaction, the module associates its <code>pax1...</code> address with the derived <code>0x...</code> address. This allows:
      </p>

      <ul>
        <li>Cosmos accounts to interact with EVM contracts</li>
        <li>EVM accounts to hold native denoms (PAX, USDL)</li>
        <li>Unified balance queries across both address formats</li>
      </ul>

      <h2>State Management</h2>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/engine/deps/xevm/keeper/state.go</code>
      </div>

      <p>
        The EVM module stores:
      </p>

      <ul>
        <li><strong>Accounts:</strong> Nonce, balance (via bank module)</li>
        <li><strong>Code:</strong> Contract bytecode (<code>keeper/code.go</code>)</li>
        <li><strong>Storage:</strong> Contract storage slots (<code>keeper/state.go</code>)</li>
        <li><strong>Receipts:</strong> Transaction receipts and logs (<code>keeper/receipt.go</code>)</li>
      </ul>

      <p>
        EVM state is stored in the module's KVStore. Account balances are managed by the bank module to maintain consistency with native token operations.
      </p>

      <h2>Transaction Execution Flow</h2>

      <p>
        When an EVM transaction is submitted:
      </p>

      <ol>
        <li>RPC receives <code>eth_sendRawTransaction</code> or SDK receives <code>MsgEthereumTx</code></li>
        <li>Ante handler (<code>keeper/ante.go</code>) validates signature, checks nonce, charges fees</li>
        <li>Msg server routes to EVM module</li>
        <li>Module calls <Link href="/engine">engine executor</Link> with <code>ExecuteTransactionFeeCharged</code></li>
        <li>Executor runs EVM bytecode against state DB</li>
        <li>Module generates receipt (<code>keeper/receipt.go</code>) with logs, gas used, status</li>
        <li>Module emits events for indexing</li>
      </ol>

      <h2>Receipts</h2>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/engine/deps/xevm/keeper/receipt.go</code>
      </div>

      <p>
        Transaction receipts include:
      </p>

      <ul>
        <li><strong>Status:</strong> 1 (success) or 0 (revert)</li>
        <li><strong>Gas Used:</strong> Total gas consumed</li>
        <li><strong>Logs:</strong> Event logs emitted during execution</li>
        <li><strong>Contract Address:</strong> For contract creation transactions</li>
        <li><strong>Bloom Filter:</strong> For efficient log filtering</li>
      </ul>

      <p>
        Receipts are stored in the module's KVStore and returned via <code>eth_getTransactionReceipt</code>.
      </p>

      <h2>Pointers</h2>

      <p>
        Pointers are a Paxeer-specific feature that links ERC-20 token contracts to native Cosmos denoms. A pointer allows:
      </p>

      <ul>
        <li>ERC-20 <code>transfer</code> to move native tokens via bank module</li>
        <li>Native token transfers to appear as ERC-20 events</li>
        <li>Unified balance view across EVM and Cosmos APIs</li>
      </ul>

      <p>
        Pointer contracts are deployed via the <Link href="/precompiles">pointer precompile</Link>.
      </p>

      <h2>Precompile Integration</h2>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/engine/deps/xevm/keeper/precompile.go</code>
      </div>

      <p>
        The EVM module registers custom precompiled contracts that expose Cosmos SDK modules to EVM callers. Precompile addresses start at <code>0x0000000000000000000000000000000000000001</code> and are handled specially by the EVM.
      </p>

      <p>
        See <Link href="/precompiles">Precompiles documentation</Link> for the full list of Paxeer precompiles (bank, staking, oracle, IBC, etc.).
      </p>

      <h2>Fee Collection</h2>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/engine/deps/xevm/keeper/fee.go</code>
      </div>

      <p>
        EVM transaction fees are:
      </p>

      <ul>
        <li>Charged in PAX (native gas token)</li>
        <li>Calculated as <code>gasUsed * gasPrice</code></li>
        <li>Sent to the fee collector module</li>
        <li>Distributed to validators and stakers</li>
      </ul>

      <p>
        EIP-1559 dynamic fees are supported with <code>baseFee</code> and priority fees.
      </p>

      <h2>Coinbase</h2>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/engine/deps/xevm/keeper/coinbase.go</code>
      </div>

      <p>
        The block coinbase address (visible in EVM via <code>COINBASE</code> opcode) is set to the proposer's EVM-format address. Block rewards are handled by the mint module and distribution module, not by direct coinbase transfers.
      </p>

      <h2>Chain ID</h2>

      <p>
        Paxeer uses <strong>EVM chain ID 125</strong>. This must match the <code>chainId</code> field in EIP-155 transaction signatures for replay protection. The Cosmos chain identifier is <code>hyperpax_125-1</code>.
      </p>

      <h2>Configuration</h2>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/engine/deps/xevm/config/</code>
      </div>

      <p>
        EVM module parameters include:
      </p>

      <ul>
        <li><strong>EVM Denom:</strong> Native token used for gas (PAX)</li>
        <li><strong>Enable Call/Create:</strong> Allow contract calls and creation</li>
        <li><strong>Extra EIPs:</strong> Activate additional Ethereum improvement proposals</li>
        <li><strong>Chain Config:</strong> Fork heights for Homestead, Byzantium, Constantinople, Istanbul, Berlin, London, etc.</li>
      </ul>

      <h2>Genesis</h2>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/engine/deps/xevm/keeper/genesis.go</code>
      </div>

      <p>
        Genesis state for the EVM module includes:
      </p>

      <ul>
        <li>Module parameters</li>
        <li>Pre-deployed contract code and storage</li>
        <li>Address associations</li>
      </ul>

      <h2>Logging</h2>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/engine/deps/xevm/keeper/log.go</code>
      </div>

      <p>
        EVM event logs are emitted during transaction execution via the <code>LOG0</code>, <code>LOG1</code>, <code>LOG2</code>, <code>LOG3</code>, <code>LOG4</code> opcodes. Logs are included in receipts and indexed for querying via <code>eth_getLogs</code>.
      </p>

      <h2>Deferred Operations</h2>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/engine/deps/xevm/keeper/deferred.go</code>
      </div>

      <p>
        Some EVM operations (like balance updates from precompiles) are deferred and batched for commit. This avoids redundant state writes during execution and improves performance.
      </p>

      <h2>Next Steps</h2>

      <ul>
        <li><Link href="/json-rpc">Use the JSON-RPC API</Link></li>
        <li><Link href="/precompiles">Explore Paxeer precompiles</Link></li>
        <li><Link href="/contracts">Deploy contracts</Link></li>
      </ul>

      <div className="prev-next">
        <Link href="/engine">
          <div className="prev-next-label">Previous</div>
          <div className="prev-next-title">Engine</div>
        </Link>
        <Link href="/storage">
          <div className="prev-next-label">Next</div>
          <div className="prev-next-title">Storage</div>
        </Link>
      </div>
    </DocsLayout>
  )
}
