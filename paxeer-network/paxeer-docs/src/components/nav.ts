export const navStructure = [
  {
    section: 'Getting Started',
    items: [
      { title: 'Introduction', href: '/' },
      { title: 'Paxeer vs LayerX', href: '/paxeer-vs-layerx' },
      { title: 'Network Parameters', href: '/network-parameters' },
      { title: 'Installation & Build', href: '/installation' },
    ]
  },
  {
    section: 'Running Nodes',
    items: [
      { title: 'Run a Node', href: '/run-node' },
      { title: 'Configuration', href: '/configuration' },
      { title: 'Operators Guide', href: '/operators' },
    ]
  },
  {
    section: 'Architecture',
    items: [
      { title: 'Consensus', href: '/consensus' },
      { title: 'Engine', href: '/engine' },
      { title: 'EVM', href: '/evm' },
      { title: 'Storage', href: '/storage' },
    ]
  },
  {
    section: 'Modules',
    items: [
      { title: 'Overview', href: '/modules' },
      { title: 'Epoch', href: '/modules/epoch' },
      { title: 'Mint', href: '/modules/mint' },
      { title: 'Oracle', href: '/modules/oracle' },
      { title: 'Store', href: '/modules/store' },
      { title: 'Token Factory', href: '/modules/tokenfactory' },
    ]
  },
  {
    section: 'Precompiles & WASM',
    items: [
      { title: 'Precompiles', href: '/precompiles' },
      { title: 'WASM', href: '/wasm' },
      { title: 'WASM Runtime', href: '/wasm-runtime' },
      { title: 'WASM Bindings', href: '/wasmbinding' },
    ]
  },
  {
    section: 'APIs',
    items: [
      { title: 'JSON-RPC', href: '/json-rpc' },
      { title: 'Unsupported Methods', href: '/json-rpc-unsupported' },
      { title: 'REST & gRPC', href: '/rest-grpc' },
    ]
  },
  {
    section: 'Advanced',
    items: [
      { title: 'Contracts', href: '/contracts' },
      { title: 'Docker', href: '/docker' },
      { title: 'SDK', href: '/sdk' },
      { title: 'Interchain', href: '/interchain' },
      { title: 'Admin & HPX', href: '/admin-hpx' },
    ]
  },
]

export function pageTitleForPath(pathname: string): string {
  return navStructure
    .flatMap((section) => section.items)
    .find((item) => item.href === pathname)?.title ?? 'Paxeer Network'
}
