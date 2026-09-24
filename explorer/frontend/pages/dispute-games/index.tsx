import type { NextPage } from 'next';
import dynamic from 'next/dynamic';
import React from 'react';

import PageNextJs from 'nextjs/PageNextJs';

const DisputeGames = dynamic(() => import('ui/pages/OptimisticL2DisputeGames'), { ssr: false });

const Page: NextPage = () => {
  return (
    <PageNextJs pathname="/dispute-games">
      <DisputeGames/>
    </PageNextJs>
  );
};

export default Page;

export { disputeGames as getServerSideProps } from 'nextjs/getServerSideProps/main';
