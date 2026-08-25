import { DocsLayout } from '@/components/DocsLayout'
import Link from 'next/link'

export default function Contracts() {
  return (
    <DocsLayout>
      <div className="page-header">
        <h1 className="page-title">Contracts</h1>
        <p className="page-description">
          Paxeer-native Solidity contracts and LayerX settlement contracts deployed on Paxeer.
        </p>
      </div>

      <div className="source-note">
        <strong>Paxeer-native:</strong> <code>paxeer-network/contracts/</code><br />
        <strong>LayerX settlement:</strong> <code>contracts/</code> at repository root
      </div>

      <h2>Two Contract Trees</h2>

      <p>
        The monorepo contains two distinct contract directories serving different purposes:
      </p>

      <table>
        <thead>
          <tr>
            <th>Path</th>
            <th>Purpose</th>
            <th>Tooling</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td><code>paxeer-network/contracts/</code></td>
            <td>Paxeer-native chain utilities: WPAX, pointer contracts, precompile interfaces, testing infrastructure</td>
            <td>Foundry + Hardhat</td>
          </tr>
          <tr>
            <td><code>contracts/</code> (root)</td>
            <td>LayerX settlement contracts: custody, checkpoints, guarantor bonds, challenges, emergency exits</td>
            <td>Solidity 0.8.28</td>
          </tr>
        </tbody>
      </table>

      <h2>Paxeer-Native Contracts</h2>

      <p>
        The contracts under <code>paxeer-network/contracts/</code> are part of the Paxeer chain itself. They provide EVM-side interfaces to chain functionality.
      </p>

      <h3>Core Contracts</h3>

      <table>
        <thead>
          <tr>
            <th>Contract</th>
            <th>Purpose</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td><code>WPAX.sol</code></td>
            <td>Wrapped PAX (ERC-20 wrapper for native PAX gas token)</td>
          </tr>
          <tr>
            <td><code>CW20ERC20Pointer.sol</code></td>
            <td>EVM pointer for Cosmos CW20 tokens</td>
          </tr>
          <tr>
            <td><code>CW721ERC721Pointer.sol</code></td>
            <td>EVM pointer for Cosmos CW721 NFTs</td>
          </tr>
          <tr>
            <td><code>CW1155ERC1155Pointer.sol</code></td>
            <td>EVM pointer for Cosmos CW1155 multi-tokens</td>
          </tr>
          <tr>
            <td><code>NativePaxTokensERC20.sol</code></td>
            <td>ERC-20 interface for native Cosmos tokens</td>
          </tr>
        </tbody>
      </table>

      <h3>Precompile Interfaces</h3>

      <p>
        Solidity interfaces for Paxeer's native precompiles:
      </p>

      <table>
        <thead>
          <tr>
            <th>Interface</th>
            <th>Precompile Address</th>
            <th>Purpose</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td><code>IAddr.sol</code></td>
            <td>TBD</td>
            <td>Address conversion (EVM ↔ Cosmos)</td>
          </tr>
          <tr>
            <td><code>IBank.sol</code></td>
            <td>TBD</td>
            <td>Native token operations</td>
          </tr>
          <tr>
            <td><code>IWasmd.sol</code></td>
            <td>TBD</td>
            <td>CosmWasm contract calls from EVM</td>
          </tr>
          <tr>
            <td><code>IJson.sol</code></td>
            <td>TBD</td>
            <td>JSON parsing and manipulation</td>
          </tr>
        </tbody>
      </table>

      <div className="source-note">
        <strong>Note:</strong> Precompile addresses are not documented in the contract source. Check deployment artifacts or chain configuration for actual addresses.
      </div>

      <h3>Testing Contracts</h3>

      <p>
        Development and testing utilities:
      </p>

      <ul>
        <li><code>EVMCompatibilityTester.sol</code> — EVM compatibility validation</li>
        <li><code>TransientStorageTester.sol</code> — EIP-1153 transient storage tests</li>
        <li><code>SelfDestructTester.sol</code> — SELFDESTRUCT behavior verification</li>
        <li><code>SnapshotRevertTester.sol</code> — State snapshot and revert testing</li>
        <li><code>SstoreGasTest.sol</code> — SSTORE gas cost verification</li>
        <li><code>ProxySwapTester.sol</code>, <code>MultiHopSwapTester.sol</code> — DEX testing</li>
        <li><code>MultiSender.sol</code>, <code>BatchCallAndSponsor.sol</code> — Batch operations</li>
        <li><code>Box.sol</code>, <code>BoxV2.sol</code> — Upgradeable proxy testing</li>
        <li><code>TestToken.sol</code>, <code>ERC721.sol</code>, <code>ERC1155.sol</code> — Token testing</li>
      </ul>

      <h3>Build & Test</h3>

      <p>
        The Paxeer contracts support both Foundry and Hardhat workflows:
      </p>

      <pre><code>{`# Foundry
cd paxeer-network/contracts
forge install
forge build

# Hardhat
cd paxeer-network/contracts
npm install
npx hardhat compile
npx hardhat test --network paxlocal`}</code></pre>

      <p>
        Hardhat configuration (<code>hardhat.config.js</code>) targets Solidity 0.8.28 with Prague EVM and includes networks:
      </p>

      <ul>
        <li><code>paxlocal</code> — <code>http://127.0.0.1:8545</code> (Docker cluster)</li>
        <li><code>devnet</code> — <code>https://evm-rpc.arctic-1.paxnetwork.io/</code></li>
      </ul>

      <h3>Updating Pointer Contracts</h3>

      <p>
        When pointer contract bytecode changes:
      </p>

      <ol>
        <li>Compile with Foundry: <code>forge build</code></li>
        <li>Extract bytecode from <code>contracts/out/</code></li>
        <li>Copy to corresponding <code>.bin</code> file under <code>modules/evm/contracts/</code></li>
        <li>Restart <code>paxd</code></li>
      </ol>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/contracts/README.md</code>, <code>paxeer-network/contracts/src/</code>
      </div>

      <h2>LayerX Settlement Contracts</h2>

      <p>
        The contracts at the repository root (<code>contracts/</code>) implement LayerX settlement on Paxeer. These contracts are <strong>deployed on Paxeer</strong> but are <strong>owned by LayerX</strong> for checkpoint settlement, custody, and exits.
      </p>

      <h3>Settlement Contracts</h3>

      <table>
        <thead>
          <tr>
            <th>Contract</th>
            <th>Purpose</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td><code>CheckpointRegistry.sol</code></td>
            <td>LayerX checkpoint settlement (state root hashes, block numbers, timestamps)</td>
          </tr>
          <tr>
            <td><code>LayerXCustody.sol</code></td>
            <td>Custody of USDL backing USDX (LayerX L1 asset held on Paxeer)</td>
          </tr>
          <tr>
            <td><code>GuarantorBond.sol</code></td>
            <td>Guarantor collateral and slashing for LayerX sequencer/validator set</td>
          </tr>
          <tr>
            <td><code>WithdrawalClaims.sol</code></td>
            <td>User exit claims from LayerX to Paxeer (merkle proofs against checkpoints)</td>
          </tr>
          <tr>
            <td><code>EmergencyExit.sol</code></td>
            <td>Last-resort withdrawal mechanism if LayerX sequencer halts</td>
          </tr>
        </tbody>
      </table>

      <h3>Supporting Directories</h3>

      <ul>
        <li><code>challenge/</code> — Challenge and dispute contracts</li>
        <li><code>custody/</code> — Custody-related helpers</li>
        <li><code>governance/</code> — LayerX governance contracts</li>
        <li><code>interfaces/</code> — Contract interfaces</li>
        <li><code>libraries/</code> — Shared libraries (merkle proofs, signature verification)</li>
        <li><code>manager/</code> — Settlement manager contracts</li>
        <li><code>security/</code> — Security modules (pausability, access control)</li>
        <li><code>storage/</code> — Storage layout contracts</li>
        <li><code>config/</code> — Deployment configuration</li>
        <li><code>deployment/</code> — Deployment scripts and artifacts</li>
      </ul>

      <h3>Deployment Addresses</h3>

      <p>
        <strong>No deployment addresses are documented in the tree.</strong> Check <code>contracts/deployment/</code> or the LayerX dashboard for deployed contract addresses on Paxeer mainnet.
      </p>

      <div className="source-note">
        <strong>Warning:</strong> Do not invent or assume contract addresses. If not in the source tree, they are not public.
      </div>

      <h3>Contract Ownership</h3>

      <p>
        LayerX settlement contracts are owned and controlled by LayerX governance, not by Paxeer validators. Paxeer provides the settlement layer; LayerX operates the settlement contracts.
      </p>

      <div className="source-note">
        <strong>Source:</strong> <code>contracts/</code> at repository root
      </div>

      <h2>Key Distinctions</h2>

      <table>
        <thead>
          <tr>
            <th>Aspect</th>
            <th>Paxeer-native</th>
            <th>LayerX settlement</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td><strong>Location</strong></td>
            <td><code>paxeer-network/contracts/</code></td>
            <td><code>contracts/</code> (root)</td>
          </tr>
          <tr>
            <td><strong>Purpose</strong></td>
            <td>Chain utilities, pointers, testing</td>
            <td>LayerX checkpoints, custody, exits</td>
          </tr>
          <tr>
            <td><strong>Deployment</strong></td>
            <td>Part of Paxeer chain (precompiles, pointers)</td>
            <td>Deployed as contracts on Paxeer</td>
          </tr>
          <tr>
            <td><strong>Ownership</strong></td>
            <td>Paxeer chain</td>
            <td>LayerX governance</td>
          </tr>
          <tr>
            <td><strong>Trust boundary</strong></td>
            <td>Paxeer validators</td>
            <td>LayerX sequencer + Paxeer settlement</td>
          </tr>
        </tbody>
      </table>

      <div className="prev-next">
        <Link href="/rest-grpc">
          <div className="prev-next-label">Previous</div>
          <div className="prev-next-title">REST & gRPC</div>
        </Link>
        <Link href="/docker">
          <div className="prev-next-label">Next</div>
          <div className="prev-next-title">Docker</div>
        </Link>
      </div>
    </DocsLayout>
  )
}
