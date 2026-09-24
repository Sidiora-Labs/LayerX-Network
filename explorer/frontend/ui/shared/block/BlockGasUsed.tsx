import { chakra } from '@chakra-ui/react';
import BigNumber from 'bignumber.js';
import React from 'react';

import config from 'configs/app';
import { Tooltip } from 'toolkit/chakra/tooltip';

import GasUsedToTargetRatio from '../GasUsedToTargetRatio';
import TextSeparator from '../TextSeparator';
import Utilization from '../Utilization/Utilization';

const rollupFeature = config.features.rollup;

interface Props {
  className?: string;
  gasUsed?: string;
  gasLimit: string;
  gasTarget?: number;
  isLoading?: boolean;
}

const BlockGasUsed = ({ className, gasUsed, gasLimit, gasTarget, isLoading }: Props) => {
  const hasGasUtilization =
    gasUsed && gasUsed !== '0' &&
    (!rollupFeature.isEnabled || rollupFeature.type === 'optimistic' || rollupFeature.type === 'shibarium');

  if (!hasGasUtilization) {
    return null;
  }

  return (
    <>
      <Tooltip content="Gas Used %" disabled={ isLoading }>
        <Utilization
          colorScheme="gray"
          value={ BigNumber(gasUsed).dividedBy(BigNumber(gasLimit)).toNumber() }
          isLoading={ isLoading }
          className={ className }
        />
      </Tooltip>
      { gasTarget && (
        <>
          <TextSeparator/>
          <GasUsedToTargetRatio value={ gasTarget } isLoading={ isLoading }/>
        </>
      ) }
    </>
  );
};

export default React.memo(chakra(BlockGasUsed));
