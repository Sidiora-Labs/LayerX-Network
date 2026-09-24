import { chakra, Code } from '@chakra-ui/react';
import React from 'react';

import type { FormFields } from '../types';

import { FormFieldText } from 'toolkit/components/forms/fields/FormFieldText';

import ContractVerificationFormRow from '../ContractVerificationFormRow';

interface Props {
  hint?: string;
}

const ContractVerificationFieldName = ({ hint }: Props) => {
  return (
    <ContractVerificationFormRow>
      <FormFieldText<FormFields>
        name="name"
        required
        placeholder="Contract name"
        rules={{ maxLength: 255 }}
      />
      { hint ? <span>{ hint }</span> : (
        <>
          <span>Must match the name specified in the code. For example, in </span>
          <Code color="text.secondary">{ `contract MyContract {..}` }</Code>
          <span>. <chakra.span fontWeight={ 600 }>MyContract</chakra.span> is the contract name.</span>
        </>
      ) }
    </ContractVerificationFormRow>
  );
};

export default React.memo(ContractVerificationFieldName);
