import type { NextPage } from 'next';
import React from 'react';

import PageNextJs from 'nextjs/PageNextJs';

import AdvancedFilter from 'ui/pages/AdvancedFilter';

const Page: NextPage = () => {
  return (
    <PageNextJs pathname="/advanced-filter">
      <AdvancedFilter/>
    </PageNextJs>
  );
};

export default Page;

export { advancedFilter as getServerSideProps } from 'nextjs/getServerSideProps/main';
