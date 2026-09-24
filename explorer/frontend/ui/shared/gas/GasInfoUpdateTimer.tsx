import React from 'react';

import dayjs from 'lib/date/dayjs';
import type { ProgressCircleRootProps } from 'toolkit/chakra/progress-circle';
import { ProgressCircleRing, ProgressCircleRoot } from 'toolkit/chakra/progress-circle';

interface Props extends ProgressCircleRootProps {
  startTime: number;
  duration: number;
}

const getValue = (startDate: dayjs.Dayjs, duration: number) => {
  const now = dayjs();
  const diff = now.diff(startDate, 'ms');
  const value = diff / duration * 100;

  if (value >= 99) {
    return 99;
  }

  return value;
};

const GasInfoUpdateTimer = ({ startTime, duration, size = 'sm', ...rest }: Props) => {
  const [ value, setValue ] = React.useState(getValue(dayjs(startTime), duration));

  React.useEffect(() => {
    const startDate = dayjs(startTime);

    const intervalId = window.setInterval(() => {
      const nextValue = getValue(startDate, duration);
      setValue(nextValue);
      if (nextValue === 99) {
        window.clearInterval(intervalId);
      }
    }, 100);

    return () => {
      window.clearInterval(intervalId);
    };
  }, [ startTime, duration ]);

  return (
    <ProgressCircleRoot
      value={ value }
      size={ size }
      { ...rest }
    >
      <ProgressCircleRing/>
    </ProgressCircleRoot>
  );
};

export default React.memo(GasInfoUpdateTimer);
