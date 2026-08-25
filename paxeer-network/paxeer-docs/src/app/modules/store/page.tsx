import { DocsLayout } from '@/components/DocsLayout'
import Link from 'next/link'

export default function Store() {
  return (
    <DocsLayout>
      <div className="page-header">
        <h1 className="page-title">Store Module</h1>
        <p className="page-description">
          Module-level store integration helpers for key formatting, iterators, and codec utilities.
        </p>
      </div>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/modules/store/</code>
      </div>

      <h2>Overview</h2>

      <p>
        The store module provides utilities for Cosmos SDK module state management. It standardizes key formatting, iteration patterns, and marshaling/unmarshaling operations that other Paxeer modules use to interact with <Link href="/storage">PaxDB storage</Link>.
      </p>

      <h2>Key Formatting</h2>

      <p>
        Modules store state in key-value pairs. The store module provides helpers for consistent key prefixing:
      </p>

      <ul>
        <li><strong>Prefix constants:</strong> Each module defines unique byte prefixes for its keys</li>
        <li><strong>Key builders:</strong> Functions to construct keys from module prefix + identifier</li>
        <li><strong>Collision avoidance:</strong> Ensures different modules and different state types within a module never overlap</li>
      </ul>

      <p>
        Example pattern:
      </p>

      <pre><code>{`const (
    AccountPrefix = byte(0x01)
    CodePrefix    = byte(0x02)
    StoragePrefix = byte(0x03)
)

func AccountKey(address common.Address) []byte {
    return append([]byte{AccountPrefix}, address.Bytes()...)
}`}</code></pre>

      <h2>Iterator Utilities</h2>

      <p>
        The store module provides helpers for range queries:
      </p>

      <ul>
        <li><strong>Prefix iteration:</strong> Iterate all keys sharing a prefix</li>
        <li><strong>Range bounds:</strong> Start and end keys for bounded iteration</li>
        <li><strong>Reverse iteration:</strong> Iterate keys in descending order</li>
      </ul>

      <p>
        These utilities wrap the underlying PaxDB iterators and handle edge cases like prefix overflow and empty ranges.
      </p>

      <h2>Codec Helpers</h2>

      <p>
        The store module standardizes state marshaling:
      </p>

      <ul>
        <li><strong>Protobuf encoding:</strong> Marshal/unmarshal state structs to/from bytes</li>
        <li><strong>JSON encoding:</strong> For human-readable export/import</li>
        <li><strong>Legacy amino support:</strong> For backwards compatibility with older state formats</li>
      </ul>

      <p>
        Modules call these helpers instead of directly invoking codec methods, ensuring consistent encoding across the codebase.
      </p>

      <h2>Common Patterns</h2>

      <h3>Get/Set State</h3>

      <pre><code>{`func (k Keeper) GetAccount(ctx sdk.Context, addr common.Address) (Account, error) {
    store := ctx.KVStore(k.storeKey)
    bz := store.Get(AccountKey(addr))
    if bz == nil {
        return Account{}, errors.New("account not found")
    }
    var acc Account
    k.cdc.MustUnmarshal(bz, &acc)
    return acc, nil
}

func (k Keeper) SetAccount(ctx sdk.Context, addr common.Address, acc Account) {
    store := ctx.KVStore(k.storeKey)
    bz := k.cdc.MustMarshal(&acc)
    store.Set(AccountKey(addr), bz)
}`}</code></pre>

      <h3>Iterate All Accounts</h3>

      <pre><code>{`func (k Keeper) IterateAccounts(ctx sdk.Context, cb func(addr common.Address, acc Account) bool) {
    store := ctx.KVStore(k.storeKey)
    iter := store.Iterator(PrefixRange(AccountPrefix))
    defer iter.Close()
    
    for ; iter.Valid(); iter.Next() {
        var acc Account
        k.cdc.MustUnmarshal(iter.Value(), &acc)
        addr := common.BytesToAddress(iter.Key()[1:]) // skip prefix byte
        if cb(addr, acc) {
            break
        }
    }
}`}</code></pre>

      <h2>State Migration</h2>

      <p>
        The store module includes utilities for state migration during chain upgrades:
      </p>

      <ul>
        <li><strong>Key remapping:</strong> Move state from old keys to new keys</li>
        <li><strong>Schema changes:</strong> Transform state from old format to new format</li>
        <li><strong>Batching:</strong> Migrate large state in chunks to avoid running out of gas</li>
      </ul>

      <h2>Integration with PaxDB</h2>

      <p>
        The store module sits above <Link href="/storage">PaxDB</Link>. It provides a module-friendly API while PaxDB handles the low-level storage engine details (state commitment, state store, WAL).
      </p>

      <p>
        Modules call store helpers → store helpers call SDK KVStore → SDK KVStore calls PaxDB.
      </p>

      <h2>Testing Utilities</h2>

      <p>
        The store module provides test helpers:
      </p>

      <ul>
        <li><strong>In-memory stores:</strong> Fast, isolated storage for unit tests</li>
        <li><strong>Mock contexts:</strong> Simulate SDK context with controlled block height, time</li>
        <li><strong>State snapshots:</strong> Capture and restore state for test isolation</li>
      </ul>

      <h2>Next Steps</h2>

      <ul>
        <li><Link href="/storage">Understand PaxDB storage architecture</Link></li>
        <li><Link href="/modules">Review other Paxeer modules</Link></li>
      </ul>

      <div className="prev-next">
        <Link href="/modules/oracle">
          <div className="prev-next-label">Previous</div>
          <div className="prev-next-title">Oracle Module</div>
        </Link>
        <Link href="/modules/tokenfactory">
          <div className="prev-next-label">Next</div>
          <div className="prev-next-title">Token Factory Module</div>
        </Link>
      </div>
    </DocsLayout>
  )
}
