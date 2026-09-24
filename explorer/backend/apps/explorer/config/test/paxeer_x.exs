import Config

config :explorer,
  json_rpc_named_arguments: [
    transport: EthereumJSONRPC.Mox,
    transport_options: [],
    variant: EthereumJSONRPC.PaxeerX
  ],
  subscribe_named_arguments: [
    transport: EthereumJSONRPC.Mox,
    transport_options: [],
    variant: EthereumJSONRPC.PaxeerX
  ]
