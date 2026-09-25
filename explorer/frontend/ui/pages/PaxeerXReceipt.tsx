import { Box } from '@chakra-ui/react';
import { useRouter } from 'next/router';
import React from 'react';

import type { PaxeerXReceipt } from 'types/api/paxeerXLists';

import useApiQuery from 'lib/api/useApiQuery';
import throwOnAbsentParamError from 'lib/errors/throwOnAbsentParamError';
import throwOnResourceLoadError from 'lib/errors/throwOnResourceLoadError';
import getQueryParamString from 'lib/router/getQueryParamString';
import { PAXEER_X_RECEIPTS_ITEM } from 'stubs/paxeerXLists';
import { TX_HASH } from 'stubs/tx';
import PaxeerXReceiptDetails from 'ui/paxeerX/receipts/PaxeerXReceiptDetails';
import TextAd from 'ui/shared/ad/TextAd';
import PageTitle from 'ui/shared/Page/PageTitle';

// The skeleton the page shows while the single-receipt request is in flight; it carries the shape
// of the payload, not a value the page ever renders as data.
const PAXEER_X_RECEIPT_PLACEHOLDER: PaxeerXReceipt = {
  ...PAXEER_X_RECEIPTS_ITEM,
  verification_status: 'checkpoint_finalised',
  payload_hash: TX_HASH,
  transaction_hash: TX_HASH,
  timestamp: '2023-05-22T18:00:36.000000Z',
};

const PaxeerXReceiptPageContent = () => {
  const router = useRouter();
  const id = getQueryParamString(router.query.id);

  const receiptQuery = useApiQuery('general:paxeer_x_receipt', {
    pathParams: { id },
    queryOptions: {
      enabled: Boolean(id),
      placeholderData: PAXEER_X_RECEIPT_PLACEHOLDER,
    },
  });

  throwOnAbsentParamError(id);
  throwOnResourceLoadError(receiptQuery);

  const isLoading = receiptQuery.isPlaceholderData;
  const data = receiptQuery.data;

  return (
    <>
      <TextAd mb={ 6 }/>
      <PageTitle title="Kernel receipt" isLoading={ isLoading }/>
      <Box mb={ 6 } color="text.secondary" wordBreak="break-all">{ id }</Box>
      { data ? <PaxeerXReceiptDetails data={ data } isLoading={ isLoading }/> : null }
    </>
  );
};

export default PaxeerXReceiptPageContent;
