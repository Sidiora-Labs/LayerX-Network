import React from 'react';

import type { Props as StatusTagProps } from './StatusTag';
import StatusTag from './StatusTag';

export interface Props extends Omit<StatusTagProps, 'type' | 'text'> {
  isFinalized: boolean;
}

const CeloEpochStatus = ({ isFinalized, ...rest }: Props) => {
  return (
    <StatusTag
      { ...rest }
      type={ isFinalized ? 'ok' : 'pending' }
      text={ isFinalized ? 'Finalized' : 'In progress' }
    />
  );
};

export default CeloEpochStatus;
