import type { SmartContractLicenseType, SmartContractVerificationConfigRaw, SmartContractVerificationMethodApi } from 'types/api/contract';

export interface ContractCodeIde {
  title: string;
  url: string;
  icon_url: string;
}

export interface ContractLicense {
  type: SmartContractLicenseType;
  url: string;
  label: string;
  title: string;
}

export const SMART_CONTRACT_EXTRA_VERIFICATION_METHODS = [
  'solidity-hardhat' as const,
  'solidity-foundry' as const,
];

export type SmartContractVerificationMethodExtra = (typeof SMART_CONTRACT_EXTRA_VERIFICATION_METHODS)[number];

export type SmartContractVerificationMethod = SmartContractVerificationMethodApi | SmartContractVerificationMethodExtra;

export interface SmartContractVerificationConfig extends SmartContractVerificationConfigRaw {
  verification_options: Array<SmartContractVerificationMethod>;
}
