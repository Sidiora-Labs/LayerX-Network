import React from 'react';

import { CctxStatus } from '@blockscout/zetachain-cctx-types';

import { Tag } from 'toolkit/chakra/tag';

type Props = {
  status: CctxStatus;
  isLoading?: boolean;
};

const TagText: Record<CctxStatus, string> = {
  [CctxStatus.PENDING_OUTBOUND]: 'Pending outbound',
  [CctxStatus.PENDING_INBOUND]: 'Pending inbound',
  [CctxStatus.OUTBOUND_MINED]: 'Outbound mined',
  [CctxStatus.PENDING_REVERT]: 'Pending revert',
  [CctxStatus.ABORTED]: 'Aborted',
  [CctxStatus.REVERTED]: 'Reverted',
  [CctxStatus.UNRECOGNIZED]: 'Unknown Status',
};

const ZetaChainCCTXStatusTag = ({ status, isLoading }: Props) => {
  return (
    <Tag loading={ isLoading }>
      { TagText[status] }
    </Tag>
  );
};

export default React.memo(ZetaChainCCTXStatusTag);
