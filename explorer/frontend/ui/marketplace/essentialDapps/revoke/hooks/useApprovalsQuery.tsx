import ERC20Artifact from '@openzeppelin/contracts/build/contracts/ERC20.json';
import { useQuery } from '@tanstack/react-query';
import { useCallback, useMemo } from 'react';
import type { GetLogsParameters } from 'viem';
import { isAddress, getAbiItem } from 'viem';

import type { EssentialDappsChainConfig } from 'types/client/marketplace';

import { ALLOWANCES } from 'stubs/essentialDapps/revoke';

import createPublicClient from '../lib/createPublicClient';
import getLogs from '../lib/getLogs';
import useSearchErc20Allowances from './useSearchErc20Allowances';
import useSearchNftAllowances from './useSearchNftAllowances';

export default function useApprovalsQuery(chain: EssentialDappsChainConfig | undefined, userAddress: string) {
  const searchErc20Allowances = useSearchErc20Allowances();
  const searchNftAllowances = useSearchNftAllowances();

  const publicClient = useMemo(
    () => createPublicClient(chain?.id),
    [ chain?.id ],
  );

  const searchAllowances = useCallback(async(signal?: AbortSignal) => {
    try {
      if (signal?.aborted) {
        throw new DOMException('Aborted', 'AbortError');
      }
      if (!publicClient) {
        throw new Error('Public client not found');
      }

      const latestBlockNumber = await publicClient.getBlockNumber();

      const filter = {
        event: getAbiItem({ abi: ERC20Artifact.abi, name: 'Approval' }),
        args: { owner: userAddress },
      } as unknown as GetLogsParameters;
      const approvalEvents = await getLogs(publicClient, filter, BigInt(0), latestBlockNumber, signal);

      const [ erc20Allowances, nftAllowances ] = await Promise.all([
        searchErc20Allowances(chain, userAddress, approvalEvents, publicClient, signal),
        searchNftAllowances(chain, userAddress, approvalEvents, publicClient, latestBlockNumber, signal),
      ]);

      const allowances = [ ...erc20Allowances, ...nftAllowances ].sort((a, b) => {
        if (b.timestamp < a.timestamp) return -1;
        if (b.timestamp > a.timestamp) return 1;
        return 0;
      });

      return allowances;
    } catch {
      return [];
    }
  }, [ searchErc20Allowances, searchNftAllowances, publicClient, chain, userAddress ]);

  return useQuery({
    queryKey: [ 'revoke:approvals', chain?.id, userAddress ],
    queryFn: ({ signal }) => searchAllowances(signal),
    enabled: Boolean(userAddress) && isAddress(userAddress),
    placeholderData: ALLOWANCES,
  });
}
