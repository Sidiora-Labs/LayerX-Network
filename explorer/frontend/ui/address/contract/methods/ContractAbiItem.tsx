import { Box } from '@chakra-ui/react';
import React from 'react';
import { Element } from 'react-scroll';

import type { FormSubmitHandler, SmartContractMethod } from './types';

import { route } from 'nextjs-routes';

import config from 'configs/app';
import { AccordionItem, AccordionItemContent, AccordionItemTrigger } from 'toolkit/chakra/accordion';
import { Alert } from 'toolkit/chakra/alert';
import { Badge } from 'toolkit/chakra/badge';
import { Hint } from 'toolkit/components/Hint/Hint';
import CopyToClipboard from 'ui/shared/CopyToClipboard';

import ContractMethodForm from './form/ContractMethodForm';
import { getElementId, getElementName } from './useScrollToMethod';
import { isReadMethod } from './utils';

interface Props {
  data: SmartContractMethod;
  index: number;
  id: number;
  addressHash: string;
  sourceAddress?: string;
  tab: string;
  onSubmit: FormSubmitHandler;
  isVisible?: boolean;
  isOpen: boolean;
}

const ContractAbiItem = ({ data, index, id, addressHash, sourceAddress, tab, onSubmit, isVisible = true, isOpen }: Props) => {
  const [ attempt, setAttempt ] = React.useState(0);

  const url = React.useMemo(() => {
    return config.app.baseUrl + route({
      pathname: '/address/[hash]',
      query: {
        hash: addressHash ?? '',
        tab,
        ...(sourceAddress ? { source_address: sourceAddress } : {}),
      },
      hash: getElementId(data),
    });
  }, [ addressHash, data, tab, sourceAddress ]);

  const handleReset = React.useCallback(() => {
    setAttempt((prev) => prev + 1);
  }, []);

  const isRead = isReadMethod(data);

  return (
    <AccordionItem
      as="section"
      value={ String(index) }
      _first={{ borderTopWidth: 0 }}
      _last={{ borderBottomWidth: 0 }}
      display={ isVisible ? 'block' : 'none' }
    >
      <Element as="h2" name={ getElementName(data) }>
        <AccordionItemTrigger
          px={ 0 }
          py={ 3 }
          _hover={{ bgColor: 'inherit' }}
          wordBreak="break-all"
          textAlign="left"
          cursor="pointer"
          display="flex"
          alignItems="center"
          columnGap={ 2 }
        >
          <CopyToClipboard text={ url } type="link" ml={ 0 } as="div"/>
          <Box fontWeight={ 500 } display="flex" alignItems="center">
            { index + 1 }. { data.type === 'fallback' || data.type === 'receive' ? data.type : data.name }
            { data.type === 'fallback' && (
              <Hint
                label={
                  `The fallback function is executed on a call to the contract if none of the other functions match
                    the given function signature, or if no data was supplied at all and there is no receive Ether function.
                    The fallback function always receives data, but in order to also receive Ether it must be marked payable.`
                }
                ml={ 1 }
                as="div"
              />
            ) }
            { data.type === 'receive' && (
              <Hint
                label={
                  `The receive function is executed on a call to the contract with empty calldata.
                    This is the function that is executed on plain Ether transfers (e.g. via .send() or .transfer()).
                    If no such function exists, but a payable fallback function exists, the fallback function will be called on a plain Ether transfer.
                    If neither a receive Ether nor a payable fallback function is present,
                    the contract cannot receive Ether through regular transactions and throws an exception.`
                }
                ml={ 1 }
                as="div"
              />
            ) }
          </Box>
          <Badge colorPalette={ isRead ? 'purple_alt' : 'blue_alt' } flexShrink={ 0 }>{ isRead ? 'read' : 'write' }</Badge>
          { 'method_id' in data && (
            <Badge
              flexShrink={ 0 }
              endElement={ <CopyToClipboard text={ data.method_id } as="div"/> }
              gap={ 0 }
            >
              { data.method_id }
            </Badge>
          ) }
        </AccordionItemTrigger>
      </Element>
      <AccordionItemContent pb={ 4 } pr={ 0 } pl="28px" w="calc(100% - 6px)">
        { 'is_invalid' in data && data.is_invalid ? (
          <Alert status="warning">An error occurred while parsing the method signature.</Alert>
        ) : (
          <ContractMethodForm
            key={ id + '_' + index + '_' + attempt }
            data={ data }
            attempt={ attempt }
            onSubmit={ onSubmit }
            onReset={ handleReset }
            isOpen={ isOpen }
          />
        ) }
      </AccordionItemContent>
    </AccordionItem>
  );
};

export default React.memo(ContractAbiItem);
