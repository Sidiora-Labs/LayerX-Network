import type { NextPage } from 'next';
import dynamic from 'next/dynamic';
import React from 'react';

import PageNextJs from 'nextjs/PageNextJs';

const NameServices = dynamic(() => import('ui/pages/NameServices'), { ssr: false });

const Page: NextPage = () => {
  return (
    <PageNextJs pathname="/name-services">
      <NameServices/>
    </PageNextJs>
  );
};

export default Page;

export { nameServices as getServerSideProps } from 'nextjs/getServerSideProps/main';
