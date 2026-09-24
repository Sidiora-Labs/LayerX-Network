import { chakra } from '@chakra-ui/react';
import React from 'react';

import config from 'configs/app';

import * as BlockEntity from './BlockEntity';

const rollupFeature = config.features.rollup;

const BlockEntityL2 = (props: BlockEntity.EntityProps) => {
  if (!rollupFeature.isEnabled) {
    return null;
  }

  return <BlockEntity.default { ...props } icon={{ name: 'txn_batches' }}/>;
};

export default chakra(BlockEntityL2);
