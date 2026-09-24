import { chakra } from '@chakra-ui/react';
import React from 'react';

import type { ButtonGroupRadioProps } from 'toolkit/chakra/button';
import { Button, ButtonGroupRadio } from 'toolkit/chakra/button';
import IconSvg from 'ui/shared/IconSvg';

import type { TNftDisplayType } from './useAddressNftQuery';

interface Props extends Omit<ButtonGroupRadioProps, 'children'> {
  value: TNftDisplayType;
}

const AddressNftDisplayTypeRadio = ({ value, onChange, ...rest }: Props) => {
  return (
    <ButtonGroupRadio
      defaultValue={ value }
      onChange={ onChange }
      equalWidth
      { ...rest }
    >
      <Button value="collection" size="sm" px={ 3 }>
        <IconSvg name="collection" boxSize={ 5 }/>
        <chakra.span hideBelow="lg">By collection</chakra.span>
      </Button>
      <Button value="list" size="sm" px={ 3 }>
        <IconSvg name="apps" boxSize={ 5 }/>
        <chakra.span hideBelow="lg">List</chakra.span>
      </Button>
    </ButtonGroupRadio>
  );
};

export default React.memo(AddressNftDisplayTypeRadio);
