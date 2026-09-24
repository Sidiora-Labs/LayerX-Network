import React from 'react';

import dayjs from 'lib/date/dayjs';
import { DAY, HOUR, MINUTE, SECOND } from 'toolkit/utils/consts';

function getUnits(diff: number) {
  if (diff < MINUTE) {
    return [ SECOND, MINUTE ];
  }

  if (diff < HOUR) {
    return [ MINUTE, HOUR ];
  }

  if (diff < DAY) {
    return [ HOUR, DAY ];
  }

  return [ DAY, 2 * DAY ];
}

function getUpdateParams(ts: string | number) {
  const timeDiff = Date.now() - new Date(ts).getTime();
  const [ unit, higherUnit ] = getUnits(timeDiff);

  if (unit === DAY) {
    return { interval: DAY };
  }

  const leftover = unit - timeDiff % unit;

  return {
    startTimeout: unit === SECOND ?
      0 :
      // here we assume that in current dayjs locale time difference is rounded by Math.round function
      // so we have to update displayed value whenever time comes over the middle of the unit interval
      // since it will be rounded to the upper bound
      (leftover < unit / 2 ? leftover + unit / 2 : leftover - unit / 2) + SECOND,
    endTimeout: higherUnit - timeDiff + SECOND,
    interval: unit,
  };
}

export default function useTimeAgoIncrement(ts: string | number | null, isEnabled?: boolean) {
  const [ value, setValue ] = React.useState(ts ? dayjs(ts).fromNow() : null);

  React.useEffect(() => {
    if (ts !== null) {
      const timeouts: Array<number> = [];
      const intervals: Array<number> = [];

      const startIncrement = () => {
        const { startTimeout, interval, endTimeout } = getUpdateParams(ts);
        if (!startTimeout && !endTimeout) {
          return;
        }

        let intervalId: number;

        const startTimeoutId = window.setTimeout(() => {
          setValue(dayjs(ts).fromNow());

          intervalId = window.setInterval(() => {
            setValue(dayjs(ts).fromNow());
          }, interval);

          intervals.push(intervalId);
        }, startTimeout);

        const endTimeoutId = window.setTimeout(() => {
          window.clearInterval(intervalId);
          startIncrement();
        }, endTimeout);

        timeouts.push(startTimeoutId);
        timeouts.push(endTimeoutId);
      };

      setValue(dayjs(ts).fromNow());

      isEnabled && startIncrement();

      !isEnabled && setValue(dayjs(ts).fromNow());

      return () => {
        timeouts.forEach(window.clearTimeout);
        intervals.forEach(window.clearInterval);
      };
    }
  }, [ isEnabled, ts ]);

  return value;
}
