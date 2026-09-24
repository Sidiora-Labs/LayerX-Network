import React from 'react';

import useIsInitialLoading from 'lib/hooks/useIsInitialLoading';
import { TruncatedText } from 'toolkit/components/truncation/TruncatedText';

interface Props {
  value: string | undefined;
  isLoading: boolean;
  type: 'transaction' | 'transfer';
}

const ListCounterText = ({ isLoading, value, type }: Props) => {
  const isInitialLoading = useIsInitialLoading(isLoading);

  if (value === undefined || value === '0') {
    return null;
  }

  const valueNum = Number(value);
  const text = `A total of ${ valueNum.toLocaleString() } ${ valueNum === 1 ? type : `${ type }s` } found`;

  return (
    <TruncatedText
      text={ text }
      loading={ isInitialLoading }
      textStyle={{ base: 'md', lg: 'sm' }}
      color="text.secondary"
      ml={{ base: 0, lg: 4 }}
      mr={{ base: 0, lg: 8 }}
      mb={{ base: 4, lg: 0 }}
    />
  );
};

export default React.memo(ListCounterText);
