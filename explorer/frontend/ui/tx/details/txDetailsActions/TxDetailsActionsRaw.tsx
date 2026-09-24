import React from 'react';

import type { TxAction } from 'types/api/txAction';

import * as DetailedInfo from 'ui/shared/DetailedInfo/DetailedInfo';
import DetailedInfoActionsWrapper from 'ui/shared/DetailedInfo/DetailedInfoActionsWrapper';

import TxDetailsAction from './TxDetailsAction';

interface Props {
  actions: Array<TxAction>;
  isLoading: boolean;
}

const TxDetailsActionsRaw = ({ actions, isLoading }: Props) => {
  return (
    <>
      <DetailedInfoActionsWrapper isLoading={ isLoading } type="tx">
        { actions.map((action, index: number) => <TxDetailsAction key={ index } action={ action }/>) }
      </DetailedInfoActionsWrapper>
      <DetailedInfo.ItemDivider/>
    </>
  );
};

export default TxDetailsActionsRaw;
