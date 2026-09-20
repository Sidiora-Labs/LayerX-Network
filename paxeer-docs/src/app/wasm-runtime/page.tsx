import { DocsLayout } from '@/components/DocsLayout'
import Link from 'next/link'

export default function WasmRuntime() {
  return (
    <DocsLayout>
      <div className="page-header">
        <h1 className="page-title">WASM Runtime</h1>
        <p className="page-description">
          The CosmWasm VM, libwasmvm linking, CGO vs no-CGO builds, and WASM execution internals.
        </p>
      </div>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/wasm-runtime/</code>
      </div>

      <h2>Overview</h2>

      <p>
        The WASM runtime is a wrapper around the <a href="https://github.com/CosmWasm/cosmwasm/tree/main/packages/vm">CosmWasm VM</a>. It compiles CosmWasm contracts (written in Rust) to WebAssembly and executes them in a sandboxed environment. The runtime links to <strong>libwasmvm</strong>, a Rust library that provides the core VM functionality.
      </p>

      <h2>libwasmvm</h2>

      <div className="source-note">
        <strong>Source:</strong> <code>paxeer-network/wasm-runtime/libwasmvm/</code>
      </div>

      <p>
        <code>libwasmvm</code> is the Rust implementation of the CosmWasm VM. It compiles to a native library (shared <code>.so</code>, <code>.dylib</code>, or <code>.dll</code>, or static <code>.a</code>) that Go code links via CGO.
      </p>

      <h3>Structure</h3>

      <ul>
        <li><strong>Rust code:</strong> <code>libwasmvm/</code> — VM implementation</li>
        <li><strong>Go bindings:</strong> <code>wasm-runtime/</code> root — CGO wrappers</li>
        <li><strong>Types package:</strong> Go types shared between VM and host</li>
      </ul>

      <h3>Building</h3>

      <p>
        Build the Rust library:
      </p>

      <pre><code>{`cd wasm-runtime/libwasmvm && cargo test       # Run Rust unit tests
make build-rust                                 # Build for current system
make release-build-alpine                       # Reproducible Alpine build
make release-build-linux                        # Reproducible Linux build
make release-build-macos                        # Reproducible macOS build`}</code></pre>

      <h2>CGO vs No-CGO</h2>

      <p>
        The Go code can be built with or without CGO:
      </p>

      <h3>With CGO (Default)</h3>

      <p>
        Links to the native <code>libwasmvm</code> library. Provides full WASM execution:
      </p>

      <pre><code>{`go build .                # Build with CGO
make test                 # Run Go tests with native VM`}</code></pre>

      <p>
        Requires <code>libwasmvm.so</code> (Linux), <code>libwasmvm.dylib</code> (macOS), or <code>libwasmvm.dll</code> (Windows) to be present or statically linked.
      </p>

      <h3>Without CGO</h3>

      <p>
        Compiles without linking to <code>libwasmvm</code>. WASM functionality is disabled:
      </p>

      <pre><code>{`CGO_ENABLED=0 go build .  # Build without CGO`}</code></pre>

      <p>
        Useful for:
      </p>

      <ul>
        <li>Cross-compilation to platforms without CGO support</li>
        <li>Static binaries with no shared library dependencies</li>
        <li>Builds where WASM support is not needed</li>
      </ul>

      <p>
        When built without CGO, WASM contract calls return errors.
      </p>

      <h2>Supported Platforms</h2>

      <p>
        libwasmvm supports platforms with <a href="https://docs.wasmer.io/ecosystem/wasmer/wasmer-features#compiler-support-by-chipset">Wasmer singlepass backend support</a>. This excludes 32-bit systems.
      </p>

      <table>
        <thead>
          <tr>
            <th>OS</th>
            <th>Arch</th>
            <th>Linking</th>
            <th>Status</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td>Linux (glibc)</td>
            <td>x86_64</td>
            <td>shared</td>
            <td>✅ libwasmvm.x86_64.so</td>
          </tr>
          <tr>
            <td>Linux (glibc)</td>
            <td>aarch64</td>
            <td>shared</td>
            <td>✅ libwasmvm.aarch64.so</td>
          </tr>
          <tr>
            <td>Linux (musl)</td>
            <td>x86_64</td>
            <td>static</td>
            <td>✅ libwasmvm_muslc.x86_64.a</td>
          </tr>
          <tr>
            <td>Linux (musl)</td>
            <td>aarch64</td>
            <td>static</td>
            <td>✅ libwasmvm_muslc.aarch64.a</td>
          </tr>
          <tr>
            <td>macOS</td>
            <td>x86_64 / aarch64</td>
            <td>shared</td>
            <td>✅ libwasmvm.dylib (universal binary)</td>
          </tr>
          <tr>
            <td>Windows</td>
            <td>x86_64</td>
            <td>shared</td>
            <td>🏗 wasmvm.dll (in progress)</td>
          </tr>
        </tbody>
      </table>

      <h2>Go Packages</h2>

      <h3>types</h3>

      <p>
        <code>wasm-runtime/types/</code> defines types shared between the VM and host. It can be compiled without CGO:
      </p>

      <pre><code>{`CGO_ENABLED=0 go build ./types`}</code></pre>

      <h3>internal/api</h3>

      <p>
        <code>wasm-runtime/internal/api/</code> contains low-level FFI bindings to <code>libwasmvm</code>. This package is fully private and requires CGO.
      </p>

      <h3>cosmwasm (root)</h3>

      <p>
        The root package (<code>github.com/CosmWasm/wasmvm</code> import) is the public API. It can be compiled without CGO, but WASM functionality is removed:
      </p>

      <pre><code>{`go build .                # Full functionality with CGO
CGO_ENABLED=0 go build .  # Compiles, but WASM calls fail`}</code></pre>

      <h2>Execution Flow</h2>

      <p>
        When a CosmWasm contract is executed:
      </p>

      <ol>
        <li>Go code calls <code>wasm-runtime</code> API</li>
        <li>CGO invokes Rust <code>libwasmvm</code> via FFI</li>
        <li>Rust VM loads WASM bytecode</li>
        <li>VM executes WASM instructions with gas metering</li>
        <li>Contract calls host functions (storage, queries, messages)</li>
        <li>Host functions call back into Go via CGO</li>
        <li>Go executes Cosmos SDK operations</li>
        <li>Results return to Rust VM</li>
        <li>VM returns final result to Go</li>
      </ol>

      <h2>Gas Metering</h2>

      <p>
        The VM instruments WASM bytecode to charge gas for:
      </p>

      <ul>
        <li><strong>Compute:</strong> WASM instructions</li>
        <li><strong>Memory:</strong> Allocations and accesses</li>
        <li><strong>Storage:</strong> Reads and writes</li>
        <li><strong>Host calls:</strong> Queries and message dispatch</li>
      </ul>

      <p>
        Gas costs are calibrated to Cosmos SDK gas units.
      </p>

      <h2>Sandboxing</h2>

      <p>
        CosmWasm contracts run in a sandboxed WASM VM with:
      </p>

      <ul>
        <li><strong>Memory isolation:</strong> No access to host memory</li>
        <li><strong>No syscalls:</strong> No file I/O, network, or process operations</li>
        <li><strong>Deterministic execution:</strong> No floating-point, no random numbers, no wall-clock time</li>
        <li><strong>Gas limits:</strong> Execution terminates if gas is exhausted</li>
      </ul>

      <h2>Host Functions</h2>

      <p>
        Contracts interact with the chain via host functions:
      </p>

      <ul>
        <li><strong>db_read / db_write:</strong> Storage access</li>
        <li><strong>query_chain:</strong> Read-only queries to modules</li>
        <li><strong>canonicalize_address / humanize_address:</strong> Address conversion</li>
        <li><strong>secp256k1_verify / secp256k1_recover_pubkey:</strong> Cryptography</li>
        <li><strong>debug:</strong> Logging (only in test mode)</li>
      </ul>

      <h2>Documentation</h2>

      <p>
        Rust documentation:
      </p>

      <pre><code>{`cd wasm-runtime/libwasmvm && cargo doc --no-deps --open`}</code></pre>

      <h2>Design</h2>

      <p>
        For architecture and specification details, see:
      </p>

      <ul>
        <li><code>wasm-runtime/spec/Architecture.md</code></li>
        <li><code>wasm-runtime/spec/Specification.md</code></li>
      </ul>

      <h2>Next Steps</h2>

      <ul>
        <li><Link href="/wasm">Understand CosmWasm contract lifecycle</Link></li>
        <li><Link href="/wasmbinding">Review Paxeer-specific bindings</Link></li>
      </ul>

      <div className="prev-next">
        <Link href="/wasm">
          <div className="prev-next-label">Previous</div>
          <div className="prev-next-title">WASM</div>
        </Link>
        <Link href="/wasmbinding">
          <div className="prev-next-label">Next</div>
          <div className="prev-next-title">WASM Bindings</div>
        </Link>
      </div>
    </DocsLayout>
  )
}
