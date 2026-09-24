import {
  List,
  Box,
  createListCollection,
} from '@chakra-ui/react';
import React from 'react';

import type { FormFields } from '../types';
import type { SmartContractVerificationMethod, SmartContractVerificationConfig } from 'types/client/contract';

import { Heading } from 'toolkit/chakra/heading';
import { Link } from 'toolkit/chakra/link';
import type { SelectOption } from 'toolkit/chakra/select';
import { FormFieldSelect } from 'toolkit/components/forms/fields/FormFieldSelect';
import { Hint } from 'toolkit/components/Hint/Hint';
import { nbsp } from 'toolkit/utils/htmlEntities';

import { METHOD_LABELS } from '../utils';

interface Props {
  methods: SmartContractVerificationConfig['verification_options'];
}

const ContractVerificationFieldMethod = ({ methods }: Props) => {
  const collection = React.useMemo(() => createListCollection<SelectOption>({
    items: methods.map((method) => ({
      value: method,
      label: METHOD_LABELS[method],
    })),
  }), [ methods ]);

  const renderPopoverListItem = React.useCallback((method: SmartContractVerificationMethod) => {
    switch (method) {
      case 'flattened-code':
        return <List.Item key={ method }>Verification through a single file.</List.Item>;
      case 'multi-part':
        return <List.Item key={ method }>Verification of multi-part Solidity files.</List.Item>;
      case 'sourcify':
        return <List.Item key={ method }>Verification through <Link href="https://sourcify.dev/" external noIcon className="dark">Sourcify</Link>.</List.Item>;
      case 'standard-input':
        return (
          <List.Item key={ method }>
            <span>Verification using </span>
            <Link
              href="https://docs.soliditylang.org/en/latest/using-the-compiler.html#input-description"
              external noIcon
              className="dark"
            >
              Standard input JSON
            </Link>
            <span> file.</span>
          </List.Item>
        );
      case 'vyper-code':
        return <List.Item key={ method }>Verification of Vyper contract.</List.Item>;
      case 'vyper-multi-part':
        return <List.Item key={ method }>Verification of multi-part Vyper files.</List.Item>;
      case 'vyper-standard-input':
        return (
          <List.Item key={ method }>
            <span>Verification of Vyper contract using </span>
            <Link
              href="https://docs.vyperlang.org/en/stable/compiling-a-contract.html#compiler-input-and-output-json-description"
              external noIcon
              className="dark"
            >
              Standard input JSON
            </Link>
            <span> file.</span>
          </List.Item>
        );
      case 'solidity-hardhat':
        return <List.Item key={ method }>Verification through Hardhat plugin.</List.Item>;
      case 'solidity-foundry':
        return <List.Item key={ method }>Verification through Foundry.</List.Item>;
      case 'stylus-github-repository':
        return <List.Item key={ method }>Verification of Stylus contract via GitHub repository.</List.Item>;
    }
  }, []);

  const tooltipContent = (
    <Box>
      <span>Currently, Blockscout supports { methods.length } methods:</span>
      <List.Root as="ol" pl={ 5 }>
        { methods.map(renderPopoverListItem) }
      </List.Root>
    </Box>
  );

  return (
    <>
      <Heading level="2" mt={{ base: 10, lg: 6 }} gridColumn={{ lg: '1 / 3' }}>
        Currently, Blockscout supports { methods.length }{ nbsp }contract verification methods
        <Hint
          label={ tooltipContent }
          tooltipProps={{ interactive: true, contentProps: { textAlign: 'left' } }}
          ml={ 1 }
        />
      </Heading>
      <FormFieldSelect<FormFields, 'method'>
        name="method"
        placeholder="Verification method (compiler type)"
        collection={ collection }
        required
        readOnly={ collection.items.length === 1 }
      />
    </>
  );
};

export default React.memo(ContractVerificationFieldMethod);
