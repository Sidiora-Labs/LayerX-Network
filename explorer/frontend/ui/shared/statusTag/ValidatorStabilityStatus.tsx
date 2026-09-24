import React from 'react';

import type { ValidatorStability } from 'types/api/validators';

import StatusTag from './StatusTag';

interface Props {
  state: ValidatorStability['state'];
  isLoading?: boolean;
}

const ValidatorStabilityStatus = ({ state, isLoading }: Props) => {
  switch (state) {
    case 'active':
      return <StatusTag type="ok" text="Active" loading={ isLoading }/>;
    case 'probation':
      return <StatusTag type="pending" text="Probation" loading={ isLoading }/>;
    case 'inactive':
      return <StatusTag type="error" text="Inactive" loading={ isLoading }/>;
  }
};

export default React.memo(ValidatorStabilityStatus);
