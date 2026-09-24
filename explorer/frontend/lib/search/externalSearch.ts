import config from 'configs/app';

const zetaChainFeature = config.features.zetachain;

export type ExternalSearchItem = {
  url: string;
  name: string;
} | null;

export function getExternalSearchItem(hash: string): ExternalSearchItem {
  if (!zetaChainFeature.isEnabled) {
    return null;
  }

  const externalSearchConfig = zetaChainFeature.externalSearchConfig;
  const externalSearchConfigItem = externalSearchConfig.find((item) => {
    try {
      return new RegExp(item.regex).test(hash);
    } catch {
      return false;
    }
  });

  if (externalSearchConfigItem) {
    return {
      url: externalSearchConfigItem.template.replace('{hash}', hash),
      name: externalSearchConfigItem.name,
    };
  }

  return null;
}
