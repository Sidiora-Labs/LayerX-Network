import type { ApiResource } from '../../types';
import type {
  SmartContract,
  SmartContractSecurityAudits,
  SmartContractVerificationConfigRaw,
} from 'types/api/contract';
import type { VerifiedContractsResponse, VerifiedContractsCounters, VerifiedContractsFilters } from 'types/api/contracts';
import type { VerifiedContractsSorting } from 'types/api/verifiedContracts';

export const GENERAL_API_CONTRACT_RESOURCES = {
  contract: {
    path: '/api/v2/smart-contracts/:hash',
    pathParams: [ 'hash' as const ],
  },
  contract_verification_config: {
    path: '/api/v2/smart-contracts/verification/config',
  },
  contract_verification_via: {
    path: '/api/v2/smart-contracts/:hash/verification/via/:method',
    pathParams: [ 'hash' as const, 'method' as const ],
  },
  contract_solidity_scan_report: {
    path: '/api/v2/proxy/3rdparty/solidityscan/smart-contracts/:hash/report',
    pathParams: [ 'hash' as const ],
  },
  contract_security_audits: {
    path: '/api/v2/smart-contracts/:hash/audit-reports',
    pathParams: [ 'hash' as const ],
  },
  verified_contracts: {
    path: '/api/v2/smart-contracts',
    filterFields: [ 'q' as const, 'filter' as const ],
    paginated: true,
  },
  verified_contracts_counters: {
    path: '/api/v2/smart-contracts/counters',
  },
} satisfies Record<string, ApiResource>;

export type GeneralApiContractResourceName = `general:${ keyof typeof GENERAL_API_CONTRACT_RESOURCES }`;

/* eslint-disable @stylistic/indent */
export type GeneralApiContractResourcePayload<R extends GeneralApiContractResourceName> =
R extends 'general:contract' ? SmartContract :
R extends 'general:contract_solidity_scan_report' ? unknown :
R extends 'general:verified_contracts' ? VerifiedContractsResponse :
R extends 'general:verified_contracts_counters' ? VerifiedContractsCounters :
R extends 'general:contract_verification_config' ? SmartContractVerificationConfigRaw :
R extends 'general:contract_security_audits' ? SmartContractSecurityAudits :
never;
/* eslint-enable @stylistic/indent */

/* eslint-disable @stylistic/indent */
export type GeneralApiContractPaginationFilters<R extends GeneralApiContractResourceName> =
R extends 'general:verified_contracts' ? VerifiedContractsFilters :
never;
/* eslint-enable @stylistic/indent */

/* eslint-disable @stylistic/indent */
export type GeneralApiContractPaginationSorting<R extends GeneralApiContractResourceName> =
R extends 'general:verified_contracts' ? VerifiedContractsSorting :
never;
/* eslint-enable @stylistic/indent */
