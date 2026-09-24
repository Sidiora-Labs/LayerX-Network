import React from 'react';

import type { FormFields } from '../types';

import { FormFieldCheckbox } from 'toolkit/components/forms/fields/FormFieldCheckbox';

import ContractVerificationFormRow from '../ContractVerificationFormRow';

const ContractVerificationFieldIsYul = () => {
  return (
    <ContractVerificationFormRow>
      <FormFieldCheckbox<FormFields, 'is_yul'>
        name="is_yul"
        label="Is Yul contract"
      />
    </ContractVerificationFormRow>
  );
};

export default React.memo(ContractVerificationFieldIsYul);
