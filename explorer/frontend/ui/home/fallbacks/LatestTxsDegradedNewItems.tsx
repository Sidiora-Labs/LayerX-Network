import React from 'react';

import useGradualIncrement from 'lib/hooks/useGradualIncrement';
import SocketNewItemsNotice from 'ui/shared/SocketNewItemsNotice';

interface Props {
  overflow: number;
  url: string;
  isLoading: boolean;
}

const LatestTxsDegradedNewItems = ({ overflow, url, isLoading }: Props) => {
  const [ num, setNum ] = useGradualIncrement(0);

  React.useEffect(() => {
    if (!isLoading && overflow > 0) {
      setNum(overflow);
    }
  }, [ isLoading, overflow, setNum ]);

  return <SocketNewItemsNotice borderBottomRadius={ 0 } num={ num } url={ url } showErrorAlert={ false } isLoading={ isLoading }/>;
};

export default React.memo(LatestTxsDegradedNewItems);
