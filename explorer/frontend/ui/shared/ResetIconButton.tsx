import React from 'react';

import { CloseButton } from 'toolkit/chakra/close-button';
import { Tooltip } from 'toolkit/chakra/tooltip';

type Props = {
  onClick: () => void;
};

const ResetIconButton = ({ onClick }: Props) => {
  return (
    <Tooltip content="Reset filter">
      <CloseButton onClick={ onClick } ml={ 1 }/>
    </Tooltip>
  );
};

export default ResetIconButton;
