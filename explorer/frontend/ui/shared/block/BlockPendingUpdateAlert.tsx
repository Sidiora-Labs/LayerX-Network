import React from 'react';

import config from 'configs/app';
import type { AlertProps } from 'toolkit/chakra/alert';
import { Alert } from 'toolkit/chakra/alert';

interface Props extends AlertProps {
  view?: 'block' | 'tx';
}

const BlockPendingUpdateAlert = ({ view = 'block', ...props }: Props) => {
  if (!config.UI.views.block.pendingUpdateAlertEnabled) {
    return null;
  }

  const content = view === 'block' ?
    'Block is being re-synced. Details may be incomplete until the update is finished.' :
    'This transaction is part of a block that is being re-synced. Details may be incomplete until the update is finished.';

  return (
    <Alert status="info" showIcon { ...props }>
      { content }
    </Alert>
  );
};

export default React.memo(BlockPendingUpdateAlert);
