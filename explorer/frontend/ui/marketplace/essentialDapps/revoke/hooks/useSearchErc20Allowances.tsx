import ERC20Artifact from '@openzeppelin/contracts/build/contracts/ERC20.json';
import { uniq } from 'es-toolkit';
import { useCallback } from 'react';
import { getAddress, formatUnits, slice } from 'viem';
import type { PublicClient, Log } from 'viem';

import type { AddressTokenBalancesResponse } from 'types/api/address';
import type { TokenInfo } from 'types/api/token';
import type { EssentialDappsChainConfig } from 'types/client/marketplace';
import type { AllowanceType, ContractAllowanceType } from 'types/client/revoke';

import useApiFetch from 'lib/api/useApiFetch';

import useGetBlockTimestamp from './useGetBlockTimestamp';

function formatAllowance(
  allowance: bigint,
  decimals: number = 18,
  totalSupply: bigint | undefined,
): string {
  if (totalSupply && allowance > totalSupply) {
    return 'Unlimited';
  }

  return formatUnits(allowance, decimals);
}

async function getERC20AllowanceFromApproval(
  tokenAddress: `0x${ string }`,
  ownerAddress: string,
  approval: Log,
  publicClient: PublicClient,
): Promise<ContractAllowanceType> {
  const spender = getAddress(slice(approval.topics[2] as `0x${ string }`, 12));
  const allowance = await publicClient
    .readContract({
      address: tokenAddress,
      abi: ERC20Artifact.abi,
      functionName: 'allowance',
      args: [ ownerAddress, spender ],
    })
    .catch(() => undefined);

  return {
    transactionId: approval.transactionHash,
    spender,
    allowance: allowance as bigint | undefined,
    blockNumber: approval.blockNumber as bigint,
  };
}

async function getERC20AllowancesFromApprovals(
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

  const allowances: Array<ContractAllowanceType> = await Promise.all(
    deduplicatedApprovals.map((approval) =>
      getERC20AllowanceFromApproval(
        tokenAddress,
        ownerAddress,
        approval,
        publicClient,
      ),
    ),
  );

  return allowances;
}

const useGetERC20TokenData = () => {
  const apiFetch = useApiFetch();

  return useCallback(async(
    tokenAddress: `0x${ string }`,
    chain: EssentialDappsChainConfig | undefined,
    signal?: AbortSignal,
  ) => {
    try {
      const data = await apiFetch('general:token', {
        pathParams: { hash: tokenAddress },
        chain,
        fetchParams: {
          signal,
        },
      }) as TokenInfo;

      return {
        symbol: data.symbol || undefined,
        decimals: data.decimals ? Number(data.decimals) : undefined,
        totalSupply: data.total_supply ? BigInt(data.total_supply) : undefined,
        name: data.name || undefined,
        tokenIcon: data.icon_url || undefined,
        price: data.exchange_rate || undefined,
        tokenReputation: data.reputation,
        balance: undefined,
      };
    } catch {
      return undefined;
    }
  }, [ apiFetch ]);
};

const useGetERC20Allowances = () => {
  const getERC20TokenData = useGetERC20TokenData();
  const getBlockTimestamp = useGetBlockTimestamp();
  const apiFetch = useApiFetch();

  return useCallback(async(
    chain: EssentialDappsChainConfig | undefined,
    tokenAddresses: Array<`0x${ string }`>,
    searchQuery: string,
    approvals: Array<Log>,
    publicClient: PublicClient,
    signal?: AbortSignal,
  ) => {
    const allowances: Array<AllowanceType> = [];
    let balances: Record<string, bigint> = {};

    const response = await apiFetch('general:address_token_balances', {
      pathParams: { hash: searchQuery },
      chain,
      fetchParams: {
        signal,
      },
    }) as AddressTokenBalancesResponse;

    balances = Object.fromEntries(
      response.map((entry) => [ entry.token.address_hash, BigInt(entry.value) ]),
    );

    await Promise.all(
      tokenAddresses.map(async(tokenAddress) => {
        if (signal?.aborted) {
          throw new DOMException('Aborted', 'AbortError');
        }
        const tokenData = await getERC20TokenData(tokenAddress, chain, signal);

        if (tokenData) {
          const tokenApprovals = approvals.filter(
            (approval) => getAddress(approval.address) === tokenAddress,
          );

          if (tokenApprovals.length > 0) {
            const tokenAllowances = await getERC20AllowancesFromApprovals(
              tokenAddress,
              searchQuery,
              tokenApprovals,
              publicClient,
            );

            await Promise.all(
              tokenAllowances.map(async(allowance) => {
                if (signal?.aborted) {
                  throw new DOMException('Aborted', 'AbortError');
                }
                const timestampMs = await getBlockTimestamp(chain, allowance.blockNumber, signal);

                let valueAtRiskUsd;

                if (
                  allowance.allowance &&
                  balances[tokenAddress] &&
                  tokenData.price &&
                  tokenData.decimals
                ) {
                  const valueAtRisk = allowance.allowance > balances[tokenAddress] ?
                    balances[tokenAddress] :
                    allowance.allowance;

                  valueAtRiskUsd = Number(
                    (
                      parseFloat(formatUnits(valueAtRisk, tokenData.decimals)) *
                      parseFloat(tokenData.price)
                    ).toFixed(2),
                  );
                }

                if (allowance.allowance && allowance.allowance !== BigInt(0)) {
                  allowances.push({
                    ...tokenData,
                    type: 'ERC-20',
                    address: tokenAddress,
                    transactionId: allowance.transactionId,
                    spender: allowance.spender,
                    timestamp: timestampMs,
                    allowance: allowance.allowance ?
                      formatAllowance(
                        allowance.allowance,
                        tokenData.decimals,
                        tokenData.totalSupply,
                      ) : undefined,
                    valueAtRiskUsd,
                  });
                }
              }),
            );
          }
        }
      }),
    );

    return allowances;
  }, [ getERC20TokenData, apiFetch, getBlockTimestamp ]);
};

export default function useSearchErc20Allowances() {
  const getERC20Allowances = useGetERC20Allowances();

  return useCallback(async(
    chain: EssentialDappsChainConfig | undefined,
    searchQuery: string,
    approvalEvents: Array<Log>,
    publicClient: PublicClient,
    signal?: AbortSignal,
  ) => {
    const erc20Events = approvalEvents.filter((ev) => ev.topics.length === 3);

    const erc20Addresses = uniq(
      erc20Events.map((event) => getAddress(event.address)),
    );

    return getERC20Allowances(
      chain,
      erc20Addresses,
      searchQuery,
      erc20Events,
      publicClient,
      signal,
    );
  }, [ getERC20Allowances ]);
}
