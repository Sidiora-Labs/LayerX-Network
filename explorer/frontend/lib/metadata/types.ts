import type { LineChart } from '@blockscout/stats-types';
import type { TokenInfo } from 'types/api/token';

import type { Route } from 'nextjs-routes';

/* eslint-disable @stylistic/indent */
export type ApiData<Pathname extends Route['pathname']> =
(
    Pathname extends '/address/[hash]' ? { domain_name: string } :
    Pathname extends '/token/[hash]' ? TokenInfo & { symbol_or_name: string; description?: string; projectName?: string } :
    Pathname extends '/token/[hash]/instance/[id]' ? { symbol_or_name: string } :
    Pathname extends '/apps/[id]' ? { app_name: string } :
    Pathname extends '/stats/[id]' ? LineChart['info'] :
    never
) | null;

export interface ProductSchema {
    '@context': string;
    '@type': 'Product';
    name?: string;
    description?: string;
    image?: string;
    url?: string;
    productID?: string;
    offers?: {
        '@type': 'Offer';
        price?: string;
        priceCurrency?: string;
        priceValidUntil?: string;
        availability?: string;
    };
    brand?: {
        '@type': 'Brand';
        name?: string;
    };
}

export interface Metadata {
    title: string;
    description: string;
    opengraph: {
        title: string;
        description?: string;
        imageUrl?: string;
    };
    canonical: string | undefined;
    jsonLd?: ProductSchema;
}
