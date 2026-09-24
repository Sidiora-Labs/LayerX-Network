import type * as bens from '@blockscout/bens-types';

export interface EnsAddressLookupFilters extends Pick<bens.LookupAddressRequest, 'address' | 'resolved_to' | 'owned_by' | 'only_active'> {
  protocols: Array<string> | undefined;
}

export interface EnsDomainLookupFilters extends Pick<bens.LookupDomainNameRequest, 'name' | 'only_active'> {
  protocols: Array<string> | undefined;
}

export interface EnsLookupSorting {
  sort: 'registration_date';
  order: Exclude<bens.Order, bens.Order.ORDER_UNSPECIFIED | bens.Order.UNRECOGNIZED>;
}

export type EnsDomainLookupFiltersOptions = Array<'resolved_to' | 'owned_by' | 'with_inactive'>;
