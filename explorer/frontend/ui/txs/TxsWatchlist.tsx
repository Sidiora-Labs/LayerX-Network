import React from 'react';

import type { QueryWithPagesResult } from 'ui/shared/pagination/useQueryWithPages';
import useRedirectForInvalidAuthToken from 'ui/snippets/auth/useRedirectForInvalidAuthToken';
import TxsWithFrontendSorting from 'ui/txs/TxsWithFrontendSorting';

type Props = {
  query: QueryWithPagesResult<'general:txs_watchlist'>;
  top?: number;
};

const TxsWatchlist = ({ query, top }: Props) => {
  useRedirectForInvalidAuthToken();
  return <TxsWithFrontendSorting query={ query } top={ top }/>;
};

export default TxsWatchlist;
