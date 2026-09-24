import NftArtifact from '@openzeppelin/contracts/build/contracts/ERC721.json';
import { useCallback } from 'react';
import { getAbiItem, getAddress, slice } from 'viem';
import type { PublicClient, GetLogsParameters, Log } from 'viem';

import type { TokenInfo } from 'types/api/token';
import type { EssentialDappsChainConfig } from 'types/client/marketplace';
import type { AllowanceType, ContractAllowanceType } from 'types/client/revoke';

import useApiFetch from 'lib/api/useApiFetch';
import { ZERO_ADDRESS } from 'toolkit/utils/consts';

import getLogs from '../lib/getLogs';
import useGetBlockTimestamp from './useGetBlockTimestamp';

async function getLimitedNftAllowancesFromApprovals(
  tokenAddress: `0x${ string }`,
  approvals: Array<Log>,
  publicClient: PublicClient,
) {
  const deduplicatedApprovals = approvals.filter(
    (approval, i) =>
      i ===
      approvals.findIndex((other) => approval.topics[2] === other.topics[2]),
  );

  const allowances: Array<ContractAllowanceType | undefined> = await Promise.all(
    deduplicatedApprovals.map((approval) =>
      getLimitedNftAllowanceFromApproval(tokenAddress, approval, publicClient),
    ),
  );

  return allowances;
}

async function getLimitedNftAllowanceFromApproval(
  tokenAddress: `0x${ string }`,
  approval: Log,
  publicClient: PublicClient,
) {
  try {
    // Some contracts (like CryptoStrikers) may not implement Nft correctly
    // by making tokenId a non-indexed parameter, in which case it needs to be
    // taken from the event data rather than topics
    const tokenId =
      approval.topics.length === 4 ? approval.topics[3] : approval.data;

    const spender = (await publicClient.readContract({
      address: tokenAddress,
      abi: NftArtifact.abi,
      functionName: 'getApproved',
      args: [ tokenId ],
    })) as `0x${ string }`;
    if (spender === ZERO_ADDRESS) return undefined;

    return {
      spender,
      transactionId: approval.transactionHash,
      allowance: BigInt(-1),
      tokenId,
      blockNumber: approval.blockNumber as bigint,
    };
  } catch {
    return undefined;
  }
}

async function getNftUnlimitedAllowancesFromApprovals(
  tokenAddress: `0x${ string }`,
  ownerAddress: string,
  approvals: Array<Log>,
  publicClient: PublicClient,
) {
  const deduplicatedApprovals = approvals.filter(
    (approval, i) =>
      i ===
      approvals.findIndex((other) => approval.topics[2] === other.topics[2]),
  );

  const allowances: Array<ContractAllowanceType | undefined> = await Promise.all(
    deduplicatedApprovals.map((approval) =>
      getNftUnlimitedAllowanceFromApproval(
        tokenAddress,
        ownerAddress,
        approval,
        publicClient,
      ),
    ),
  );

  return allowances;
}

async function getNftUnlimitedAllowanceFromApproval(
  tokenAddress: `0x${ string }`,
  ownerAddress: string,
  approval: Log,
  publicClient: PublicClient,
): Promise<ContractAllowanceType | undefined> {
  const spender = getAddress(slice(approval.topics[2] as `0x${ string }`, 12));

  const isApprovedForAll = await publicClient.readContract({
    address: tokenAddress,
    abi: NftArtifact.abi,
    functionName: 'isApprovedForAll',
    args: [ ownerAddress, spender ],
  });
  if (!isApprovedForAll) return undefined;

  return {
    spender,
    transactionId: approval.transactionHash,
    allowance: isApprovedForAll && BigInt(-1),
    blockNumber: approval.blockNumber as bigint,
  };
}

