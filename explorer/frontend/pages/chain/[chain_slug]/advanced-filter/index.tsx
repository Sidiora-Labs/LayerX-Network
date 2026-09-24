import type { NextPage } from 'next';
import React from 'react';

import PageNextJs from 'nextjs/PageNextJs';

import { MultichainProvider } from 'lib/contexts/multichain';
import AdvancedFilter from 'ui/pages/AdvancedFilter';

const Page: NextPage = () => {
  return (
    <PageNextJs pathname="/chain/[chain_slug]/advanced-filter">
      <MultichainProvider>
        <AdvancedFilter/>
      </MultichainProvider>
    </PageNextJs>
  );
};

export default Page;

export { advancedFilter as getServerSideProps } from 'nextjs/getServerSideProps/multichain';
