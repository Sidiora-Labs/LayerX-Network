import React from 'react';

import type { SmartContractCreationStatus } from 'types/api/contract';

import type { BadgeProps } from 'toolkit/chakra/badge';
import { Badge } from 'toolkit/chakra/badge';
import { Tooltip } from 'toolkit/chakra/tooltip';

import StatusTag from './StatusTag';

interface Props extends BadgeProps {
  status: SmartContractCreationStatus;
}

const ContractCreationStatus = ({ status, ...rest }: Props) => {
  switch (status) {
    case 'success':
      return (
        <Tooltip content="The contract was successfully created">
          <StatusTag type="ok" text="Success" { ...rest }/>
        </Tooltip>
      );
    case 'failed':
      return (
        <Tooltip content="The creation transaction failed">
          <StatusTag type="error" text="Failed" { ...rest }/>
        </Tooltip>
      );
    case 'selfdestructed':
      return (
        <Tooltip content="The contract was created at some point but has since self-destructed">
          <Badge colorPalette="gray" { ...rest }>Self-destructed</Badge>
        </Tooltip>
      );
    default:
      return null;
  }
};

export default React.memo(ContractCreationStatus);
