import { DocsLayout } from '@/components/DocsLayout'
import Link from 'next/link'

export default function Engine() {
  return (
    <DocsLayout>
      <div className="page-header">
        <h1 className="page-title">Engine</h1>
        <p className="page-description">
          Paxeer's EVM execution engine, transaction processing, and state transition management.
        </p>
      </div>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/engine/</code>
      </div>

      <h2>Overview</h2>

      <p>
        The engine package provides EVM transaction execution for Paxeer. It wraps go-ethereum's EVM with Paxeer-specific precompiles, state management, and fee handling. The engine sits between the consensus layer (which orders transactions) and the EVM module (which manages on-chain state).
      </p>

      <h2>Executor</h2>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/engine/executor/</code>
      </div>

      <p>
        The executor is the core EVM execution wrapper. It provides two entry points:
      </p>

      <h3>Executor Types</h3>

      <p>
        The <code>Executor</code> struct in <code>engine/executor/executor.go</code> supports two backends:
      </p>

      <ul>
        <li><strong>Geth Executor:</strong> Uses go-ethereum's native EVM interpreter</li>
        <li><strong>Evmone Executor:</strong> Uses the evmone C++ VM via EVMC bindings for performance</li>
      </ul>

      <pre><code>{`// From engine/executor/executor.go
type Executor struct {
    evm *vm.EVM
}

func NewGethExecutor(blockCtx vm.BlockContext, stateDB vm.StateDB, 
    chainConfig *params.ChainConfig, config vm.Config, 
    customPrecompiles map[common.Address]vm.PrecompiledContract) *Executor

func NewEvmoneExecutor(evmoneVM *evmc.VM, blockCtx vm.BlockContext, 
    stateDB vm.StateDB, chainConfig *params.ChainConfig, config vm.Config, 
    customPrecompiles map[common.Address]vm.PrecompiledContract) *Executor`}</code></pre>

      <h3>Transaction Execution</h3>

      <p>
        The executor provides two execution modes:
      </p>

      <h4>Standard Execution</h4>

      <pre><code>{`func (e *Executor) ExecuteTransaction(tx *types.Transaction, 
    sender common.Address, baseFee *big.Int, 
    gasPool *core.GasPool) (*core.ExecutionResult, error)`}</code></pre>

      <p>
        Standard execution charges gas fees from the sender, executes the transaction, and refunds unused gas. This path is used for direct EVM transaction submission.
      </p>

      <h4>Fee-Already-Charged Execution</h4>

      <pre><code>{`func (e *Executor) ExecuteTransactionFeeCharged(tx *types.Transaction, 
    sender common.Address, baseFee *big.Int, 
    gasPool *core.GasPool) (*core.ExecutionResult, error)`}</code></pre>

      <p>
        This mode assumes fees were already charged by the Cosmos SDK ante handler. It skips fee deduction/refund and only increments the sender nonce. This matches how EVM transactions flow through the SDK's <code>msg_server</code> path where the ante handler processes fees separately.
      </p>

      <h3>Internal Components</h3>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/engine/executor/internal/</code>
      </div>

      <p>
        Internal execution utilities:
      </p>

      <ul>
        <li><strong>HostContext:</strong> EVMC host context for evmone integration</li>
        <li><strong>Signer:</strong> Transaction signature verification</li>
        <li><strong>Interpreter:</strong> EVM opcode interpreter wrapper</li>
      </ul>

      <h2>Precompiles</h2>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/engine/executor/precompiles/</code>
      </div>

      <p>
        Paxeer-specific precompiled contracts are registered with the executor at initialization. The executor passes the <code>customPrecompiles</code> map to the EVM, which routes calls to addresses like <code>0x00...01</code>, <code>0x00...02</code>, etc.
      </p>

      <p>
        See <Link href="/precompiles">Precompiles documentation</Link> for the full list of Paxeer precompiles (bank, staking, oracle, pointer, etc.).
      </p>

      <h2>Dependencies (xbank, xevm)</h2>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/engine/deps/</code>
      </div>

      <p>
        The engine depends on Cosmos SDK modules via thin wrappers in <code>engine/deps/</code>:
      </p>

      <h3>xbank</h3>

      <p>
        <code>engine/deps/xbank/</code> wraps the SDK bank module for balance operations:
      </p>

      <ul>
        <li><strong>Send:</strong> Transfer tokens between accounts</li>
        <li><strong>View:</strong> Query balances and supply</li>
        <li><strong>Deferred Cache:</strong> Batch balance updates before commit</li>
      </ul>

      <h3>xevm</h3>

      <p>
        <code>engine/deps/xevm/</code> wraps the EVM module keeper:
      </p>

      <ul>
        <li><strong>State:</strong> EVM storage access (accounts, code, storage slots)</li>
        <li><strong>Code:</strong> Contract bytecode storage and retrieval</li>
        <li><strong>Receipt:</strong> Transaction receipt generation</li>
        <li><strong>Address:</strong> Cosmos ↔ EVM address association</li>
        <li><strong>Nonce:</strong> Account nonce management</li>
        <li><strong>Fee:</strong> Gas price and fee collection</li>
        <li><strong>Coinbase:</strong> Block reward recipient</li>
        <li><strong>Precompile:</strong> Precompile address registration</li>
      </ul>

      <h2>Configuration</h2>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/engine/executor/config/</code>
      </div>

      <p>
        Executor configuration includes:
      </p>

      <ul>
        <li><strong>EVM backend:</strong> geth vs evmone</li>
        <li><strong>Chain config:</strong> Chain ID (125), fork heights, EVM rules</li>
        <li><strong>Precompiles:</strong> Custom precompile address map</li>
      </ul>

      <h2>Testing</h2>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/engine/tests/</code>
      </div>

      <p>
        The engine includes comprehensive test suites:
      </p>

      <ul>
        <li><strong>State Tests:</strong> <code>state_test.go</code> runs Ethereum state transition tests</li>
        <li><strong>Harness:</strong> <code>tests/harness/</code> provides test fixtures and builders</li>
        <li><strong>Giga Test:</strong> <code>giga_test.go</code> for large-scale execution</li>
      </ul>

      <h2>Chain ID</h2>

      <p>
        Paxeer uses <strong>EVM chain ID 125</strong>. This is hardcoded in the chain configuration and must match the <code>chainId</code> field in EVM transactions for replay protection.
      </p>

      <h2>Utilities</h2>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/engine/executor/utils/</code>
      </div>

      <p>
        Shared utilities for address conversion, gas calculation, and result formatting.
      </p>

      <h2>Integration with EVM Module</h2>

      <p>
        The engine is invoked by the EVM module's <code>msg_server</code> when processing <code>MsgEthereumTx</code> messages. The flow is:
      </p>

      <ol>
        <li>Consensus orders transactions in a block</li>
        <li>ABCI DeliverTx routes <code>MsgEthereumTx</code> to the EVM module</li>
        <li>EVM module's ante handler charges fees</li>
        <li>EVM module calls <code>ExecuteTransactionFeeCharged</code> on the engine</li>
        <li>Engine executes EVM bytecode against the state DB</li>
        <li>EVM module generates receipt and emits events</li>
      </ol>

      <p>
        See <Link href="/evm">EVM Module documentation</Link> for the full message handling flow.
      </p>

      <h2>Next Steps</h2>

      <ul>
        <li><Link href="/evm">Understand the EVM module</Link></li>
        <li><Link href="/precompiles">Review Paxeer precompiles</Link></li>
        <li><Link href="/json-rpc">Use the JSON-RPC API</Link></li>
      </ul>

      <div className="prev-next">
        <Link href="/consensus">
          <div className="prev-next-label">Previous</div>
          <div className="prev-next-title">Consensus</div>
        </Link>
        <Link href="/evm">
          <div className="prev-next-label">Next</div>
          <div className="prev-next-title">EVM</div>
        </Link>
      </div>
    </DocsLayout>
  )
}
