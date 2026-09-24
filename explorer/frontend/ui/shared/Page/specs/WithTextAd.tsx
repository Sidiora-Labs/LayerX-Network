import React from 'react';

import { Badge } from 'toolkit/chakra/badge';

import PageTitle from '../PageTitle';

const WithTextAd = () => {
  return (
    <PageTitle
      title="Block"
      contentAfter={ <Badge key="custom" colorPalette="orange" variant="solid">Awesome</Badge> }
      withTextAd
    />
  );
};

export default WithTextAd;
