import { DocsLayout } from '@/components/DocsLayout'

const placeholderPages = [
  { path: 'configuration', title: 'Configuration', prev: 'run-node', next: 'operators' },
  { path: 'operators', title: 'Operators Guide', prev: 'configuration', next: 'consensus' },
  { path: 'consensus', title: 'Consensus', prev: 'operators', next: 'engine' },
  { path: 'engine', title: 'Engine', prev: 'consensus', next: 'evm' },
  { path: 'evm', title: 'EVM', prev: 'engine', next: 'storage' },
  { path: 'storage', title: 'Storage', prev: 'evm', next: 'modules' },
  { path: 'modules/epoch', title: 'Epoch Module', prev: 'modules', next: 'modules/mint' },
  { path: 'modules/mint', title: 'Mint Module', prev: 'modules/epoch', next: 'modules/oracle' },
  { path: 'modules/oracle', title: 'Oracle Module', prev: 'modules/mint', next: 'modules/store' },
  { path: 'modules/store', title: 'Store Module', prev: 'modules/oracle', next: 'modules/tokenfactory' },
  { path: 'modules/tokenfactory', title: 'Token Factory Module', prev: 'modules/store', next: 'precompiles' },
  { path: 'precompiles', title: 'Precompiles', prev: 'modules/tokenfactory', next: 'wasm' },
  { path: 'wasm', title: 'WASM', prev: 'precompiles', next: 'wasm-runtime' },
  { path: 'wasm-runtime', title: 'WASM Runtime', prev: 'wasm', next: 'wasmbinding' },
  { path: 'wasmbinding', title: 'WASM Bindings', prev: 'wasm-runtime', next: 'json-rpc' },
  { path: 'rest-grpc', title: 'REST & gRPC', prev: 'json-rpc-unsupported', next: 'contracts' },
  { path: 'contracts', title: 'Contracts', prev: 'rest-grpc', next: 'docker' },
  { path: 'docker', title: 'Docker', prev: 'contracts', next: 'sdk' },
  { path: 'sdk', title: 'SDK', prev: 'docker', next: 'interchain' },
  { path: 'interchain', title: 'Interchain', prev: 'sdk', next: 'admin-hpx' },
  { path: 'admin-hpx', title: 'Admin & HPX', prev: 'interchain', next: '' },
]

export default placeholderPages
