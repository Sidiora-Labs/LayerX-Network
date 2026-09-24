import { Box, createListCollection } from '@chakra-ui/react';
import React from 'react';

import type { FormFields } from '../types';
import type { SmartContractVerificationConfig } from 'types/client/contract';

import { Link } from 'toolkit/chakra/link';
import { FormFieldSelectAsync } from 'toolkit/components/forms/fields/FormFieldSelectAsync';

import ContractVerificationFormRow from '../ContractVerificationFormRow';

const OPTIONS_LIMIT = 50;

const ContractVerificationFieldZkCompiler = ({ config }: { config: SmartContractVerificationConfig }) => {
  const versions = React.useMemo(() => (
    config?.zk_compiler_versions || []
  ), [ config?.zk_compiler_versions ]);

  const loadOptions = React.useCallback(async(inputValue: string, currentValue: Array<string>) => {
    const items = versions
      ?.filter((value) => !inputValue || currentValue.includes(value) || value.toLowerCase().includes(inputValue.toLowerCase()))
      .sort((a, b) => {
        if (currentValue.includes(a)) {
          return -1;
        }
        if (currentValue.includes(b)) {
          return 1;
        }
        return 0;
      })
      .slice(0, OPTIONS_LIMIT)
      .map((value) => ({ label: value, value })) ?? [];

    return createListCollection({ items });
  }, [ versions ]);

  return (
    <ContractVerificationFormRow>
      <FormFieldSelectAsync<FormFields, 'zk_compiler'>
        name="zk_compiler"
        placeholder="ZK compiler"
        loadOptions={ loadOptions }
        required
      />
      <Box>
        <Link external href="https://docs.zksync.io/zk-stack/components/compiler/specification#glossary">zksolc</Link>
        <span> compiler version.</span>
      </Box>
    </ContractVerificationFormRow>
  );
};

export default React.memo(ContractVerificationFieldZkCompiler);
