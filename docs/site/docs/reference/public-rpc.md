# Public RPC endpoints

Paxeer X Network exposes the EVM execution domain (chain ID 125) through a
set of public full nodes. Each endpoint fronts one node with TLS and serves
the standard Ethereum JSON-RPC surface over HTTPS and WebSocket.

The limited beta has not opened yet. These endpoints are read and broadcast
surfaces for the live chain; the gateway API and developer allocations become
available when the beta opens.

## Endpoints

| Node | HTTPS JSON-RPC | WebSocket |
| --- | --- | --- |
| 1 | `https://api1.mainnet-beta.paxeer.network` | `wss://api1.mainnet-beta.paxeer.network/ws` |
| 2 | `https://api2.mainnet-beta.paxeer.network` | `wss://api2.mainnet-beta.paxeer.network/ws` |
| 3 | `https://api3.mainnet-beta.paxeer.network` | `wss://api3.mainnet-beta.paxeer.network/ws` |
| 4 | `https://api4.mainnet-beta.paxeer.network` | `wss://api4.mainnet-beta.paxeer.network/ws` |
| 5 | `https://api5.mainnet-beta.paxeer.network` | `wss://api5.mainnet-beta.paxeer.network/ws` |
| 6 | `https://api6.mainnet-beta.paxeer.network` | `wss://api6.mainnet-beta.paxeer.network/ws` |
| 7 | `https://api7.mainnet-beta.paxeer.network` | `wss://api7.mainnet-beta.paxeer.network/ws` |
| 8 | `https://api8.mainnet-beta.paxeer.network` | `wss://api8.mainnet-beta.paxeer.network/ws` |
| 9 | `https://api9.mainnet-beta.paxeer.network` | `wss://api9.mainnet-beta.paxeer.network/ws` |
| 10 | `https://api10.mainnet-beta.paxeer.network` | `wss://api10.mainnet-beta.paxeer.network/ws` |
| 11 | `https://api11.mainnet-beta.paxeer.network` | `wss://api11.mainnet-beta.paxeer.network/ws` |
| 12 | `https://api12.mainnet-beta.paxeer.network` | `wss://api12.mainnet-beta.paxeer.network/ws` |
| 13 | `https://api13.mainnet-beta.paxeer.network` | `wss://api13.mainnet-beta.paxeer.network/ws` |
| 14 | `https://api14.mainnet-beta.paxeer.network` | `wss://api14.mainnet-beta.paxeer.network/ws` |
| 15 | `https://api15.mainnet-beta.paxeer.network` | `wss://api15.mainnet-beta.paxeer.network/ws` |
| 16 | `https://api16.mainnet-beta.paxeer.network` | `wss://api16.mainnet-beta.paxeer.network/ws` |

Endpoints are numbered, not ranked. Pick any one, and fall back to another if
it stops answering. Nodes may briefly trail the chain tip; compare
`eth_blockNumber` across two endpoints if you need the newest head.

## Network parameters

| Parameter | Value |
| --- | --- |
| Network name | Paxeer X Network |
| Chain ID | 125 |
| Native coin | Paxeer (PAX), 18 decimals |
| Consensus chain ID | hyperpax_125-1 |

## Usage

```bash
curl -s https://api1.mainnet-beta.paxeer.network \
  -H 'content-type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"eth_blockNumber","params":[]}'
```

```bash
wscat -c wss://api1.mainnet-beta.paxeer.network/ws
> {"jsonrpc":"2.0","id":1,"method":"eth_subscribe","params":["newHeads"]}
```

WebSocket upgrades are also accepted on the root path, so clients that only
take one URL can use `wss://api1.mainnet-beta.paxeer.network`.

## Limits and conduct

- Each endpoint is a single node behind a reverse proxy. There is no request
  quota today, but sustained heavy polling on one node degrades it for others;
  spread load across endpoints or use subscriptions.
- Request bodies above 10 MB are rejected.
- Long-running subscriptions are kept open for up to one hour of inactivity.
- Certificates are issued by Let's Encrypt and renew automatically.

The unsupported subset of the EVM JSON-RPC surface is listed in
[EVM JSON-RPC differences](https://github.com/Sidiora-Labs/Paxeer-X-Network/blob/main/docs/evm_jsonrpc_unsupported.md) and the
unified interface is described in [Unified network](../overview/unified-network.md).
