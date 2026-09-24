import multichainConfig from 'configs/multichain';

export default function getChainIdFromSlug(slug: string) {
  const config = multichainConfig();
  if (!config) {
    return undefined;
  }

  return config.chains.find((chain) => chain.slug === slug)?.id;
}
