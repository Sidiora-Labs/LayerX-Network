import type { NextPage } from 'next';
import dynamic from 'next/dynamic';
import React from 'react';

import PageNextJs from 'nextjs/PageNextJs';

const PaxeerXAnchors = dynamic(() => import('ui/pages/PaxeerXAnchors'), { ssr: false });

const Page: NextPage = () => {
  return (
    <PageNextJs pathname="/paxeer-x/anchors">
      <PaxeerXAnchors/>
    </PageNextJs>
  );
};

export default Page;

export { paxeerXLists as getServerSideProps } from 'nextjs/getServerSideProps/main';
