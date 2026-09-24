import type { NextPage } from 'next';
import dynamic from 'next/dynamic';
import React from 'react';

import PageNextJs from 'nextjs/PageNextJs';

import config from 'configs/app';

const TokenTransfers = dynamic(() => {
  if (config.features.multichain.isEnabled) {
    return import('ui/multichain/tokenTransfers/MultichainTokenTransfers');
  }

  return import('ui/pages/TokenTransfers');
}, { ssr: false });

const Page: NextPage = () => {
  return (
    <PageNextJs pathname="/token-transfers">
      <TokenTransfers/>
    </PageNextJs>
  );
};

export default Page;

export { base as getServerSideProps } from 'nextjs/getServerSideProps/main';
