import React from 'react';

import type { SmartContractVerificationConfig } from 'types/client/contract';

import config from 'configs/app';

import ContractVerificationMethod from '../ContractVerificationMethod';
import ContractVerificationFieldAutodetectArgs from '../fields/ContractVerificationFieldAutodetectArgs';
import ContractVerificationFieldCompiler from '../fields/ContractVerificationFieldCompiler';
import ContractVerificationFieldName from '../fields/ContractVerificationFieldName';
import ContractVerificationFieldSources from '../fields/ContractVerificationFieldSources';
import ContractVerificationFieldZkCompiler from '../fields/ContractVerificationFieldZkCompiler';

const FILE_TYPES = [ '.json' as const ];
const rollupFeature = config.features.rollup;

const ContractVerificationStandardInput = ({ config }: { config: SmartContractVerificationConfig }) => {
  return (
    <ContractVerificationMethod title="Contract verification via Solidity (standard JSON input) " disableScroll={ config.verification_options.length === 1 }>
      { !config?.is_rust_verifier_microservice_enabled && <ContractVerificationFieldName/> }
      <ContractVerificationFieldCompiler config={ config }/>
      { rollupFeature.isEnabled && rollupFeature.type === 'zkSync' && <ContractVerificationFieldZkCompiler config={ config }/> }
      <ContractVerificationFieldSources
        fileTypes={ FILE_TYPES }
        title="Standard Input JSON"
        hint="Upload the standard input JSON file created during contract compilation."
        required
      />
      { !config?.is_rust_verifier_microservice_enabled && <ContractVerificationFieldAutodetectArgs/> }
    </ContractVerificationMethod>
  );
};

export default React.memo(ContractVerificationStandardInput);
