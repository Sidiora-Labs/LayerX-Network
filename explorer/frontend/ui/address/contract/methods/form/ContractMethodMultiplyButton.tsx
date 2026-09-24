import { chakra, List, Input, ListItem } from '@chakra-ui/react';
import React from 'react';

import { Button } from 'toolkit/chakra/button';
import { IconButton } from 'toolkit/chakra/icon-button';
import { PopoverBody, PopoverContent, PopoverRoot, PopoverTrigger } from 'toolkit/chakra/popover';
import { useDisclosure } from 'toolkit/hooks/useDisclosure';
import { times } from 'toolkit/utils/htmlEntities';
import IconSvg from 'ui/shared/IconSvg';

interface Props {
  onClick: (power: number) => void;
  isDisabled?: boolean;
  initialValue: number;
  onChange: (power: number) => void;
}

const ContractMethodMultiplyButton = ({ onClick, isDisabled, initialValue, onChange }: Props) => {
  const [ selectedOption, setSelectedOption ] = React.useState<number | undefined>(initialValue);
  const [ customValue, setCustomValue ] = React.useState<number>();
  const { open, onOpenChange } = useDisclosure();

  const handleOptionClick = React.useCallback((event: React.MouseEvent) => {
    const id = Number((event.currentTarget as HTMLDivElement).getAttribute('data-id'));
    if (!Object.is(id, NaN)) {
      setSelectedOption((prev) => prev === id ? undefined : id);
      setCustomValue(undefined);
      onOpenChange({ open: false });
      onChange(id);
    }
  }, [ onOpenChange, onChange ]);

  const handleInputChange = React.useCallback((event: React.ChangeEvent<HTMLInputElement>) => {
    const value = Number(event.target.value);
    setCustomValue(value);
    setSelectedOption(undefined);
    onChange(value);
  }, [ onChange ]);

  const value = selectedOption || customValue;

  const handleButtonClick = React.useCallback(() => {
    value && onClick(value);
  }, [ onClick, value ]);

  return (
    <>
      { Boolean(value) && (
        <Button
          px={ 1 }
          textStyle="md"
          size="xs"
          fontWeight={ 500 }
          ml={ 1 }
          variant="subtle"
          display="inline"
          onClick={ handleButtonClick }
          disabled={ isDisabled }
          borderBottomRightRadius={ 0 }
          borderTopRightRadius={ 0 }
        >
          { times }
          <chakra.span>10</chakra.span>
          <chakra.span fontSize="xs" lineHeight="16px" verticalAlign="super">{ value }</chakra.span>
        </Button>
      ) }
      <PopoverRoot open={ open } onOpenChange={ onOpenChange } positioning={{ placement: 'bottom-end' }}>
        <PopoverTrigger>
          <IconButton
            variant="subtle"
            cursor="pointer"
            boxSize={ 6 }
            p={ 0 }
            disabled={ isDisabled }
            borderBottomLeftRadius={ 0 }
            borderTopLeftRadius={ 0 }
            borderLeftWidth="1px"
            borderLeftColor="border.divider"
          >
            <IconSvg
              name="arrows/east-mini"
              transitionDuration="fast"
              transitionProperty="transform"
              transitionTimingFunction="ease-in-out"
              transform={ open ? 'rotate(90deg)' : 'rotate(-90deg)' }
              boxSize={ 6 }
            />
          </IconButton>
        </PopoverTrigger>
        <PopoverContent w="110px">
          <PopoverBody textStyle="md" py={ 2 }>
            <List.Root>
              { [ 8, 12, 16, 18, 20 ].map((id) => (
                <List.Item
                  key={ id }
                  py={ 2 }
                  data-id={ id }
                  onClick={ handleOptionClick }
                  display="flex"
                  justifyContent="space-between"
                  alignItems="center"
                  cursor="pointer"
                >
                  <span>10*{ id }</span>
                  { selectedOption === id && <IconSvg name="check" boxSize={ 6 }/> }
                </List.Item>
              )) }
              <ListItem
                py={ 2 }
                display="flex"
                justifyContent="space-between"
                alignItems="center"
              >
                <span>10*</span>
                <Input
                  type="number"
                  min={ 0 }
                  max={ 100 }
                  ml={ 3 }
                  size="sm"
                  onChange={ handleInputChange }
                  value={ customValue || '' }
                />
              </ListItem>
            </List.Root>
          </PopoverBody>
        </PopoverContent>
      </PopoverRoot>
    </>
  );
};

export default React.memo(ContractMethodMultiplyButton);
