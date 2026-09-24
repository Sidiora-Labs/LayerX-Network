import type { NextPage } from 'next';
import dynamic from 'next/dynamic';
import React from 'react';

import PageNextJs from 'nextjs/PageNextJs';

const MultichainEcosystems = dynamic(() => import('ui/multichain/ecosystems/MultichainEcosystems'), { ssr: false });

const Page: NextPage = () => {
  return (
    <PageNextJs pathname="/ecosystems">
      <MultichainEcosystems/>
    </PageNextJs>
  );
};

export default Page;

export { base as getServerSideProps } from 'nextjs/getServerSideProps/main';
