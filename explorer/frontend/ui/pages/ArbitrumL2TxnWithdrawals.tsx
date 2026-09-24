import { Box, chakra, Text } from '@chakra-ui/react';
import { useRouter } from 'next/router';
import React from 'react';

import useApiQuery from 'lib/api/useApiQuery';
import { layerLabels } from 'lib/rollups/utils';
import getQueryParamString from 'lib/router/getQueryParamString';
import { ARBITRUM_L2_TXN_WITHDRAWALS_ITEM } from 'stubs/arbitrumL2';
import { FilterInput } from 'toolkit/components/filters/FilterInput';
import { FormFieldError } from 'toolkit/components/forms/components/FormFieldError';
import { TRANSACTION_HASH_REGEXP } from 'toolkit/components/forms/validators/transaction';
import DataListDisplay from 'ui/shared/DataListDisplay';
import PageTitle from 'ui/shared/Page/PageTitle';
import ArbitrumL2TxnWithdrawalsList from 'ui/txnWithdrawals/arbitrumL2/ArbitrumL2TxnWithdrawalsList';
import ArbitrumL2TxnWithdrawalsTable from 'ui/txnWithdrawals/arbitrumL2/ArbitrumL2TxnWithdrawalsTable';

const ArbitrumL2TxnWithdrawals = () => {
  const router = useRouter();

  const [ searchTerm, setSearchTerm ] = React.useState(getQueryParamString(router.query.q) || undefined);
  const [ error, setError ] = React.useState<string | null>(null);

  const { data, isError, isPlaceholderData } = useApiQuery('general:arbitrum_l2_txn_withdrawals', {
    pathParams: {
      hash: searchTerm,
    },
    queryOptions: {
      enabled: Boolean(searchTerm),
      placeholderData: {
        items: [ ARBITRUM_L2_TXN_WITHDRAWALS_ITEM ],
      },
    },
  });

  const handleSearchTermChange = React.useCallback(() => {
    setError(null);
  }, [ ]);

  const handleSearchInputFocus = React.useCallback(() => {}, []);

  const handleSearchInputBlur = React.useCallback((event: React.FocusEvent<HTMLInputElement>) => {
    const { value } = event.target;

    if (!value || TRANSACTION_HASH_REGEXP.test(value)) {
      setSearchTerm(value);
      setError(null);
      router.push(
        { pathname: router.pathname, query: value ? { q: value } : undefined },
        undefined,
        { shallow: true },
      );
    } else {
      setError('Invalid transaction hash');
      setSearchTerm(undefined);
    }
  }, [ router ]);

  const handleSubmit = React.useCallback((event: React.FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const formData = new FormData(event.target as HTMLFormElement);
    const searchTerm = formData.get('tx_hash') as string;
    handleSearchInputBlur({ target: { value: searchTerm } } as React.FocusEvent<HTMLInputElement>);
  }, [ handleSearchInputBlur ]);

  const content = data?.items ? (
    <>
      <Box display={{ base: 'block', lg: 'none' }} mt={ 6 }>
        <ArbitrumL2TxnWithdrawalsList data={ data.items } txHash={ searchTerm } isLoading={ isPlaceholderData }/>
      </Box>
      <Box display={{ base: 'none', lg: 'block' }} mt={ 6 }>
        <ArbitrumL2TxnWithdrawalsTable data={ data.items } txHash={ searchTerm } isLoading={ isPlaceholderData }/>
      </Box>
    </>
  ) : null;

  return (
    <>
      <PageTitle title="Transaction withdrawals" withTextAd/>
      <Text>
        { layerLabels.current } to { layerLabels.parent } message relayer: search for your { layerLabels.current } transaction to execute a manual withdrawal.
      </Text>
      <chakra.form onSubmit={ handleSubmit } noValidate>
        <FilterInput
          name="tx_hash"
          w={{ base: '100%', lg: '700px' }}
          mt={ 6 }
          size="sm"
          placeholder="Search by transaction hash"
          initialValue={ searchTerm }
          onChange={ handleSearchTermChange }
          onFocus={ handleSearchInputFocus }
          onBlur={ handleSearchInputBlur }
        />
      </chakra.form>
      { error && <FormFieldError message={ error }/> }
      <DataListDisplay
        mt={ 6 }
        isError={ isError }
        itemsNum={ searchTerm ? data?.items.length : undefined }
        hasActiveFilters={ Boolean(searchTerm) }
        emptyStateProps={{
          term: 'withdrawal',
        }}
      >
        { content }
      </DataListDisplay>
    </>
  );
};

export default React.memo(ArbitrumL2TxnWithdrawals);
