import type { StackProps } from '@chakra-ui/react';
import { chakra } from '@chakra-ui/react';
import React from 'react';

import type { Step } from './types';

import { Skeleton } from 'toolkit/chakra/skeleton';

import VerificationStep from './VerificationStep';

export interface Props {
  currentStep: string;
  currentStepPending?: boolean;
  steps: Array<Step>;
  isLoading?: boolean;
  rightSlot?: React.ReactNode;
  className?: string;
  itemProps?: StackProps;
}

const VerificationSteps = ({ currentStep, currentStepPending, steps, isLoading, rightSlot, className, itemProps }: Props) => {
  const currentStepIndex = steps.findIndex((step) => {
    const label = typeof step === 'string' ? step : step.label;
    return label === currentStep;
  });

  return (
    <Skeleton
      className={ className }
      loading={ isLoading }
      display="flex"
      columnGap={ 2 }
      alignItems="center"
      flexWrap="wrap"
    >
      { steps.map((step, index) => (
        <VerificationStep
          key={ index }
          step={ step }
          isLast={ index === steps.length - 1 && !rightSlot }
          isPassed={ index <= currentStepIndex }
          isPending={ index === currentStepIndex && currentStepPending }
          noIcon={ typeof step !== 'string' && index === currentStepIndex }
          { ...itemProps }
        />
      )) }
      { rightSlot }
    </Skeleton>
  );
};

export default chakra(VerificationSteps);
