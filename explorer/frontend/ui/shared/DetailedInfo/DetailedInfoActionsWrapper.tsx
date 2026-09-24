import React from 'react';

import * as DetailedInfo from './DetailedInfo';

export const TX_ACTIONS_BLOCK_ID = 'tx-actions';

type Props = {
  children: React.ReactNode;
  isLoading?: boolean;
  type: 'tx' | 'user_op';
};

const DetailedInfoActionsWrapper = ({ children, isLoading, type }: Props) => {
  const [ hasScroll, setHasScroll ] = React.useState(false);

  return (
    <>
      <DetailedInfo.ItemLabel
        id={ TX_ACTIONS_BLOCK_ID }
        hint={ `Highlighted events of the ${ type === 'tx' ? 'transaction' : 'user operation' }` }
        isLoading={ isLoading }
        hasScroll={ hasScroll }
      >
        <span>{ `${ type === 'tx' ? 'Transaction' : 'User operation' } action` }</span>
      </DetailedInfo.ItemLabel>
      <DetailedInfo.ItemValueWithScroll
        gradientHeight={ 48 }
        onScrollVisibilityChange={ setHasScroll }
        alignItems="stretch"
        rowGap={ 5 }
        w="100%"
        maxH="200px"
      >
        { children }
      </DetailedInfo.ItemValueWithScroll>

    </>
  );
};

export default React.memo(DetailedInfoActionsWrapper);
