import { Box } from '@chakra-ui/react';
import { useRouter } from 'next/router';
import React from 'react';

import config from 'configs/app';
import useApiQuery from 'lib/api/useApiQuery';
import getQueryParamString from 'lib/router/getQueryParamString';
import { Heading } from 'toolkit/chakra/heading';
import ActivityList from 'ui/paxeerX/account/ActivityList';
import AssetList from 'ui/paxeerX/account/AssetList';
import IdentityList from 'ui/paxeerX/account/IdentityList';
import { UNIFIED_ACCOUNT_PLACEHOLDER } from 'ui/paxeerX/account/placeholderData';
import TextAd from 'ui/shared/ad/TextAd';
import DataFetchAlert from 'ui/shared/DataFetchAlert';
import PageTitle from 'ui/shared/Page/PageTitle';

const feature = config.features.paxeerXLists;

interface SectionProps {
  title: string;
  children: React.ReactNode;
}

const Section = ({ title, children }: SectionProps) => (
  <Box mb={ 8 }>
    <Heading level="2" mb={ 3 }>{ title }</Heading>
    { children }
  </Box>
);

const PaxeerXAccountPageContent = () => {
  const router = useRouter();
  const hash = getQueryParamString(router.query.hash);

  const capabilitiesQuery = useApiQuery('general:paxeer_x_capabilities', {
    queryOptions: {
      enabled: feature.isEnabled,
    },
  });

  const accountQuery = useApiQuery('general:paxeer_x_unified_account', {
    pathParams: { hash },
    queryOptions: {
      enabled: feature.isEnabled && Boolean(hash),
      placeholderData: UNIFIED_ACCOUNT_PLACEHOLDER,
    },
  });

  const isLoading = accountQuery.isPlaceholderData;
  const capabilities = capabilitiesQuery.data;

  if (!feature.isEnabled) {
    return null;
  }

  const content = (() => {
    if (accountQuery.isError) {
      return <DataFetchAlert/>;
    }

    const data = accountQuery.data;

    if (!data) {
      return <DataFetchAlert/>;
    }

    return (
      <>
        <Section title="Identities">
          { capabilities && !capabilities.addr ? (
            <Box color="text.secondary">The account binding precompile is not available on this node.</Box>
          ) : (
            <IdentityList identities={ data.identities } isLoading={ isLoading }/>
          ) }
        </Section>
        <Section title="Assets">
          { capabilities && !capabilities.custody ? (
            <Box color="text.secondary">The custody precompile is not available on this node, so only chain balances are counted.</Box>
          ) : null }
          <AssetList items={ data.balances } isLoading={ isLoading }/>
        </Section>
        <Section title="Activity">
          <ActivityList items={ data.activity } isLoading={ isLoading }/>
        </Section>
      </>
    );
  })();

  return (
    <>
      <TextAd mb={ 6 }/>
      <PageTitle title="Unified account" isLoading={ isLoading }/>
      <Box mb={ 6 } color="text.secondary" wordBreak="break-all">{ hash }</Box>
      { content }
    </>
  );
};

export default PaxeerXAccountPageContent;
