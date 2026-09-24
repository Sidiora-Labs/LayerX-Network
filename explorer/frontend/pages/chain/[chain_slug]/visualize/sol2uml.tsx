import type { NextPage } from 'next';
import React from 'react';

import PageNextJs from 'nextjs/PageNextJs';

import { MultichainProvider } from 'lib/contexts/multichain';
import Sol2Uml from 'ui/pages/Sol2Uml';

const Page: NextPage = () => {
  return (
    <PageNextJs pathname="/visualize/sol2uml">
      <MultichainProvider>
        <Sol2Uml/>
      </MultichainProvider>
    </PageNextJs>
  );
};

export default Page;

export { base as getServerSideProps } from 'nextjs/getServerSideProps/multichain';
