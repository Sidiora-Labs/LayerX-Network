import type * as multichain from '@blockscout/multichain-aggregator-types';

export default function getAvailableChainIds(data: multichain.GetAddressResponse | undefined) {
  return Object.keys(data?.chain_infos ?? {}).filter((chainId) => data?.chain_infos[chainId]);
}
