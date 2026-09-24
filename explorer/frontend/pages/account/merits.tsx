import type { NextPage } from 'next';
import dynamic from 'next/dynamic';
import React from 'react';

import PageNextJs from 'nextjs/PageNextJs';

const RewardsDashboard = dynamic(() => import('ui/pages/RewardsDashboard'), { ssr: false });

const Page: NextPage = () => {
  return (
    <PageNextJs pathname="/account/merits">
      <RewardsDashboard/>
    </PageNextJs>
  );
};

export default Page;

export { account as getServerSideProps } from 'nextjs/getServerSideProps/main';
