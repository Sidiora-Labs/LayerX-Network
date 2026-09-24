import type * as stats from '@blockscout/stats-types';
import type { SmartContract, SmartContractMudSystemsResponse } from 'types/api/contract';
import type { HotContract, VerifiedContract, VerifiedContractsCounters } from 'types/api/contracts';

import type { SolidityScanReport } from 'lib/solidityScan/schema';

import { ADDRESS_PARAMS, ADDRESS_HASH } from './addressParams';
import { STATS_COUNTER } from './stats';

export const CONTRACT_CODE_UNVERIFIED = {
  creation_bytecode: '0x60806040526e',
  deployed_bytecode: '0x608060405233',
  creation_status: 'success',
} as SmartContract;

export const CONTRACT_CODE_VERIFIED = {
  abi: [
    {
      inputs: [],
      name: 'symbol',
      outputs: [ { internalType: 'string', name: '', type: 'string' } ],
      stateMutability: 'view',
      type: 'function',
    },
    {
      inputs: [ { internalType: 'address', name: 'newOwner', type: 'address' } ],
      name: 'transferOwnership',
      outputs: [],
      stateMutability: 'nonpayable',
      type: 'function',
    },
  ],
  additional_sources: [],
  can_be_visualized_via_sol2uml: true,
  compiler_settings: {
    compilationTarget: {
      'contracts/StubContract.sol': 'StubContract',
    },
    evmVersion: 'london',
    libraries: {},
    metadata: {
      bytecodeHash: 'ipfs',
    },
    optimizer: {
      enabled: false,
      runs: 200,
    },
    remappings: [],
  },
  compiler_version: 'v0.8.7+commit.e28d00a7',
  constructor_args: '0000000000000000000000005c7bcd6e7de5423a257d81b442095a1a6ced35c5000000000000000000000000c02aaa39b223fe8d0a0e5c4f27ead9083c756cc2',
  creation_bytecode: '0x6080604052348',
  deployed_bytecode: '0x60806040',
  evm_version: 'london',
  external_libraries: [],
  file_path: 'contracts/StubContract.sol',
  is_verified: true,
  name: 'StubContract',
  optimization_enabled: false,
  optimization_runs: 200,
  source_code: 'source_code',
  verified_at: '2023-02-21T14:39:16.906760Z',
  license_type: 'mit',
} as unknown as SmartContract;

export const VERIFIED_CONTRACT_INFO: VerifiedContract = {
  address: { ...ADDRESS_PARAMS, name: 'StubContract' },
  coin_balance: '30319033612988277',
  compiler_version: 'v0.8.17+commit.8df45f5f',
  has_constructor_args: true,
  language: 'solidity',
  market_cap: null,
  optimization_enabled: false,
  transactions_count: 565058,
  verified_at: '2023-04-10T13:16:33.884921Z',
  license_type: 'mit',
};

export const VERIFIED_CONTRACTS_COUNTERS: VerifiedContractsCounters = {
  smart_contracts: '123456789',
  new_smart_contracts_24h: '12345',
  verified_smart_contracts: '654321',
  new_verified_smart_contracts_24h: '1234',
};

export const VERIFIED_CONTRACTS_COUNTERS_MICROSERVICE: stats.ContractsPageStats = {
  total_contracts: STATS_COUNTER,
  new_contracts_24h: STATS_COUNTER,
  total_verified_contracts: STATS_COUNTER,
  new_verified_contracts_24h: STATS_COUNTER,
};

export const SOLIDITY_SCAN_REPORT: SolidityScanReport = {
  scan_report: {
    contractname: 'BullRunners',
    scan_status: 'scan_done',
    scan_summary: {
      issue_severity_distribution: {
        critical: 0,
        gas: 1,
        high: 0,
        informational: 0,
        low: 2,
        medium: 0,
      },
      score_v2: '72.22',
    },
    scanner_reference_url: 'https://solidityscan.com/quickscan/0xc1EF7811FF2ebFB74F80ed7423f2AdAA37454be2/blockscout/eth-goerli?ref=blockscout',
  },
};

export const MUD_SYSTEMS: SmartContractMudSystemsResponse = {
  items: [
    {
      name: 'sy.AccessManagement',
      address_hash: ADDRESS_HASH,
    },
  ],
};

export const HOT_CONTRACTS: HotContract = {
  contract_address: VERIFIED_CONTRACT_INFO.address,
  balance: '1000000000000000000',
  transactions_count: '1000',
  total_gas_used: '100000000',
};
