import React from 'react';

import type { SmartContractVerificationConfig } from 'types/client/contract';

import ContractVerificationMethod from '../ContractVerificationMethod';
import ContractVerificationFieldCode from '../fields/ContractVerificationFieldCode';
import ContractVerificationFieldCompiler from '../fields/ContractVerificationFieldCompiler';
import ContractVerificationFieldConstructorArgs from '../fields/ContractVerificationFieldConstructorArgs';
import ContractVerificationFieldEvmVersion from '../fields/ContractVerificationFieldEvmVersion';
import ContractVerificationFieldName from '../fields/ContractVerificationFieldName';

const ContractVerificationVyperContract = ({ config }: { config: SmartContractVerificationConfig }) => {
  return (
    <ContractVerificationMethod title="Contract verification via Vyper (contract)">
      <ContractVerificationFieldName hint="The contract name is the name assigned to the verified contract in Blockscout."/>
      <ContractVerificationFieldCompiler config={ config } isVyper/>
      { config?.is_rust_verifier_microservice_enabled && <ContractVerificationFieldEvmVersion isVyper config={ config }/> }
      <ContractVerificationFieldCode isVyper/>
      { !config?.is_rust_verifier_microservice_enabled && <ContractVerificationFieldConstructorArgs/> }
    </ContractVerificationMethod>
  );
};

export default React.memo(ContractVerificationVyperContract);
