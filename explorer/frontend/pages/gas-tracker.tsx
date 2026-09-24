import type { NextPage } from 'next';
import dynamic from 'next/dynamic';
import React from 'react';

import PageNextJs from 'nextjs/PageNextJs';

const GasTracker = dynamic(() => import('ui/pages/GasTracker'), { ssr: false });

const Page: NextPage = () => {
  return (
    <PageNextJs pathname="/gas-tracker">
      <GasTracker/>
    </PageNextJs>
  );
};

export default Page;

export { gasTracker as getServerSideProps } from 'nextjs/getServerSideProps/main';
