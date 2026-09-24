import React from 'react';

import type { MarketplaceApp } from 'types/client/marketplace';

import InfoButton from 'ui/shared/InfoButton';

import Content from './MarketplaceAppInfo/Content';

interface Props {
  data: MarketplaceApp | undefined;
  isLoading?: boolean;
}

const MarketplaceAppInfo = ({ data, isLoading }: Props) => {
  return (
    <InfoButton isLoading={ isLoading }>
      <Content data={ data }/>
    </InfoButton>
  );
};

export default React.memo(MarketplaceAppInfo);
