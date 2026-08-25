import { DocsLayout } from '@/components/DocsLayout'
import Link from 'next/link'

export default function Epoch() {
  return (
    <DocsLayout>
      <div className="page-header">
        <h1 className="page-title">Epoch Module</h1>
        <p className="page-description">
          Time-based hooks and epoch lifecycle management for coordinating periodic chain operations.
        </p>
      </div>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/modules/epoch/</code>
      </div>

      <h2>Overview</h2>

      <p>
        The epoch module manages fixed time periods (epochs) and triggers registered hooks when each epoch begins or ends. An epoch defaults to <strong>60 seconds</strong> relative to genesis time. This enables time-coordinated actions like validator set updates, reward distributions, and parameter adjustments.
      </p>

      <p>
        Other modules register hooks via a simple interface, and the epoch module calls those hooks at the start and end of each epoch during <code>BeginBlock</code>.
      </p>

      <h2>State</h2>

      <p>
        The epoch module maintains a single state object:
      </p>

      <pre><code>{`> paxd q epoch epoch --output json
{
  "epoch": {
    "genesis_time": "2023-04-27T19:08:11.958027Z",
    "epoch_duration": "60s",
    "current_epoch": "0",
    "current_epoch_start_time": "2023-04-27T19:08:11.958027Z",
    "current_epoch_height": "0"
  }
}`}</code></pre>

      <ul>
        <li><strong>genesis_time:</strong> Chain genesis time (fixed)</li>
        <li><strong>epoch_duration:</strong> Duration of each epoch in seconds (default 60s)</li>
        <li><strong>current_epoch:</strong> Current epoch number (increments at each epoch boundary)</li>
        <li><strong>current_epoch_start_time:</strong> Timestamp when the current epoch started</li>
        <li><strong>current_epoch_height:</strong> Block height at which the current epoch started</li>
      </ul>

      <h2>Epoch Boundaries</h2>

      <p>
        Epochs are defined by wall-clock time, not block height. The module checks the block timestamp in <code>BeginBlock</code>. If <code>blockTime - current_epoch_start_time ≥ epoch_duration</code>, a new epoch begins.
      </p>

      <p>
        This time-based approach ensures epochs occur at predictable intervals even if block times vary.
      </p>

      <h2>Hooks</h2>

      <p>
        Other modules implement the epoch hooks interface to execute logic at epoch boundaries:
      </p>

      <h3>BeforeEpochStart</h3>

      <pre><code>{`func (k Keeper) BeforeEpochStart(ctx sdk.Context, epoch epochTypes.Epoch) {
  // Execute logic at the start of each epoch
}`}</code></pre>

      <p>
        Called when a new epoch begins, before any other epoch processing.
      </p>

      <h3>AfterEpochEnd</h3>

      <pre><code>{`func (k Keeper) AfterEpochEnd(ctx sdk.Context, epoch epochTypes.Epoch) {
  // Execute logic at the end of each epoch
}`}</code></pre>

      <p>
        Called when an epoch ends, after all epoch processing is complete.
      </p>

      <h3>Example: Mint Module</h3>

      <p>
        The mint module (<code>modules/mint/keeper</code>) implements epoch hooks to distribute inflation rewards daily. It registers its hooks in <code>node/app.go</code>:
      </p>

      <pre><code>{`app.EpochKeeper = *epochmodulekeeper.NewKeeper(
  appCodec,
  keys[epochmoduletypes.StoreKey],
  keys[epochmoduletypes.MemStoreKey],
  app.GetSubspace(epochmoduletypes.ModuleName),
).SetHooks(epochmoduletypes.NewMultiEpochHooks(
  app.MintKeeper.Hooks()))`}</code></pre>

      <h2>Events</h2>

      <p>
        The epoch module emits a <code>new_epoch</code> event at each epoch boundary:
      </p>

      <ul>
        <li><strong>epoch_number:</strong> The new epoch's epoch number</li>
        <li><strong>epoch_time:</strong> The new epoch's start time</li>
        <li><strong>epoch_height:</strong> The block height at which the new epoch started</li>
      </ul>

      <h2>Messages</h2>

      <p>
        The epoch module does not expose any transactions. All interactions happen via hooks and events. Only the module itself updates epoch state during <code>BeginBlock</code>.
      </p>

      <h2>Parameters</h2>

      <p>
        The epoch module has no runtime parameters. The epoch duration is fixed at genesis and cannot be changed without a chain upgrade.
      </p>

      <h2>Use Cases</h2>

      <ul>
        <li><strong>Mint module:</strong> Distribute inflation rewards once per epoch (daily)</li>
        <li><strong>Validator updates:</strong> Apply validator set changes at epoch boundaries</li>
        <li><strong>Parameter changes:</strong> Activate governance-approved parameter updates</li>
        <li><strong>Periodic cleanup:</strong> Prune expired state or cache entries</li>
      </ul>

      <h2>Next Steps</h2>

      <ul>
        <li><Link href="/modules/mint">Understand how the mint module uses epochs</Link></li>
        <li><Link href="/modules">Review all Paxeer modules</Link></li>
      </ul>

      <div className="prev-next">
        <Link href="/modules">
          <div className="prev-next-label">Previous</div>
          <div className="prev-next-title">Modules Overview</div>
        </Link>
        <Link href="/modules/mint">
          <div className="prev-next-label">Next</div>
          <div className="prev-next-title">Mint Module</div>
        </Link>
      </div>
    </DocsLayout>
  )
}
