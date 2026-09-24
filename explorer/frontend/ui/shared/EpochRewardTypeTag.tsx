import React from 'react';

import type { CeloEpochRewardsType } from 'types/api/epochs';

import type { BadgeProps } from 'toolkit/chakra/badge';
import { Badge } from 'toolkit/chakra/badge';
import { Tooltip } from 'toolkit/chakra/tooltip';

type Props = {
  type: CeloEpochRewardsType;
  isLoading?: boolean;
};

const TYPE_TAGS: Record<CeloEpochRewardsType, { text: string; label: string; color: BadgeProps['colorPalette'] }> = {
  group: {
    text: 'Validator group rewards',
    // eslint-disable-next-line max-len
    label: 'Reward given to a validator group. The address being viewed is the group\'s address; the associated address is the validator\'s address on whose behalf the reward was paid.',
    color: 'teal',
  },
  validator: {
    text: 'Validator rewards',
    label: 'Reward given to a validator. The address being viewed is the validator\'s address; the associated address is the validator group\'s address.',
    color: 'purple',
  },
  delegated_payment: {
    text: 'Delegated payments',
    // eslint-disable-next-line max-len
    label: 'Reward portion delegated by a validator to another address. The address being viewed is the beneficiary receiving the reward; the associated address is the validator who set the delegation.',
    color: 'blue',
  },
  voter: {
    text: 'Voting rewards',
    label: 'Reward given to a voter. The address being viewed is the voter\'s address; the associated address is the group address.',
    color: 'yellow',
  },
};

const EpochRewardTypeTag = ({ type, isLoading }: Props) => {
  const { text, label, color } = TYPE_TAGS[type];

  return (
    <Tooltip content={ label }>
      <Badge colorPalette={ color } loading={ isLoading }>
        { text }
      </Badge>
    </Tooltip>
  );
};

export default React.memo(EpochRewardTypeTag);
