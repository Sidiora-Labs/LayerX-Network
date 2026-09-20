import { DocsLayout } from '@/components/DocsLayout'
import Link from 'next/link'

export default function Precompiles() {
  return (
    <DocsLayout>
      <div className="page-header">
        <h1 className="page-title">Precompiles</h1>
        <p className="page-description">
          Paxeer-specific precompiled contracts exposing Cosmos modules to EVM contracts.
        </p>
      </div>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/precompiles/</code>
      </div>

      <h2>Overview</h2>

      <p>
        Precompiles are special contracts deployed at fixed addresses (starting at <code>0x0000000000000000000000000000000000000001</code>) that the EVM treats specially. Instead of executing bytecode, the EVM routes calls to these addresses to native Go code.
      </p>

      <p>
        Paxeer uses precompiles to expose Cosmos SDK modules to EVM contracts, allowing Solidity code to interact with staking, bank, oracle, governance, and other chain features.
      </p>

      <h2>Precompile List</h2>

      <p>
        Paxeer implements the following precompiles:
      </p>

      <h3>addr</h3>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/precompiles/addr/</code>
      </div>

      <p>
        Address utilities for converting between Cosmos bech32 addresses and EVM hex addresses. Contracts call this precompile to resolve <code>pax1...</code> ↔ <code>0x...</code> mappings.
      </p>

      <h3>bank</h3>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/precompiles/bank/</code>
      </div>

      <p>
        Bank module operations:
      </p>

      <ul>
        <li><strong>Balance queries:</strong> Check native token balances</li>
        <li><strong>Send:</strong> Transfer native tokens</li>
        <li><strong>Denom metadata:</strong> Query token info</li>
      </ul>

      <p>
        Allows EVM contracts to interact with native PAX, USDL, and tokenfactory denoms.
      </p>

      <h3>common</h3>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/precompiles/common/</code>
      </div>

      <p>
        Shared precompile utilities:
      </p>

      <ul>
        <li><strong>Event emission:</strong> Emit Cosmos events from precompiles</li>
        <li><strong>ABI encoding:</strong> Encode/decode Solidity types</li>
        <li><strong>Error handling:</strong> Convert Go errors to EVM reverts</li>
      </ul>

      <h3>distribution</h3>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/precompiles/distribution/</code>
      </div>

      <p>
        Distribution module operations:
      </p>

      <ul>
        <li><strong>Withdraw rewards:</strong> Claim staking rewards</li>
        <li><strong>Set withdraw address:</strong> Configure reward recipient</li>
        <li><strong>Query rewards:</strong> Check pending rewards</li>
      </ul>

      <h3>gov</h3>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/precompiles/gov/</code>
      </div>

      <p>
        Governance operations:
      </p>

      <ul>
        <li><strong>Submit proposal:</strong> Create governance proposals</li>
        <li><strong>Vote:</strong> Vote on proposals</li>
        <li><strong>Query proposals:</strong> List and inspect proposals</li>
      </ul>

      <p>
        Enables DAO contracts to interact with on-chain governance.
      </p>

      <h3>ibc</h3>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/precompiles/ibc/</code>
      </div>

      <p>
        IBC transfer operations:
      </p>

      <ul>
        <li><strong>Transfer:</strong> Send tokens to other IBC chains</li>
        <li><strong>Query channels:</strong> List IBC channels and connections</li>
      </ul>

      <h3>json</h3>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/precompiles/json/</code>
      </div>

      <p>
        JSON parsing and encoding for EVM contracts. Allows contracts to work with JSON data without implementing a full parser in Solidity.
      </p>

      <h3>oracle</h3>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/precompiles/oracle/</code>
      </div>

      <p>
        Oracle module queries:
      </p>

      <ul>
        <li><strong>Get exchange rate:</strong> Query canonical asset prices</li>
        <li><strong>Query all rates:</strong> Get all oracle-tracked exchange rates</li>
      </ul>

      <p>
        See <Link href="/modules/oracle">Oracle module</Link> for how price data is aggregated.
      </p>

      <h3>p256</h3>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/precompiles/p256/</code>
      </div>

      <p>
        P-256 (secp256r1) signature verification. This curve is used by WebAuthn and many hardware security modules but not natively supported by the EVM.
      </p>

      <p>
        The precompile provides efficient P-256 verification for passkey-based authentication.
      </p>

      <h3>pointer</h3>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/precompiles/pointer/</code>
      </div>

      <p>
        Pointer contracts link ERC-20 interfaces to native denoms. A pointer allows:
      </p>

      <ul>
        <li>ERC-20 <code>transfer</code> to move native tokens</li>
        <li>Native transfers to emit ERC-20 <code>Transfer</code> events</li>
        <li>Unified balance queries across Cosmos and EVM APIs</li>
      </ul>

      <p>
        Pointers are deployed via the pointer precompile factory.
      </p>

      <h3>pointerview</h3>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/precompiles/pointerview/</code>
      </div>

      <p>
        Read-only queries for pointer contracts:
      </p>

      <ul>
        <li>List all pointers</li>
        <li>Get pointer address for a denom</li>
        <li>Get denom for a pointer address</li>
      </ul>

      <h3>solo</h3>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/precompiles/solo/</code>
      </div>

      <p>
        Single-owner contracts for privileged operations. Used internally for precompile initialization and admin functions.
      </p>

      <h3>staking</h3>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/precompiles/staking/</code>
      </div>

      <p>
        Staking module operations:
      </p>

      <ul>
        <li><strong>Delegate:</strong> Stake tokens to validators</li>
        <li><strong>Undelegate:</strong> Unstake tokens (starts unbonding period)</li>
        <li><strong>Redelegate:</strong> Move stake between validators</li>
        <li><strong>Query delegations:</strong> Check delegation amounts</li>
        <li><strong>Query validators:</strong> List validator set</li>
      </ul>

      <h3>wasmd</h3>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/precompiles/wasmd/</code>
      </div>

      <p>
        CosmWasm contract interaction from EVM:
      </p>

      <ul>
        <li><strong>Execute:</strong> Call CosmWasm contract methods</li>
        <li><strong>Query:</strong> Read CosmWasm contract state</li>
      </ul>

      <p>
        See <Link href="/wasm">WASM documentation</Link> for CosmWasm support.
      </p>

      <h2>Precompile Addresses</h2>

      <p>
        Precompile addresses are determined at chain initialization and registered in the EVM module. Addresses are not declared in the paxeer-network tree; they are assigned by the node application during setup.
      </p>

      <div className="source-note">
        <strong>Note:</strong> Contract addresses for precompiles are not hardcoded in <code>paxeer-network/precompiles/</code>. They are registered at runtime in <code>node/app.go</code>.
      </div>

      <h2>Legacy Precompiles</h2>

      <p>
        Each precompile directory includes a <code>legacy/</code> subdirectory with older implementations retained for backwards compatibility. New code should use the non-legacy versions.
      </p>

      <h2>Setup and Registration</h2>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/precompiles/setup.go</code>
      </div>

      <p>
        Precompiles are registered during node initialization. The <code>setup.go</code> file coordinates:
      </p>

      <ul>
        <li>Address assignment</li>
        <li>Keeper injection (precompiles need access to module keepers)</li>
        <li>Registration with the EVM module</li>
      </ul>

      <h2>Using Precompiles from Solidity</h2>

      <p>
        Contracts interact with precompiles via interfaces. Example for the bank precompile:
      </p>

      <pre><code>{`interface IBank {
    function balance(address account, string calldata denom) external view returns (uint256);
    function send(address to, string calldata denom, uint256 amount) external returns (bool);
}

contract MyContract {
    IBank constant bank = IBank(0x0000000000000000000000000000000000001001); // Example address
    
    function transferPAX(address recipient, uint256 amount) public {
        require(bank.send(recipient, "upax", amount), "transfer failed");
    }
}`}</code></pre>

      <p>
        Interface definitions are available in <code>paxeer-network/contracts/</code>.
      </p>

      <h2>Gas Costs</h2>

      <p>
        Precompiles have custom gas metering. Each precompile defines its gas cost based on the complexity of the underlying Cosmos operation. Gas costs are typically lower than equivalent Solidity implementations because precompiles execute native Go code.
      </p>

      <h2>Next Steps</h2>

      <ul>
        <li><Link href="/contracts">Use precompile interfaces in your contracts</Link></li>
        <li><Link href="/modules">Understand the underlying Cosmos modules</Link></li>
        <li><Link href="/wasm">Explore CosmWasm integration</Link></li>
      </ul>

      <div className="prev-next">
        <Link href="/modules/tokenfactory">
          <div className="prev-next-label">Previous</div>
          <div className="prev-next-title">Token Factory Module</div>
        </Link>
        <Link href="/wasm">
          <div className="prev-next-label">Next</div>
          <div className="prev-next-title">WASM</div>
        </Link>
      </div>
    </DocsLayout>
  )
}
