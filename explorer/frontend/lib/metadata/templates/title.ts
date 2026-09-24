import { getFeaturePayload } from 'configs/app/features/types';

import type { Route } from 'nextjs-routes';

import config from 'configs/app';
import { layerLabels } from 'lib/rollups/utils';

const dappEntityName = (getFeaturePayload(config.features.marketplace)?.titles.entity_name ?? '').toLowerCase();

const TEMPLATE_MAP: Record<Route['pathname'], string> = {
  '/': '%network_name% blockchain explorer - View %network_name% stats',
  '/txs': '%network_name% transactions - %network_name% explorer',
  '/internal-txs': '%network_name% internal transactions - %network_name% explorer',
  '/txs/kettle/[hash]': '%network_name% kettle %hash% transactions',
  '/tx/[hash]': '%network_name% transaction %hash%',
  '/blocks': '%network_name% blocks',
  '/block/[height_or_hash]': '%network_name% block %height_or_hash%',
  '/block/countdown': '%network_name% block countdown index',
  '/block/countdown/[height]': '%network_name% block %height% countdown',
  '/accounts': '%network_name% top accounts',
  '/accounts/label/[slug]': '%network_name% addresses search by label',
  '/address/[hash]': '%network_name% address details for %hash%',
  '/verified-contracts': 'Verified %network_name% contracts lookup - %network_name% explorer',
  '/contract-verification': '%network_name% verify contract',
  '/address/[hash]/contract-verification': '%network_name% contract verification for %hash%',
  '/tokens': 'Tokens list - %network_name% explorer',
  '/token/[hash]': '%network_name% token details',
  '/token/[hash]/instance/[id]': '%network_name% NFT instance',
  '/apps': `%network_name% ${ dappEntityName }s - Explore top ${ dappEntityName }s`,
  '/apps/[id]': `%network_name% marketplace ${ dappEntityName }`,
  '/essential-dapps/[id]': '%id_cap%',
  '/stats': '%network_name% stats - %network_name% network insights',
  '/stats/[id]': '%network_name% stats - %id% chart',
  '/uptime': '%network_name% uptime',
  '/hot-contracts': '%network_name% hot contracts',
  '/api-docs': '%network_name% API docs - %network_name% developer tools',
  '/search-results': '%network_name% search result for %q%',
  '/auth/profile': '%network_name% - my profile',
  '/account/merits': '%network_name% - Merits',
  '/account/watchlist': '%network_name% - watchlist',
  '/account/api-key': '%network_name% - API keys',
  '/account/custom-abi': '%network_name% - custom ABI',
  '/account/tag-address': '%network_name% - private tags',
  '/account/verified-addresses': '%network_name% - my verified addresses',
  '/public-tags/submit': '%network_name% - public tag requests',
  '/withdrawals': '%network_name% withdrawals - track on %network_name% explorer',
  '/txn-withdrawals': `${ layerLabels.current } to ${ layerLabels.parent } message relayer`,
  '/visualize/sol2uml': '%network_name% Solidity UML diagram',
  '/csv-export': '%network_name% export data to CSV',
  '/deposits': '%network_name% deposits - track on %network_name% explorer',
  '/output-roots': '%network_name% output roots',
  '/dispute-games': '%network_name% dispute games',
  '/batches': '%network_name% txn batches',
  '/batches/[number]': `%network_name% ${ layerLabels.current } txn batch %number%`,
  '/batches/celestia/[height]/[commitment]': `%network_name% ${ layerLabels.current } txn batch %height% %commitment%`,
  '/blobs/[hash]': '%network_name% blob %hash% details',
  '/ops': 'User operations on %network_name% - %network_name% explorer',
  '/op/[hash]': '%network_name% user operation %hash%',
  '/404': '%network_name% error - page not found',
  '/name-services': '%network_name% name services - %network_name% explorer',
  '/name-services/domains/[name]': '%network_name% %name% domain details',
  '/name-services/clusters/[name]': '%network_name% %name% cluster details',
  '/validators': '%network_name% validators list',
  '/validators/[id]': '%network_name% validator %id% details',
  '/epochs': '%network_name% epochs',
  '/epochs/[number]': '%network_name% epoch %number% details',
  '/gas-tracker': 'Track %network_name% gas fees in %network_gwei%',
  '/mud-worlds': '%network_name% MUD worlds list',
  '/token-transfers': '%network_name% token transfers',
  '/advanced-filter': '%network_name% advanced filter',
  '/pools': '%network_name% DEX pools',
  '/pools/[hash]': '%network_name% pool details',
  '/interop-messages': '%network_name% interop messages',
  '/operations': '%network_name% operations',
  '/operation/[id]': '%network_name% operation %id%',
  '/cc/tx/[hash]': '%network_name% cross-chain transaction %hash% details',
  '/cross-chain-tx/[id]': '%network_name% cross-chain transaction %id% details',

  // multichain routes
  '/chain/[chain_slug]/accounts/label/[slug]': '%network_name% addresses search by label',
  '/chain/[chain_slug]/advanced-filter': '%network_name% advanced filter',
  '/chain/[chain_slug]/block/[height_or_hash]': '%network_name% block %height_or_hash% details',
  '/chain/[chain_slug]/block/countdown': '%network_name% block countdown index',
  '/chain/[chain_slug]/block/countdown/[height]': '%network_name% block %height% countdown',
  '/chain/[chain_slug]/csv-export': '%network_name% export data to CSV',
  '/chain/[chain_slug]/op/[hash]': '%network_name% user operation %hash% details',
  '/chain/[chain_slug]/token/[hash]': '%network_name% token details',
  '/chain/[chain_slug]/token/[hash]/instance/[id]': '%network_name% token NFT instance',
  '/chain/[chain_slug]/tx/[hash]': '%network_name% transaction %hash% details',
  '/chain/[chain_slug]/visualize/sol2uml': '%network_name% Solidity UML diagram',
  '/ecosystems': '%network_name% ecosystems',

  // service routes, added only to make typescript happy
  '/login': '%network_name% login',
  '/sprite': '%network_name% SVG sprite',
  '/chakra': '%network_name% Chakra UI showcase',
  '/api/metrics': '%network_name% node API prometheus metrics',
  '/api/monitoring/invalid-api-schema': '%network_name% node API prometheus metrics',
  '/api/log': '%network_name% node API request log',
  '/api/tokens/[hash]/instances/[id]/media-type': '%network_name% node API token instance media type',
  '/api/proxy': '%network_name% node API proxy',
  '/api/csrf': '%network_name% node API CSRF token',
  '/api/healthz': '%network_name% node API health check',
  '/api/config': '%network_name% node API app config',
};

const TEMPLATE_MAP_ENHANCED: Partial<Record<Route['pathname'], string>> = {
  '/token/[hash]': '%network_name% %symbol_or_name% token details',
  '/token/[hash]/instance/[id]': '%network_name% token instance for %symbol_or_name%',
  '/apps/[id]': '%network_name% - %app_name%',
  '/address/[hash]': '%network_name% address details for %domain_name%',
  '/stats/[id]': '%title% chart on %network_name%',
};

export function make(pathname: Route['pathname'], isEnriched = false) {
  const template = (isEnriched ? TEMPLATE_MAP_ENHANCED[pathname] : undefined) ?? TEMPLATE_MAP[pathname];
  const postfix = config.meta.promoteBlockscoutInTitle ? ' | Blockscout' : '';

  return (template + postfix).trim();
}