function useGetNftAllowances() {
  const apiFetch = useApiFetch();
  const getBlockTimestamp = useGetBlockTimestamp();

  return useCallback(async(
    tokenAddresses: Array<`0x${ string }`>,
    searchQuery: string,
    approvals: Array<Log>,
    approvalsForAll: Array<Log>,
    publicClient: PublicClient,
    chain: EssentialDappsChainConfig | undefined,
    signal?: AbortSignal,
  ) => {
    const allowances: Array<AllowanceType> = [];

    try {
      await Promise.all(
        tokenAddresses.map(async(tokenAddress) => {
          if (signal?.aborted) {
            throw new DOMException('Aborted', 'AbortError');
          }

          const tokenData = await apiFetch('general:token', {
            pathParams: { hash: tokenAddress },
            chain,
            fetchParams: {
              signal,
            },
          }) as TokenInfo;

          const tokenApprovals = approvals.filter(
            (approval) => getAddress(approval.address) === tokenAddress,
          );
          const tokenApprovalsForAll = approvalsForAll.filter(
            (approval) => getAddress(approval.address) === tokenAddress,
          );

          if (tokenApprovals.length > 0 || tokenApprovalsForAll.length > 0) {
            const unlimitedAllowances =
              await getNftUnlimitedAllowancesFromApprovals(
                tokenAddress,
                searchQuery,
                tokenApprovalsForAll,
                publicClient,
              );
            const limitedAllowances = await getLimitedNftAllowancesFromApprovals(
              tokenAddress,
              tokenApprovals,
              publicClient,
            );
            const allAllowances = [
              ...limitedAllowances,
              ...unlimitedAllowances,
            ].filter(
              (allowance): allowance is ContractAllowanceType =>
                allowance !== undefined,
            );

            return Promise.all(
              allAllowances.map(async(allowance) => {
                const timestampMs = await getBlockTimestamp(chain, allowance.blockNumber as bigint, signal);

                if (allowance?.allowance && allowance?.allowance !== BigInt(0)) {
                  allowances.push({
                    type: 'ERC-721',
                    allowance: allowance?.allowance === BigInt(-1) ? 'Unlimited' : allowance?.allowance.toString(),
                    address: tokenAddress,
                    name: tokenData.name || undefined,
                    symbol: tokenData.symbol || undefined,
                    tokenIcon: tokenData.icon_url || undefined,
                    tokenReputation: tokenData.reputation,
                    transactionId: allowance?.transactionId,
                    timestamp: timestampMs,
                    spender: allowance?.spender,
                    tokenId: allowance?.tokenId,
                  });
                }
              }),
            );
          }

          return undefined;
        }),
      );
    } catch {
      return [];
    }

    return allowances;
  }, [ apiFetch, getBlockTimestamp ]);
}

export default function useSearchNftAllowances() {
  const getNftAllowances = useGetNftAllowances();

  return useCallback(async(
    chain: EssentialDappsChainConfig | undefined,
    searchQuery: string,
    approvalEvents: Array<Log>,
    publicClient: PublicClient,
    latestBlockNumber: bigint,
    signal?: AbortSignal,
  ) => {
    try {
      const transferEventsPromise = getLogs(
        publicClient,
        {
          event: getAbiItem({ abi: NftArtifact.abi, name: 'Transfer' }),
          args: { to: searchQuery },
        } as unknown as GetLogsParameters,
        BigInt(0),
        latestBlockNumber,
        signal,
      );

      const approvalForAllEventsPromise = getLogs(
        publicClient,
        {
          event: getAbiItem({ abi: NftArtifact.abi, name: 'ApprovalForAll' }),
          args: { owner: searchQuery },
        } as unknown as GetLogsParameters,
        BigInt(0),
        latestBlockNumber,
        signal,
      );

      const [ transferEvents, approvalForAllEvents ] = await Promise.all([
        transferEventsPromise,
        approvalForAllEventsPromise,
      ]);

      const nftApprovalEvents = approvalEvents.filter(
        (ev) => ev.topics.length === 4,
      );
      const nftTransferEvents = transferEvents.filter(
        (ev) => ev.topics.length === 4,
      );

      const nftEvents = [
        ...nftApprovalEvents,
        ...approvalForAllEvents,
        ...nftTransferEvents,
      ];

      const nftAddresses = nftEvents
        .filter(
          (event, i) =>
            i ===
            approvalEvents.findIndex((other) => event.address === other.address),
        )
        .map((event) => getAddress(event.address));

      return getNftAllowances(
        nftAddresses,
        searchQuery,
        nftApprovalEvents,
        approvalForAllEvents,
        publicClient,
        chain,
        signal,
      );
    } catch {
      return [];
    }
  }, [ getNftAllowances ]);
}
