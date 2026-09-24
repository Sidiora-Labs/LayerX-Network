import type { SmartContractLicenseType } from 'types/api/contract';
import type { SmartContractVerificationMethod } from 'types/client/contract';

import type { SelectOption } from 'toolkit/chakra/select';

export interface ContractLibrary {
  name: string;
  address: string;
}

export interface LicenseOption {
  label: string;
  value: SmartContractLicenseType;
}

interface FormFieldsBase {
  address: string;
  method: Array<SmartContractVerificationMethod>;
  license_type: Array<SmartContractLicenseType>;
}

export interface FormFieldsFlattenSourceCode extends FormFieldsBase {
  is_yul: boolean;
  name: string | undefined;
  compiler: Array<string>;
  evm_version: Array<string>;
  is_optimization_enabled: boolean;
  optimization_runs: string;
  code: string;
  autodetect_constructor_args: boolean;
  constructor_args: string;
  libraries: Array<ContractLibrary>;
}

export interface FormFieldsStandardInput extends FormFieldsBase {
  name: string;
  compiler: Array<string>;
  sources: Array<File>;
  autodetect_constructor_args: boolean;
  constructor_args: string;
}

export interface FormFieldsStandardInputZk extends FormFieldsBase {
  name: string;
  compiler: Array<string>;
  zk_compiler: Array<string>;
  sources: Array<File>;
  autodetect_constructor_args: boolean;
  constructor_args: string;
}

export interface FormFieldsSourcify extends FormFieldsBase {
  sources: Array<File>;
  contract_index?: SelectOption;
}

export interface FormFieldsMultiPartFile extends FormFieldsBase {
  compiler: Array<string>;
  evm_version: Array<string>;
  is_optimization_enabled: boolean;
  optimization_runs: string;
  sources: Array<File>;
  libraries: Array<ContractLibrary>;
}

export interface FormFieldsVyperContract extends FormFieldsBase {
  name: string;
  evm_version: Array<string>;
  compiler: Array<string>;
  code: string;
  constructor_args: string | undefined;
}

export interface FormFieldsVyperMultiPartFile extends FormFieldsBase {
  compiler: Array<string>;
  evm_version: Array<string>;
  sources: Array<File>;
  interfaces: Array<File>;
}

export interface FormFieldsVyperStandardInput extends FormFieldsBase {
  compiler: Array<string>;
  sources: Array<File>;
}

export interface FormFieldsStylusGitHubRepo extends FormFieldsBase {
  compiler: Array<string>;
  repository_url: string;
  commit_hash: string;
  path_prefix: string;
}

export type FormFields = FormFieldsFlattenSourceCode | FormFieldsStandardInput | FormFieldsStandardInputZk | FormFieldsSourcify |
FormFieldsMultiPartFile | FormFieldsVyperContract | FormFieldsVyperMultiPartFile | FormFieldsVyperStandardInput | FormFieldsStylusGitHubRepo;
