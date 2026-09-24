import type { NextPage } from 'next';
import dynamic from 'next/dynamic';
import React from 'react';

import PageNextJs from 'nextjs/PageNextJs';

import config from 'configs/app';
const rollupFeature = config.features.rollup;

const Withdrawals = dynamic(() => {
  if (rollupFeature.isEnabled && rollupFeature.type === 'arbitrum') {
    return import('ui/pages/ArbitrumL2TxnWithdrawals');
  }

  throw new Error('Txn withdrawals feature is not enabled.');
}, { ssr: false });

const Page: NextPage = () => {
  return (
    <PageNextJs pathname="/txn-withdrawals">
      <Withdrawals/>
    </PageNextJs>
  );
};

export default Page;

export { txnWithdrawals as getServerSideProps } from 'nextjs/getServerSideProps/main';
