import type * as multichain from '@blockscout/multichain-aggregator-types';

export function getName(data: { chain_infos: Record<string, multichain.GetAddressResponse_ChainInfo> }) {
  const chainInfos = Object.values(data.chain_infos ?? {});
  const isContractChains = chainInfos.filter((chainInfo) => chainInfo.is_contract);
  const names = isContractChains.map((chainInfo) => chainInfo.contract_name);

  if (names.some((name) => !name)) {
    return;
  }

  const uniqueNames = [ ...new Set(names) ];

  if (uniqueNames.length === 1) {
    return uniqueNames[0];
  }
}

export function isContract(data: { chain_infos: Record<string, multichain.GetAddressResponse_ChainInfo> } | undefined) {
  if (!data) {
    return false;
  }

  const chainInfos = Object.values(data.chain_infos ?? {});
  const isContractChains = chainInfos.filter((chainInfo) => chainInfo.is_contract);
  return isContractChains.length > chainInfos.length / 2;
}

export function isVerified(data: { chain_infos: Record<string, multichain.GetAddressResponse_ChainInfo> } | undefined) {
  if (!data) {
    return false;
  }

  const chainInfos = Object.values(data.chain_infos ?? {});
  const isContractChains = chainInfos.filter((chainInfo) => chainInfo.is_contract);
  const isVerifiedChains = chainInfos.filter((chainInfo) => chainInfo.is_verified);
  return isVerifiedChains.length > 0 && isVerifiedChains.length === isContractChains.length;
}
