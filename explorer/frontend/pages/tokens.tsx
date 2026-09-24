import type { NextPage } from 'next';
import dynamic from 'next/dynamic';
import React from 'react';

import PageNextJs from 'nextjs/PageNextJs';

import config from 'configs/app';

const Tokens = dynamic(() => {
  if (config.features.multichain.isEnabled) {
    return import('ui/multichain/tokens/MultichainTokens');
  }

  return import('ui/pages/Tokens');
}, { ssr: false });

const Page: NextPage = () => {
  return (
    <PageNextJs pathname="/tokens">
      <Tokens/>
    </PageNextJs>
  );
};

export default Page;

export { base as getServerSideProps } from 'nextjs/getServerSideProps/main';
