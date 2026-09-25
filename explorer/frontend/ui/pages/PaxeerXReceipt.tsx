import { Box } from '@chakra-ui/react';
import { useRouter } from 'next/router';
import React from 'react';

import useApiQuery from 'lib/api/useApiQuery';
import throwOnAbsentParamError from 'lib/errors/throwOnAbsentParamError';
import throwOnResourceLoadError from 'lib/errors/throwOnResourceLoadError';
import getQueryParamString from 'lib/router/getQueryParamString';
import PaxeerXReceiptDetails from 'ui/paxeerX/receipts/PaxeerXReceiptDetails';
import TextAd from 'ui/shared/ad/TextAd';
import PageTitle from 'ui/shared/Page/PageTitle';

const PaxeerXReceiptPageContent = () => {
  const router = useRouter();
  const id = getQueryParamString(router.query.id);

  const receiptQuery = useApiQuery('general:paxeer_x_receipt', {
    pathParams: { id },
    queryOptions: {
      enabled: Boolean(id),
    },
  });

  throwOnAbsentParamError(id);
  throwOnResourceLoadError(receiptQuery);

  const isLoading = receiptQuery.isPending;
  const data = receiptQuery.data;

  return (
    <>
      <TextAd mb={ 6 }/>
      <PageTitle title="Kernel receipt" isLoading={ isLoading }/>
      <Box mb={ 6 } color="text.secondary" wordBreak="break-all">{ id }</Box>
      { data ? <PaxeerXReceiptDetails data={ data }/> : null }
    </>
  );
};

export default PaxeerXReceiptPageContent;
