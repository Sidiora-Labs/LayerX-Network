import { DocsLayout } from '@/components/DocsLayout'
import Link from 'next/link'

export default function Storage() {
  return (
    <DocsLayout>
      <div className="page-header">
        <h1 className="page-title">Storage</h1>
        <p className="page-description">
          PaxDB: Paxeer's next-generation storage engine with state commitment, state store, WAL, and performance optimizations.
        </p>
      </div>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/storage/</code>
      </div>

      <h2>Overview</h2>

      <p>
        PaxDB is Paxeer's custom storage layer designed to replace the traditional IAVL store used in Cosmos chains. It dramatically reduces state size, improves sync times, and increases throughput while maintaining Merkle proof capabilities.
      </p>

      <h2>Key Improvements</h2>

      <p>
        PaxDB delivers measurable performance gains over IAVL:
      </p>

      <ul>
        <li><strong>60% reduction</strong> in active chain state size</li>
        <li><strong>~90% reduction</strong> in historical data growth rate</li>
        <li><strong>1200% faster</strong> state sync times</li>
        <li><strong>2x faster</strong> block sync</li>
        <li><strong>287x improvement</strong> in block commit times</li>
        <li><strong>2x TPS increase</strong> from faster state access and commit</li>
      </ul>

      <p>
        Archive nodes maintain the same performance as full nodes.
      </p>

      <h2>Architecture</h2>

      <p>
        PaxDB splits storage into two layers, inspired by the <a href="https://docs.cosmos.network/main/build/architecture/adr-065-store-v2">Cosmos StoreV2 ADR</a>:
      </p>

      <h3>State Commitment (SC) Layer</h3>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/storage/state_db/sc/</code>
      </div>

      <p>
        The SC layer stores the active chain state in a memory-mapped Merkle tree. It provides:
      </p>

      <ul>
        <li><strong>Root app hash:</strong> Merkle root for each block</li>
        <li><strong>Fast state access:</strong> Direct memory-mapped reads for transaction execution</li>
        <li><strong>State sync support:</strong> Export/import snapshots for fast bootstrap</li>
        <li><strong>Historical proofs:</strong> Merkle proofs for heights not yet pruned</li>
      </ul>

      <p>
        PaxDB forks <a href="https://github.com/crypto-org-chain/cronos/tree/main/memiavl">MemIAVL</a> for the SC layer. MemIAVL uses the same Merkelized AVL tree structure as Cosmos SDK's IAVL but represents it with memory-mapped flat files instead of key-value pairs in a database. This eliminates database overhead and improves access latency.
      </p>

      <h3>State Store (SS) Layer</h3>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/storage/state_db/ss/</code>
      </div>

      <p>
        The SS layer provides versioned key-value storage for historical queries. It stores raw key-value pairs without Merkle tree overhead, saving disk space and reducing write amplification.
      </p>

      <p>
        Responsibilities:
      </p>

      <ul>
        <li><strong>Versioned queries:</strong> Read state at any historical height</li>
        <li><strong>CRUD operations:</strong> Create, read, update, delete with version tracking</li>
        <li><strong>Batching:</strong> Atomic multi-key updates</li>
        <li><strong>Iteration:</strong> Range scans across key prefixes</li>
        <li><strong>Pruning:</strong> Remove old versions to reclaim disk space</li>
      </ul>

      <h3>Trade-offs</h3>

      <p>
        PaxDB optimizes for active state performance at the cost of:
      </p>

      <ul>
        <li><strong>No historical Merkle proofs:</strong> Proofs are only available for recent, unpruned heights</li>
        <li><strong>Limited integrity checks:</strong> Historical data in SS layer has no cryptographic proof of correctness</li>
      </ul>

      <p>
        For most use cases (validators, full nodes, RPC), these trade-offs are acceptable. Archive nodes still serve historical queries efficiently.
      </p>

      <h2>Database Engine</h2>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/storage/db_engine/</code>
      </div>

      <p>
        PaxDB supports multiple database backends for the SS layer:
      </p>

      <h3>PebbleDB (Default)</h3>

      <p>
        <code>storage/db_engine/pebbledb/</code> provides the default backend. Extensive benchmarking of LevelDB, RocksDB, PebbleDB, and SQLite showed <strong>PebbleDB performs best</strong> for Paxeer's workload (random writes, reads, forward/backward iteration).
      </p>

      <h3>RocksDB</h3>

      <p>
        <code>storage/db_engine/rocksdb/</code> is available as an alternative backend.
      </p>

      <h3>Litt</h3>

      <p>
        <code>storage/db_engine/litt/</code> provides a custom embedded storage backend.
      </p>

      <h3>Backend Interface</h3>

      <p>
        <code>storage/db_engine/types/</code> defines the database interface. New backends can be plugged in by implementing this interface.
      </p>

      <h2>Write-Ahead Log (WAL)</h2>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/storage/wal/</code>
      </div>

      <p>
        The WAL provides crash recovery for state writes. Before committing state changes to the database, PaxDB writes the changes to a sequential log file. If the node crashes mid-commit, the WAL can replay uncommitted writes on restart.
      </p>

      <h2>Common Utilities</h2>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/storage/common/</code>
      </div>

      <p>
        Shared storage utilities:
      </p>

      <ul>
        <li><strong>Errors:</strong> <code>common/errors/</code> — storage error types</li>
        <li><strong>Keys:</strong> <code>common/keys/</code> — key formatting and prefixing</li>
        <li><strong>Iterators:</strong> <code>common/iterators/</code> — range scan helpers</li>
        <li><strong>Metrics:</strong> <code>common/metrics/</code> — Prometheus instrumentation</li>
        <li><strong>Threading:</strong> <code>common/threading/</code> — concurrency primitives</li>
        <li><strong>Utils:</strong> <code>common/utils/</code> — byte manipulation, encoding</li>
      </ul>

      <h2>Benchmarking and Tools</h2>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/storage/tools/</code>
      </div>

      <p>
        Storage benchmarking and profiling tools:
      </p>

      <ul>
        <li><strong>Bench:</strong> <code>tools/bench/</code> — performance benchmarks for each backend</li>
        <li><strong>Commands:</strong> <code>tools/cmd/</code> — CLI tools for state inspection and migration</li>
      </ul>

      <h2>Protobuf Definitions</h2>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/storage/proto/</code>
      </div>

      <p>
        Protocol buffer definitions for storage data structures, including MemIAVL tree nodes and snapshots.
      </p>

      <h2>Configuration</h2>

      <p>
        Storage configuration is set in the node's <code>config.toml</code>:
      </p>

      <ul>
        <li><strong>Backend:</strong> <code>pebbledb</code> (default), <code>rocksdb</code>, or <code>litt</code></li>
        <li><strong>Pruning:</strong> Keep last N versions or prune by age</li>
        <li><strong>Cache size:</strong> In-memory cache for hot keys</li>
        <li><strong>Compaction:</strong> Background compaction settings</li>
      </ul>

      <h2>State Sync</h2>

      <p>
        PaxDB's SC layer supports state sync, allowing new nodes to bootstrap by downloading a recent state snapshot instead of replaying the entire chain history. This reduces sync time from hours or days to minutes.
      </p>

      <p>
        State sync is coordinated by the consensus layer (<Link href="/consensus">consensus state sync</Link>) and uses PaxDB's export/import APIs to serialize and restore Merkle tree snapshots.
      </p>

      <h2>Integration with Consensus</h2>

      <p>
        PaxDB is used by the Cosmos SDK's base app to store module state. The consensus layer (<Link href="/consensus">consensus</Link>) commits blocks, which triggers PaxDB to:
      </p>

      <ol>
        <li>Write state changes to WAL</li>
        <li>Update SC layer Merkle tree</li>
        <li>Compute root app hash</li>
        <li>Write versioned state to SS layer</li>
        <li>Prune old versions if configured</li>
      </ol>

      <h2>Next Steps</h2>

      <ul>
        <li><Link href="/configuration">Configure storage backends</Link></li>
        <li><Link href="/operators">Run a validator with PaxDB</Link></li>
        <li><Link href="/modules">Understand module state storage</Link></li>
      </ul>

      <div className="prev-next">
        <Link href="/evm">
          <div className="prev-next-label">Previous</div>
          <div className="prev-next-title">EVM</div>
        </Link>
        <Link href="/modules">
          <div className="prev-next-label">Next</div>
          <div className="prev-next-title">Modules Overview</div>
        </Link>
      </div>
    </DocsLayout>
  )
}
