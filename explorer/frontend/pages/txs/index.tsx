import type { NextPage } from 'next';
import dynamic from 'next/dynamic';
import React from 'react';

import PageNextJs from 'nextjs/PageNextJs';

import config from 'configs/app';

const Transactions = dynamic(() => {
  if (config.features.multichain.isEnabled) {
    return import('ui/multichain/txs/MultichainTxs');
  }

  if (config.features.zetachain.isEnabled) {
    return import('ui/pages/TransactionsZetaChain');
  }

  if (config.features.crossChainTxs.isEnabled) {
    return import('ui/crossChain/txs/Transactions');
  }

  return import('ui/pages/Transactions');
}, { ssr: false });

const Page: NextPage = () => {

  return (
    <PageNextJs pathname="/txs">
      <Transactions/>
    </PageNextJs>
  );
};

export default Page;

export { base as getServerSideProps } from 'nextjs/getServerSideProps/main';
