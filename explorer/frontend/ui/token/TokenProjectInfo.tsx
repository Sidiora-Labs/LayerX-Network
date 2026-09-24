import React from 'react';

import type { TokenVerifiedInfo } from 'types/api/token';

import InfoButton from 'ui/shared/InfoButton';

import Content, { hasContent } from './TokenProjectInfo/Content';

interface Props {
  data: TokenVerifiedInfo;
}

const TokenProjectInfo = ({ data }: Props) => {
  if (!hasContent(data)) {
    return null;
  }

  return (
    <InfoButton>
      <Content data={ data }/>
    </InfoButton>
  );
};

export default React.memo(TokenProjectInfo);
