import { useRouter } from 'next/router';
import React from 'react';

import getQueryParamString from 'lib/router/getQueryParamString';
import { TX } from 'stubs/tx';
import { generateListStub } from 'stubs/utils';
import AddressEntity from 'ui/shared/entities/address/AddressEntity';
import PageTitle from 'ui/shared/Page/PageTitle';
import useQueryWithPages from 'ui/shared/pagination/useQueryWithPages';
import TxsWithFrontendSorting from 'ui/txs/TxsWithFrontendSorting';

const KettleTxs = () => {
  const router = useRouter();

  const hash = getQueryParamString(router.query.hash);

  const query = useQueryWithPages({
    resourceName: 'general:txs_execution_node',
    pathParams: { hash },
    options: {
      placeholderData: generateListStub<'general:txs_execution_node'>(TX, 50, { next_page_params: {
        block_number: 9005713,
        index: 5,
        items_count: 50,
        filter: 'validated',
      } }),
    },
  });

  return (
    <>
      <PageTitle title="Computor transactions" withTextAd/>
      <AddressEntity address={{ hash }} mb={ 6 }/>
      <TxsWithFrontendSorting query={ query }/>
    </>
  );
};

export default KettleTxs;
