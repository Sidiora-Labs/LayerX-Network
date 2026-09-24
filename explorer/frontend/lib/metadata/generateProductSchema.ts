import type { ProductSchema, ApiData } from './types';
import type { RouteParams } from 'nextjs/types';

import type { Route } from 'nextjs-routes';

import config from 'configs/app';

/**
 * Generates Product schema (JSON-LD) for token pages
 * Returns undefined for non-token pages or when data is not available
 */
export default function generateProductSchema<Pathname extends Route['pathname']>(
  route: RouteParams<Pathname>,
  apiData: ApiData<Pathname>,
): ProductSchema | undefined {
  // Only generate for token pages
  if (route.pathname !== '/token/[hash]') {
    return undefined;
  }

  // Only generate if we have token data
  if (!apiData || typeof apiData !== 'object') {
    return undefined;
  }

  const tokenData = apiData as ApiData<'/token/[hash]'>;
  if (!tokenData) {
    return undefined;
  }

  const hash = typeof route.query?.hash === 'string' ? route.query.hash : undefined;
  if (!hash) {
    return undefined;
  }

  const baseUrl = config.app.baseUrl;
  const tokenUrl = `${ baseUrl }/token/${ hash }`;

  const schema: ProductSchema = {
    '@context': 'https://schema.org',
    '@type': 'Product',
    name: tokenData.name || tokenData.symbol || undefined,
    description: tokenData.description || undefined,
    image: tokenData.icon_url || undefined,
    url: tokenUrl,
    productID: tokenData.address_hash,
  };

  if (tokenData.projectName) {
    schema.brand = {
      '@type': 'Brand',
      name: tokenData.projectName,
    };
  }

  // Only include offers if we have a valid price
  // Schema.org requires valid price data when including an Offer
  if (tokenData.exchange_rate) {
    schema.offers = {
      '@type': 'Offer',
      price: tokenData.exchange_rate,
      priceCurrency: 'USD',
      priceValidUntil: new Date(Date.now() + 1000 * 60 * 60 * 24).toISOString(),
      availability: 'InStock',
    };
  }

  return schema;
}
