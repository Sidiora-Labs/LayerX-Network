import React from 'react';

import config from 'configs/app';
import type { BadgeProps } from 'toolkit/chakra/badge';

import StatusLadderBadge from './StatusLadderBadge';
import useTxStatusLadderQuery from './useTxStatusLadderQuery';

const feature = config.features.paxeerXLists;

export interface Props extends Omit<BadgeProps, 'children'> {
  hash: string | undefined;
}

const TxStatusLadderBadge = ({ hash, ...rest }: Props) => {
  const { data, isPlaceholderData, isError } = useTxStatusLadderQuery(hash);

  if (!feature.isEnabled || isError || !data) {
    return null;
  }

  return <StatusLadderBadge rung={ data.rung } isLoading={ isPlaceholderData } { ...rest }/>;
};

export default TxStatusLadderBadge;
