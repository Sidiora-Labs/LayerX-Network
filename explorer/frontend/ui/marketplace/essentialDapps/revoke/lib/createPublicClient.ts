import { createPublicClient as createPublicClientDefault, http } from 'viem';
import type { PublicClient } from 'viem';

import essentialDappsChainsConfig from 'configs/essential-dapps-chains';
import { chains } from 'lib/web3/chains';

export default function createPublicClient(chainId: string | undefined): PublicClient | undefined {
  const chain = chains.find((chain) => chain.id === Number(chainId));
  const chainConfig = essentialDappsChainsConfig()?.chains.find((chain) => chain.id === chainId);

  if (!chain || !chainConfig) return undefined;

  return createPublicClientDefault({
    chain,
    transport: http(`${ chainConfig?.app_config?.apis?.general?.endpoint }/api/eth-rpc`, { batch: { wait: 100, batchSize: 5 } }),
    batch: { multicall: true },
  });
}
