import type { NextPage } from 'next';
import dynamic from 'next/dynamic';
import React from 'react';

import PageNextJs from 'nextjs/PageNextJs';

const WatchList = dynamic(() => import('ui/pages/Watchlist'), { ssr: false });

const Page: NextPage = () => {
  return (
    <PageNextJs pathname="/account/watchlist">
      <WatchList/>
    </PageNextJs>
  );
};

export default Page;

export { account as getServerSideProps } from 'nextjs/getServerSideProps/main';
