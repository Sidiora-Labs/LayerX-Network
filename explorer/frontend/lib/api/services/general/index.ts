import type { ApiResource } from '../../types';

import type { GeneralApiAccountResourceName, GeneralApiAccountResourcePayload } from './account';
import { GENERAL_API_ACCOUNT_RESOURCES } from './account';
import type {
  GeneralApiAddressPaginationFilters,
  GeneralApiAddressPaginationSorting,
  GeneralApiAddressResourceName,
  GeneralApiAddressResourcePayload,
} from './address';
import { GENERAL_API_ADDRESS_RESOURCES } from './address';
import type {
  GeneralApiBlockPaginationFilters,
  GeneralApiBlockResourceName, GeneralApiBlockResourcePayload } from './block';
import { GENERAL_API_BLOCK_RESOURCES } from './block';
import { GENERAL_API_CONTRACT_RESOURCES } from './contract';
import type {
  GeneralApiContractPaginationFilters,
  GeneralApiContractPaginationSorting,
  GeneralApiContractResourceName,
  GeneralApiContractResourcePayload,
} from './contract';
import type {
  GeneralApiMiscPaginationFilters,
  GeneralApiMiscPaginationSorting,
  GeneralApiMiscResourceName,
  GeneralApiMiscResourcePayload,
} from './misc';
import { GENERAL_API_MISC_RESOURCES } from './misc';
import type {
  GeneralApiRollupPaginationFilters,
  GeneralApiRollupPaginationSorting,
  GeneralApiRollupResourceName,
  GeneralApiRollupResourcePayload,
} from './rollup';
import { GENERAL_API_ROLLUP_RESOURCES } from './rollup';
import type {
  GeneralApiTokenPaginationFilters,
  GeneralApiTokenPaginationSorting,
  GeneralApiTokenResourceName,
  GeneralApiTokenResourcePayload,
} from './token';
import { GENERAL_API_TOKEN_RESOURCES } from './token';
import type { GeneralApiTxResourceName, GeneralApiTxResourcePayload, GeneralApiTxPaginationFilters } from './tx';
import { GENERAL_API_TX_RESOURCES } from './tx';
import type { GeneralApiV1ResourceName, GeneralApiV1ResourcePayload } from './v1';
import { GENERAL_API_V1_RESOURCES } from './v1';

export const GENERAL_API_RESOURCES = {
  ...GENERAL_API_ACCOUNT_RESOURCES,
  ...GENERAL_API_ADDRESS_RESOURCES,
  ...GENERAL_API_BLOCK_RESOURCES,
  ...GENERAL_API_CONTRACT_RESOURCES,
  ...GENERAL_API_MISC_RESOURCES,
  ...GENERAL_API_ROLLUP_RESOURCES,
  ...GENERAL_API_TOKEN_RESOURCES,
  ...GENERAL_API_TX_RESOURCES,
  ...GENERAL_API_V1_RESOURCES,
} satisfies Record<string, ApiResource>;

export type GeneralApiResourceName = `general:${ keyof typeof GENERAL_API_RESOURCES }`;

/* eslint-disable @stylistic/indent */
export type GeneralApiResourcePayload<R extends GeneralApiResourceName> =
R extends GeneralApiAccountResourceName ? GeneralApiAccountResourcePayload<R> :
R extends GeneralApiAddressResourceName ? GeneralApiAddressResourcePayload<R> :
R extends GeneralApiBlockResourceName ? GeneralApiBlockResourcePayload<R> :
R extends GeneralApiContractResourceName ? GeneralApiContractResourcePayload<R> :
R extends GeneralApiMiscResourceName ? GeneralApiMiscResourcePayload<R> :
R extends GeneralApiRollupResourceName ? GeneralApiRollupResourcePayload<R> :
R extends GeneralApiTokenResourceName ? GeneralApiTokenResourcePayload<R> :
R extends GeneralApiTxResourceName ? GeneralApiTxResourcePayload<R> :
R extends GeneralApiV1ResourceName ? GeneralApiV1ResourcePayload<R> :
never;
/* eslint-enable @stylistic/indent */

/* eslint-disable @stylistic/indent */
export type GeneralApiPaginationFilters<R extends GeneralApiResourceName> =
R extends GeneralApiAddressResourceName ? GeneralApiAddressPaginationFilters<R> :
R extends GeneralApiBlockResourceName ? GeneralApiBlockPaginationFilters<R> :
R extends GeneralApiContractResourceName ? GeneralApiContractPaginationFilters<R> :
R extends GeneralApiMiscResourceName ? GeneralApiMiscPaginationFilters<R> :
R extends GeneralApiRollupResourceName ? GeneralApiRollupPaginationFilters<R> :
R extends GeneralApiTokenResourceName ? GeneralApiTokenPaginationFilters<R> :
R extends GeneralApiTxResourceName ? GeneralApiTxPaginationFilters<R> :
never;
/* eslint-enable @stylistic/indent */

/* eslint-disable @stylistic/indent */
export type GeneralApiPaginationSorting<R extends GeneralApiResourceName> =
R extends GeneralApiAddressResourceName ? GeneralApiAddressPaginationSorting<R> :
R extends GeneralApiContractResourceName ? GeneralApiContractPaginationSorting<R> :
R extends GeneralApiMiscResourceName ? GeneralApiMiscPaginationSorting<R> :
R extends GeneralApiRollupResourceName ? GeneralApiRollupPaginationSorting<R> :
R extends GeneralApiTokenResourceName ? GeneralApiTokenPaginationSorting<R> :
never;
/* eslint-enable @stylistic/indent */
