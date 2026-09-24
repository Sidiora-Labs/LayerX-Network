import { chakra, Flex } from '@chakra-ui/react';
import React from 'react';

import { Switch } from 'toolkit/chakra/switch';
import { Hint } from 'toolkit/components/Hint/Hint';

interface Props {
  id: string;
  onChange: (isChecked: boolean) => void;
  initialValue?: boolean;
  isDisabled?: boolean;
  className?: string;
}

const UserOpCallDataSwitch = ({ className, initialValue, isDisabled, onChange, id }: Props) => {
  const [ isChecked, setIsChecked ] = React.useState(initialValue ?? false);

  const handleChange = React.useCallback(() => {
    setIsChecked((prevValue) => {
      const nextValue = !prevValue;
      onChange(nextValue);
      return nextValue;
    });
  }, [ onChange ]);

  return (
    <Flex ml="auto" alignItems="center" gap={ 2 }>
      <Switch
        className={ className }
        id={ id }
        checked={ isChecked }
        disabled={ isDisabled }
        onCheckedChange={ handleChange }
        direction="rtl"
        labelProps={{ fontWeight: '600', textStyle: 'sm' }}
      >
        <chakra.span hideBelow="lg">Show external call data</chakra.span>
        <chakra.span hideFrom="lg">External call data</chakra.span>
      </Switch>
      <Hint label="Inner call data is a predicted decoded call from this user operation"/>
    </Flex>
  );
};

export default React.memo(chakra(UserOpCallDataSwitch));
