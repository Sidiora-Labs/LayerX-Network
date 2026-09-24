import type { PublicClient, GetLogsParameters, Log } from 'viem';

export default async function getLogs(
  publicClient: PublicClient,
  baseFilter: GetLogsParameters,
  fromBlock: bigint,
  toBlock: bigint,
  signal?: AbortSignal,
): Promise<Array<Log>> {
  const filter = { ...baseFilter, fromBlock, toBlock } as GetLogsParameters;

  if (signal?.aborted) {
    throw new DOMException('Aborted', 'AbortError');
  }

  try {
    return (await publicClient.getLogs(filter));
  } catch {
    const middle = fromBlock + (toBlock - fromBlock) / BigInt(2);
    const leftPromise = getLogs(publicClient, baseFilter, fromBlock, middle, signal);
    const rightPromise = getLogs(publicClient, baseFilter, middle + BigInt(1), toBlock, signal);
    const [ left, right ] = await Promise.all([ leftPromise, rightPromise ]);
    return [ ...left, ...right ];
  }
};
