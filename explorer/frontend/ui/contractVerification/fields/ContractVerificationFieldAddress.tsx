import React from 'react';

import type { FormFields } from '../types';

import { Heading } from 'toolkit/chakra/heading';
import { FormFieldAddress } from 'toolkit/components/forms/fields/FormFieldAddress';

import ContractVerificationFormRow from '../ContractVerificationFormRow';

interface Props {
  readOnly?: boolean;
}

const ContractVerificationFieldAddress = ({ readOnly }: Props) => {
  return (
    <>
      <ContractVerificationFormRow>
        <Heading level="2">
          Contract address to verify
        </Heading>
      </ContractVerificationFormRow>
      <ContractVerificationFormRow>
        <FormFieldAddress<FormFields>
          name="address"
          required
          placeholder="Smart contract / Address (0x...)"
          readOnly={ readOnly }
        />
      </ContractVerificationFormRow>
    </>
  );
};

export default React.memo(ContractVerificationFieldAddress);
