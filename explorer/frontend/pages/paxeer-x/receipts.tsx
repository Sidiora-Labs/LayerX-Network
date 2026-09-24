import type { NextPage } from 'next';
import dynamic from 'next/dynamic';
import React from 'react';

import PageNextJs from 'nextjs/PageNextJs';

const PaxeerXReceipts = dynamic(() => import('ui/pages/PaxeerXReceipts'), { ssr: false });

const Page: NextPage = () => {
  return (
    <PageNextJs pathname="/paxeer-x/receipts">
      <PaxeerXReceipts/>
    </PageNextJs>
  );
};

export default Page;

export { paxeerXLists as getServerSideProps } from 'nextjs/getServerSideProps/main';
