import { Flex, VStack } from '@chakra-ui/react';
import { isEqual } from 'es-toolkit';
import type { ChangeEvent } from 'react';
import React from 'react';

import type { ZetaChainCCTXFilterParams } from 'types/client/zetaChain';

import { Input } from 'toolkit/chakra/input';
import { InputGroup } from 'toolkit/chakra/input-group';
import AddButton from 'toolkit/components/buttons/AddButton';
import { ClearButton } from 'toolkit/components/buttons/ClearButton';
import type { Props as ChainSelectBaseProps } from 'ui/shared/externalChains/ChainSelect';
import ChainSelectBase from 'ui/shared/externalChains/ChainSelect';
import TableColumnFilter from 'ui/shared/filters/TableColumnFilter';
import useZetaChainConfig from 'ui/zetaChain/useZetaChainConfig';

type Props = {
  value?: Array<string>;
  chainValue?: Array<string>;
  handleFilterChange: (field: keyof ZetaChainCCTXFilterParams, value?: Array<string> | Array<string>) => void;
  columnName: string;
  isLoading?: boolean;
  onClose?: () => void;
  filterParam: keyof ZetaChainCCTXFilterParams;
  chainFilterParam: keyof ZetaChainCCTXFilterParams;
  title: string;
  placeholder: string;
};

type InputProps = {
  address?: string;
  isLast: boolean;
  onChange: (event: ChangeEvent<HTMLInputElement>) => void;
  onClear: () => void;
  onAddFieldClick: () => void;
  placeholder: string;
};

const AddressFilterInput = ({ address, onChange, onClear, isLast, onAddFieldClick, placeholder }: InputProps) => {
  return (
    <Flex alignItems="center" w="100%">
      <InputGroup
        flexGrow={ 1 }
        endElement={ <ClearButton onClick={ onClear } mx={ 2 } disabled={ !address }/> }
      >
        <Input value={ address } onChange={ onChange } placeholder={ placeholder } size="sm" autoComplete="off"/>
      </InputGroup>
      { isLast && (
        <AddButton
          ml={ 2 }
          onClick={ onAddFieldClick }
        />
      ) }
    </Flex>
  );
};

interface ChainSelectProps extends ChainSelectBaseProps {}

const ChainSelect = ({ chainsConfig, value, onValueChange, ...props }: ChainSelectProps) => {
  const formattedValue = React.useMemo(() => {
    if (!value || value.length === 0) {
      return [ 'all' ];
    }

    return value;
  }, [ value ]);

  const handleValueChange = React.useCallback(({ value }: { value: Array<string> }) => {
    const chainIds = value.filter(item => item !== 'all');
    onValueChange?.({ value: chainIds, items: chainsConfig.filter(chain => chainIds.includes(chain.id)) });
  }, [ chainsConfig, onValueChange ]);
  return (
    <ChainSelectBase
      { ...props }
      chainsConfig={ chainsConfig }
      withAllOption
      mode="default"
      value={ formattedValue }
      onValueChange={ handleValueChange }
      size="sm"
      w="full"
      positioning={{
        placement: 'bottom-start',
        sameWidth: true,
        offset: { mainAxis: 4 },
      }}
      contentProps={{
        zIndex: 'modal2',
      }}
    />
  );
};

const emptyItem = { address: '' };

const ZetaChainAddressFilter = ({
  value = [],
  chainValue = [],
  handleFilterChange,
  onClose,
  filterParam,
  chainFilterParam,
  title,
  placeholder,
}: Props) => {
  const [ currentValue, setCurrentValue ] = React.useState<Array<{ address: string }>>([ ...value.map(addr => ({ address: addr })), emptyItem ]);
  const [ currentChainValue, setCurrentChainValue ] = React.useState<Array<string>>(chainValue.map(String));

  const { data: chains = [], isPending: isChainsLoading } = useZetaChainConfig();

  const handleAddressChange = React.useCallback((index: number) => (event: ChangeEvent<HTMLInputElement>) => {
    setCurrentValue(prev => {
      const newValue = [ ...prev ];
      newValue[index] = { address: event.target.value };
      return newValue;
    });
  }, []);

  const handleAddressClear = React.useCallback((index: number) => () => {
    setCurrentValue(prev => {
      const newValue = [ ...prev ];
      newValue[index] = { address: '' };
      return newValue;
    });
  }, []);

  const handleAddField = React.useCallback(() => {
    setCurrentValue(prev => [ ...prev, { address: '' } ]);
  }, []);

  const handleChainChange = React.useCallback(({ value }: { value: Array<string> }) => {
    setCurrentChainValue(value);
  }, []);

  const onReset = React.useCallback(() => {
    setCurrentValue([ emptyItem ]);
    setCurrentChainValue([]);
  }, []);

  const onFilter = React.useCallback(() => {
    const addresses = currentValue
      .map(item => item.address.trim())
      .filter(address => address.length > 0);

    const chainIds = currentChainValue.length > 0 ? currentChainValue : undefined;

    handleFilterChange(filterParam, addresses.length > 0 ? addresses : undefined);
    handleFilterChange(chainFilterParam, chainIds);
    onClose?.();
  }, [ handleFilterChange, currentValue, currentChainValue, onClose, filterParam, chainFilterParam ]);

  const isFilled = Boolean(
    currentValue.some(item => item.address.trim().length > 0) ||
    currentChainValue.length > 0,
  );

  const isTouched = !isEqual(
    currentValue.map(item => item.address.trim()).filter(Boolean).sort(),
    value.sort(),
  ) || !isEqual(currentChainValue.sort(), chainValue.sort());

  return (
    <TableColumnFilter
      title={ title }
      isFilled={ isFilled }
      isTouched={ isTouched }
      onFilter={ onFilter }
      onReset={ onReset }
      hasReset
    >
      <VStack gap={ 3 } align="stretch">
        <ChainSelect
          value={ currentChainValue }
          onValueChange={ handleChainChange }
          chainsConfig={ chains }
          loading={ isChainsLoading }
        />
        <VStack gap={ 2 } align="stretch">
          { currentValue.map((item, index) => (
            <AddressFilterInput
              key={ index }
              address={ item.address }
              isLast={ index === currentValue.length - 1 }
              onChange={ handleAddressChange(index) }
              onClear={ handleAddressClear(index) }
              onAddFieldClick={ handleAddField }
              placeholder={ placeholder }
            />
          )) }
        </VStack>
      </VStack>
    </TableColumnFilter>
  );
};

export default ZetaChainAddressFilter;
