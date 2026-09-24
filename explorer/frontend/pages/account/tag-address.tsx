import type { NextPage } from 'next';
import dynamic from 'next/dynamic';
import React from 'react';

import PageNextJs from 'nextjs/PageNextJs';

const PrivateTags = dynamic(() => import('ui/pages/PrivateTags'), { ssr: false });

const Page: NextPage = () => {
  return (
    <PageNextJs pathname="/account/tag-address">
      <PrivateTags/>
    </PageNextJs>
  );
};

export default Page;

export { account as getServerSideProps } from 'nextjs/getServerSideProps/main';
