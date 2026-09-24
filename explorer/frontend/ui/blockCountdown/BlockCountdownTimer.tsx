import { HStack, StackSeparator } from '@chakra-ui/react';
import React from 'react';

import { SECOND } from 'toolkit/utils/consts';

import BlockCountdownTimerItem from './BlockCountdownTimerItem';
import splitSecondsInPeriods from './splitSecondsInPeriods';

interface Props {
  value: number;
  onFinish: () => void;
}

const BlockCountdownTimer = ({ value: initialValue, onFinish }: Props) => {

  const [ value, setValue ] = React.useState(initialValue);

  React.useEffect(() => {
    const intervalId = window.setInterval(() => {
      setValue((prev) => {
        if (prev > 1) {
          return prev - 1;
        }

        onFinish();
        return 0;
      });
    }, SECOND);

    return () => {
      window.clearInterval(intervalId);
    };
  }, [ initialValue, onFinish ]);

  const periods = splitSecondsInPeriods(value);

  return (
    <HStack
      bgColor={{ _light: 'gray.50', _dark: 'whiteAlpha.100' }}
      mt={{ base: 6, lg: 8 }}
      p={{ base: 3, lg: 4 }}
      borderRadius="base"
      separator={ <StackSeparator borderColor="border.divider"/> }
    >
      <BlockCountdownTimerItem label="Days" value={ periods.days }/>
      <BlockCountdownTimerItem label="Hours" value={ periods.hours }/>
      <BlockCountdownTimerItem label="Minutes" value={ periods.minutes }/>
      <BlockCountdownTimerItem label="Seconds" value={ periods.seconds }/>
    </HStack>
  );
};

export default React.memo(BlockCountdownTimer);
