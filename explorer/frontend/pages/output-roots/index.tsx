import type { NextPage } from 'next';
import dynamic from 'next/dynamic';
import React from 'react';

import PageNextJs from 'nextjs/PageNextJs';

const OutputRoots = dynamic(() => import('ui/pages/OptimisticL2OutputRoots'), { ssr: false });

const Page: NextPage = () => {
  return (
    <PageNextJs pathname="/output-roots">
      <OutputRoots/>
    </PageNextJs>
  );
};

export default Page;

export { outputRoots as getServerSideProps } from 'nextjs/getServerSideProps/main';
