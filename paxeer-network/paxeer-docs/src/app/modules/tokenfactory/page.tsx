import { DocsLayout } from '@/components/DocsLayout'
import Link from 'next/link'

export default function TokenFactory() {
  return (
    <DocsLayout>
      <div className="page-header">
        <h1 className="page-title">Token Factory Module</h1>
        <p className="page-description">
          Permissionless creation and management of native token denominations with namespace protection.
        </p>
      </div>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/modules/tokenfactory/</code>
      </div>

      <h2>Overview</h2>

      <p>
        The tokenfactory module allows any account to create new native tokens. Tokens are namespaced by creator address as <code>factory/&#123;creator address&#125;/&#123;subdenom&#125;</code>, eliminating name collisions. A single account can create multiple denoms by providing unique subdenoms.
      </p>

      <p>
        The original creator is granted admin privileges over the token, allowing them to mint, burn, transfer, and change admin.
      </p>

      <h2>Token Naming</h2>

      <p>
        Tokenfactory denoms follow the format:
      </p>

      <pre><code>{`factory/{creator_address}/{subdenom}`}</code></pre>

      <p>
        Example:
      </p>

      <pre><code>{`factory/pax166vhptur29s3gw5qr6dm30s06gej6pr4zevkqk/ufoo`}</code></pre>

      <ul>
        <li><strong>Prefix:</strong> Always <code>factory</code> (7 bytes)</li>
        <li><strong>Creator address:</strong> Bech32 address (max 75 bytes)</li>
        <li><strong>Subdenom:</strong> User-defined (max 44 bytes, <code>[a-zA-Z0-9./]</code>)</li>
      </ul>

      <p>
        Total denom length must not exceed 128 bytes (Cosmos SDK constraint).
      </p>

      <h2>Messages</h2>

      <h3>CreateDenom</h3>

      <p>
        Create a new denom:
      </p>

      <pre><code>{`message MsgCreateDenom {
    string sender = 1;
    string subdenom = 2;
}`}</code></pre>

      <p>
        CLI:
      </p>

      <pre><code>{`paxd tx tokenfactory create-denom ufoo --from mylocalwallet`}</code></pre>

      <p>
        State modifications:
      </p>

      <ul>
        <li>Set <code>DenomMetadata</code> via bank keeper</li>
        <li>Set <code>AuthorityMetadata</code> with sender as admin</li>
        <li>Add denom to <code>CreatorPrefixStore</code></li>
      </ul>

      <h3>Mint</h3>

      <p>
        Mint tokens (admin only):
      </p>

      <pre><code>{`message MsgMint {
    string sender = 1;
    cosmos.base.v1beta1.Coin amount = 2;
}`}</code></pre>

      <p>
        CLI:
      </p>

      <pre><code>{`paxd tx tokenfactory mint 100000000000factory/pax166vh.../ufoo --from mylocalwallet`}</code></pre>

      <p>
        Checks:
      </p>

      <ul>
        <li>Denom was created via tokenfactory</li>
        <li>Sender is the admin</li>
      </ul>

      <p>
        Mints the specified amount via the bank module.
      </p>

      <h3>Burn</h3>

      <p>
        Burn tokens (admin only):
      </p>

      <pre><code>{`message MsgBurn {
    string sender = 1;
    cosmos.base.v1beta1.Coin amount = 2;
}`}</code></pre>

      <p>
        Checks:
      </p>

      <ul>
        <li>Denom was created via tokenfactory</li>
        <li>Sender is the admin</li>
      </ul>

      <p>
        Burns the specified amount via the bank module.
      </p>

      <h3>ChangeAdmin</h3>

      <p>
        Transfer admin privileges:
      </p>

      <pre><code>{`message MsgChangeAdmin {
    string sender = 1;
    string denom = 2;
    string newAdmin = 3;
}`}</code></pre>

      <p>
        The sender must be the current admin. Set <code>newAdmin</code> to <code>""</code> to renounce admin privileges (irreversible).
      </p>

      <h2>Admin Capabilities</h2>

      <p>
        The admin can:
      </p>

      <ul>
        <li><strong>Mint:</strong> Create new tokens of the denom</li>
        <li><strong>Burn:</strong> Destroy tokens from any account</li>
        <li><strong>Force Transfer:</strong> Move tokens between any two accounts</li>
        <li><strong>Change Admin:</strong> Transfer or renounce admin rights</li>
      </ul>

      <p>
        Admins can share privileges via the authz module without changing the master admin.
      </p>

      <h2>Queries</h2>

      <h3>Denom Metadata</h3>

      <pre><code>{`paxd query bank denom-metadata --denom factory/pax166vh.../ufoo`}</code></pre>

      <h3>Denoms by Creator</h3>

      <pre><code>{`paxd query tokenfactory denoms-from-creator pax166vhptur29s3gw5qr6dm30s06gej6pr4zevkqk`}</code></pre>

      <h2>Restrictions</h2>

      <p>
        To fit within the 128-byte Cosmos SDK denom limit:
      </p>

      <ul>
        <li><strong>Max subdenom length:</strong> 44 characters</li>
        <li><strong>Max creator address length:</strong> 75 characters (bech32 prefix ≤ 16 chars)</li>
      </ul>

      <p>
        Calculation:
      </p>

      <pre><code>{`len("factory") + 2*len("/") + len(creator_address) + len(subdenom) ≤ 128
7 + 2 + 75 + 44 = 128`}</code></pre>

      <h2>Use Cases</h2>

      <ul>
        <li><strong>Wrapped assets:</strong> Mint tokens representing off-chain assets</li>
        <li><strong>Synthetic tokens:</strong> Create tokens backed by other on-chain assets</li>
        <li><strong>Protocol-managed denoms:</strong> Modules create denoms for internal accounting</li>
        <li><strong>Governance tokens:</strong> DAOs issue native governance tokens</li>
      </ul>

      <h2>Integration with Bank Module</h2>

      <p>
        Tokenfactory denoms are native tokens, not ERC-20 contracts. They integrate with the bank module for:
      </p>

      <ul>
        <li>Balance queries</li>
        <li>Transfers via <code>MsgSend</code></li>
        <li>IBC transfers</li>
        <li>Distribution to stakers</li>
      </ul>

      <h2>EVM Integration</h2>

      <p>
        Tokenfactory denoms can be exposed to EVM contracts via <Link href="/precompiles">pointer precompiles</Link>, allowing ERC-20-style interaction from Solidity.
      </p>

      <h2>Next Steps</h2>

      <ul>
        <li><Link href="/precompiles">Use precompiles to expose tokenfactory denoms to EVM</Link></li>
        <li><Link href="/modules">Review other Paxeer modules</Link></li>
      </ul>

      <div className="prev-next">
        <Link href="/modules/store">
          <div className="prev-next-label">Previous</div>
          <div className="prev-next-title">Store Module</div>
        </Link>
        <Link href="/precompiles">
          <div className="prev-next-label">Next</div>
          <div className="prev-next-title">Precompiles</div>
        </Link>
      </div>
    </DocsLayout>
  )
}
