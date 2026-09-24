import BigNumber from 'bignumber.js';

import type { Block } from 'types/api/block';

import { ZERO } from 'toolkit/utils/consts';
import { WEI } from 'ui/shared/value/utils';

export default function getBlockTotalReward(block: Block) {
  const totalReward = block.rewards
    ?.map(({ reward }) => BigNumber(reward))
    .reduce((result, item) => result.plus(item), ZERO) || ZERO;

  return totalReward.div(WEI);
}
