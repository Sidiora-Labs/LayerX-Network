import type { GetServerSideProps, NextPage } from 'next';
import React from 'react';

import type { Route } from 'nextjs-routes';
import type { Props } from 'nextjs/getServerSideProps/handlers';
import * as gSSP from 'nextjs/getServerSideProps/main';
import PageNextJs from 'nextjs/PageNextJs';
import detectBotRequest from 'nextjs/utils/detectBotRequest';
import fetchApi from 'nextjs/utils/fetchApi';

import config from 'configs/app';
import getQueryParamString from 'lib/router/getQueryParamString';
import Token from 'ui/pages/Token';

const pathname: Route['pathname'] = '/token/[hash]';

const Page: NextPage<Props<typeof pathname>> = (props: Props<typeof pathname>) => {
  return (
    <PageNextJs pathname={ pathname } query={ props.query } apiData={ props.apiData }>
      <Token/>
    </PageNextJs>
  );
};

export default Page;

export const getServerSideProps: GetServerSideProps<Props<typeof pathname>> = async(ctx) => {
  const baseResponse = await gSSP.token<typeof pathname>(ctx);

  if ('props' in baseResponse && !config.features.multichain.isEnabled) {
    if (
      config.meta.seo.enhancedDataEnabled ||
      (config.meta.og.enhancedDataEnabled && detectBotRequest(ctx.req)?.type === 'social_preview')
    ) {
      const tokenData = await fetchApi({
        resource: 'general:token',
        pathParams: { hash: getQueryParamString(ctx.query.hash) },
        timeout: 500,
      });
      const apiData = tokenData ? {
        ...tokenData,
        symbol_or_name: tokenData.symbol ?? tokenData.name ?? '',
      } : null;

      (await baseResponse.props).apiData = apiData;
    }
  }

  return baseResponse;
};
