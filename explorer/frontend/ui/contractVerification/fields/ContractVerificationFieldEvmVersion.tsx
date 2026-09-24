import { createListCollection } from '@chakra-ui/react';
import React from 'react';

import type { FormFields } from '../types';
import type { SmartContractVerificationConfig } from 'types/client/contract';

import { Link } from 'toolkit/chakra/link';
import { FormFieldSelect } from 'toolkit/components/forms/fields/FormFieldSelect';

import ContractVerificationFormRow from '../ContractVerificationFormRow';

interface Props {
  isVyper?: boolean;
  config: SmartContractVerificationConfig;
}

const ContractVerificationFieldEvmVersion = ({ isVyper, config }: Props) => {
  const collection = React.useMemo(() => {
    const items = (isVyper ? config?.vyper_evm_versions : config?.solidity_evm_versions)?.map((option) => ({ label: option, value: option })) || [];

    return createListCollection({ items });
  }, [ config?.solidity_evm_versions, config?.vyper_evm_versions, isVyper ]);

  return (
    <ContractVerificationFormRow>
      <FormFieldSelect<FormFields, 'evm_version'>
        name="evm_version"
        placeholder="EVM Version"
        collection={ collection }
        required
      />
      <>
        <span>The EVM version the contract is written for. If the bytecode does not match the version, we try to verify using the latest EVM version. </span>
        <Link
          href={ isVyper ?
            'https://docs.vyperlang.org/en/stable/compiling-a-contract.html#target-options' :
            'https://docs.soliditylang.org/en/latest/using-the-compiler.html#target-options'
          }
          external
          noIcon
        >
          EVM version details
        </Link>
      </>
    </ContractVerificationFormRow>
  );
};

export default React.memo(ContractVerificationFieldEvmVersion);
