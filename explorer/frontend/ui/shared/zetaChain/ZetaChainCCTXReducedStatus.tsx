import React from 'react';

import { CctxStatusReduced } from '@blockscout/zetachain-cctx-types';

import StatusTag, { type StatusTagType } from 'ui/shared/statusTag/StatusTag';

type Props = {
  status: CctxStatusReduced;
  isLoading?: boolean;
  type?: 'reduced' | 'full';
};

const ZetaChainCCTXReducedStatus = ({ status, isLoading, type = 'reduced' }: Props) => {
  let statusTagType: StatusTagType;
  switch (status) {
    case CctxStatusReduced.SUCCESS:
      statusTagType = 'ok';
      break;
    case CctxStatusReduced.PENDING:
      statusTagType = 'pending';
      break;
    case CctxStatusReduced.FAILED:
      statusTagType = 'error';
      break;
    case CctxStatusReduced.UNRECOGNIZED:
      statusTagType = 'pending';
      break;
  }

  if (type === 'full') {
    let text: string;
    switch (status) {
      case CctxStatusReduced.SUCCESS:
        text = 'Success';
        break;
      case CctxStatusReduced.PENDING:
        text = 'Pending';
        break;
      case CctxStatusReduced.FAILED:
        text = 'Failed';
        break;
      case CctxStatusReduced.UNRECOGNIZED:
        text = 'Unrecognized';
        break;
    }
    return <StatusTag type={ statusTagType } text={ text } size="md" loading={ isLoading }/>;
  }

  return <StatusTag type={ statusTagType } size="sm" loading={ isLoading }/>;
};

export default ZetaChainCCTXReducedStatus;
