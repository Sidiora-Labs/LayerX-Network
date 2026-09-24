import type { ApiResource } from '../types';
import type * as bens from '@blockscout/bens-types';
import type { EnsAddressLookupFilters, EnsDomainLookupFilters, EnsLookupSorting } from 'types/api/ens';

export const BENS_API_RESOURCES = {
  addresses_lookup: {
    path: '/api/v1/addresses\\:lookup',
    filterFields: [ 'address' as const, 'resolved_to' as const, 'owned_by' as const, 'only_active' as const, 'protocols' as const ],
    paginated: true,
  },
  address_domain: {
    path: '/api/v1/addresses/:address',
    pathParams: [ 'address' as const ],
    filterFields: [ 'protocols' as const ],
  },
  domain_info: {
    path: '/api/v1/domains/:name',
    pathParams: [ 'name' as const ],
    filterFields: [ 'protocols' as const ],
  },
  domain_events: {
    path: '/api/v1/domains/:name/events',
    pathParams: [ 'name' as const ],
    filterFields: [ 'protocols' as const ],
  },
  domains_lookup: {
    path: '/api/v1/domains\\:lookup',
    filterFields: [ 'name' as const, 'only_active' as const, 'protocols' as const ],
    paginated: true,
  },
  protocols: {
    path: '/api/v1/protocols',
  },
} satisfies Record<string, ApiResource>;

export type BensApiResourceName = `bens:${ keyof typeof BENS_API_RESOURCES }`;

/* eslint-disable @stylistic/indent */
export type BensApiResourcePayload<R extends BensApiResourceName> =
R extends 'bens:addresses_lookup' ? bens.LookupAddressResponse :
R extends 'bens:address_domain' ? bens.GetAddressResponse :
R extends 'bens:domain_info' ? bens.DetailedDomain :
R extends 'bens:domain_events' ? bens.ListDomainEventsResponse :
R extends 'bens:domains_lookup' ? bens.LookupDomainNameResponse :
R extends 'bens:protocols' ? bens.GetProtocolsResponse :
never;
/* eslint-enable @stylistic/indent */

/* eslint-disable @stylistic/indent */
export type BensApiPaginationFilters<R extends BensApiResourceName> =
R extends 'bens:addresses_lookup' ? EnsAddressLookupFilters :
R extends 'bens:domains_lookup' ? EnsDomainLookupFilters :
never;
/* eslint-enable @stylistic/indent */

/* eslint-disable @stylistic/indent */
export type BensApiPaginationSorting<R extends BensApiResourceName> =
R extends 'bens:addresses_lookup' ? EnsLookupSorting :
R extends 'bens:domains_lookup' ? EnsLookupSorting :
never;
/* eslint-enable @stylistic/indent */
