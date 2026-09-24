import { chakra } from '@chakra-ui/react';
import React from 'react';

import dayjs from 'lib/date/dayjs';

interface Props {
  date: string | undefined;
}

const NameDomainExpiryStatus = ({ date }: Props) => {
  if (!date) {
    return null;
  }

  const hasExpired = dayjs(date).isBefore(dayjs());

  if (hasExpired) {
    return <chakra.span color="red.600">Expired</chakra.span>;
  }

  const diff = dayjs(date).diff(dayjs(), 'day');
  if (diff < 30) {
    return <chakra.span color="red.600">{ diff } days left</chakra.span>;
  }

  return <chakra.span color="text.secondary">Expires { dayjs(date).fromNow() }</chakra.span>;
};

export default React.memo(NameDomainExpiryStatus);
