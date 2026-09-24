import type { NextPage } from 'next';
import dynamic from 'next/dynamic';
import React from 'react';

import PageNextJs from 'nextjs/PageNextJs';

const Epochs = dynamic(() => import('ui/pages/Epochs'), { ssr: false });

const Page: NextPage = () => {
  return (
    <PageNextJs pathname="/epochs">
      <Epochs/>
    </PageNextJs>
  );
};

export default Page;

export { celo as getServerSideProps } from 'nextjs/getServerSideProps/main';
