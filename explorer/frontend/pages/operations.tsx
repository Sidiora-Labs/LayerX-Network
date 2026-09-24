import type { NextPage } from 'next';
import dynamic from 'next/dynamic';
import React from 'react';

import PageNextJs from 'nextjs/PageNextJs';

const TacOperations = dynamic(() => import('ui/pages/TacOperations'), { ssr: false });

const Page: NextPage = () => {
  return (
    <PageNextJs pathname="/operations">
      <TacOperations/>
    </PageNextJs>
  );
};

export default Page;

export { tac as getServerSideProps } from 'nextjs/getServerSideProps/main';
