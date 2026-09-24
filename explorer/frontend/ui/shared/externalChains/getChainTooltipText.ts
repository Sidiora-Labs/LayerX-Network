import type { ExternalChain } from 'types/externalChains';

export default function getChainTooltipText(chain: Pick<ExternalChain, 'id' | 'name'> | undefined, prefix: string = '') {
  if (!chain) {
    return 'Unknown chain';
  }
  return `${ prefix }${ chain.name } (Chain ID: ${ chain.id })`;
}
