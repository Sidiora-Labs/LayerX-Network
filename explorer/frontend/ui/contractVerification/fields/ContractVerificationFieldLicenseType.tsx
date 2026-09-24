import { createListCollection } from '@chakra-ui/react';
import React from 'react';

import type { FormFields } from '../types';

import { CONTRACT_LICENSES } from 'lib/contracts/licenses';
import type { SelectOption } from 'toolkit/chakra/select';
import { FormFieldSelect } from 'toolkit/components/forms/fields/FormFieldSelect';

import ContractVerificationFormRow from '../ContractVerificationFormRow';

const collection = createListCollection<SelectOption>({
  items: CONTRACT_LICENSES.map(({ label, title, type }) => ({ label: `${ title } (${ label })`, value: type })),
});

const ContractVerificationFieldLicenseType = () => {

  return (
    <ContractVerificationFormRow>
      <FormFieldSelect<FormFields, 'license_type'>
        name="license_type"
        placeholder="Contract license"
        collection={ collection }
      />
      <span>
        For best practices, all contract source code holders, publishers and authors are encouraged to also
        specify the accompanying license for their verified contract source code provided.
      </span>
    </ContractVerificationFormRow>
  );
};

export default React.memo(ContractVerificationFieldLicenseType);
