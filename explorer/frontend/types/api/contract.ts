import type { Abi, AbiType } from 'abitype';

import type { AddressImplementation } from './addressParams';

export type SmartContractMethodArgType = AbiType;
export type SmartContractMethodStateMutability = 'view' | 'nonpayable' | 'payable';

export type SmartContractCreationStatus = 'success' | 'failed' | 'selfdestructed';

export type SmartContractLicenseType =
'none' |
'unlicense' |
'mit' |
'gnu_gpl_v2' |
'gnu_gpl_v3' |
'gnu_lgpl_v2_1' |
'gnu_lgpl_v3' |
'bsd_2_clause' |
'bsd_3_clause' |
'mpl_2_0' |
'osl_3_0' |
'apache_2_0' |
'gnu_agpl_v3' |
'bsl_1_1';

export type SmartContractProxyType =
  'eip1167' |
  'eip1967' |
  'eip1967_oz' |
  'eip1967_beacon' |
  'eip1822' |
  'eip930' |
  'eip2535' |
  'eip7702' |
  'erc7760' |
  'master_copy' |
  'basic_implementation' |
  'basic_get_implementation' |
  'comptroller' |
  'clone_with_immutable_arguments' |
  'resolved_delegate_proxy' |
  'unknown' |
  null;

export interface SmartContractConflictingImplementation {
  proxy_type: NonNullable<SmartContractProxyType>;
  implementations: Array<AddressImplementation>;
}

export interface SmartContract {
  deployed_bytecode: string | null;
  creation_bytecode: string | null;
  creation_status: SmartContractCreationStatus | null;
  abi: Abi | null;
  compiler_version: string | null;
  evm_version: string | null;
  optimization_enabled: boolean | null;
  optimization_runs: number | string | null;
  name: string | null;
  verified_at: string | null;
  is_blueprint: boolean | null;
  is_verified: boolean | null;
  is_verified_via_eth_bytecode_db: boolean | null;
  is_changed_bytecode: boolean | null;
  conflicting_implementations: Array<SmartContractConflictingImplementation> | null;

  // sourcify info >>>
  is_verified_via_sourcify: boolean | null;
  is_fully_verified: boolean | null;
  is_partially_verified: boolean | null;
  sourcify_repo_url: string | null;
  // <<<<
  source_code: string | null;
  constructor_args: string | null;
  decoded_constructor_args: Array<SmartContractDecodedConstructorArg> | null;
  can_be_visualized_via_sol2uml: boolean | null;
  file_path: string;
  additional_sources: Array<{ file_path: string; source_code: string }>;
  external_libraries: Array<SmartContractExternalLibrary> | null;
  compiler_settings?: {
    evmVersion?: string;
    remappings?: Array<string>;
  };
  verified_twin_address_hash: string | null;
  verified_twin_filecoin_robust_address?: string | null;
  language: string | null;
  license_type: SmartContractLicenseType | null;
  certified?: boolean;
  zk_compiler_version?: string;
  github_repository_metadata?: {
    commit?: string;
    path_prefix?: string;
    repository_url?: string;
  };
  package_name?: string;
}

export type SmartContractDecodedConstructorArg = [
  unknown,
  {
    internalType: SmartContractMethodArgType;
    name: string;
    type: SmartContractMethodArgType;
  },
];

export interface SmartContractExternalLibrary {
  address_hash: string;
  name: string;
}

// VERIFICATION

export type SmartContractVerificationMethodApi = 'flattened-code' | 'standard-input' | 'sourcify' | 'multi-part' |
'vyper-code' | 'vyper-multi-part' | 'vyper-standard-input' | 'stylus-github-repository';

export interface SmartContractVerificationConfigRaw {
  solidity_compiler_versions: Array<string>;
  solidity_evm_versions: Array<string>;
  verification_options: Array<string>;
  vyper_compiler_versions: Array<string>;
  stylus_compiler_versions?: Array<string>;
  vyper_evm_versions: Array<string>;
  is_rust_verifier_microservice_enabled: boolean;
  license_types: Record<SmartContractLicenseType, number>;
  zk_compiler_versions?: Array<string>;
  zk_optimization_modes?: Array<string>;
}

export type SmartContractVerificationResponse = {
  status: 'error';
  errors: SmartContractVerificationError;
} | {
  status: 'success';
};

export interface SmartContractVerificationError {
  contract_source_code?: Array<string>;
  files?: Array<string>;
  interfaces?: Array<string>;
  compiler_version?: Array<string>;
  constructor_arguments?: Array<string>;
  name?: Array<string>;
}

type SmartContractSecurityAudit = {
  audit_company_name: string;
  audit_publish_date: string;
  audit_report_url: string;
};

export type SmartContractSecurityAudits = {
  items: Array<SmartContractSecurityAudit>;
};

export type SmartContractSecurityAuditSubmission = {
  address_hash: string;
  submitter_name: string;
  submitter_email: string;
  is_project_owner: boolean;
  project_name: string;
  project_url: string;
  audit_company_name: string;
  audit_report_url: string;
  audit_publish_date: string;
  comment?: string;
};

// MUD SYSTEM

export interface SmartContractMudSystemsResponse {
  items: Array<SmartContractMudSystemItem>;
}

export interface SmartContractMudSystemItem {
  address_hash: string;
  name: string;
}

export interface SmartContractMudSystemInfo {
  name: string;
  abi: Abi;
}
