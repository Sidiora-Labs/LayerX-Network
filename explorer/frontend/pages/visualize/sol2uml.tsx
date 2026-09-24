import type { NextPage } from 'next';
import React from 'react';

import PageNextJs from 'nextjs/PageNextJs';

import Sol2Uml from 'ui/pages/Sol2Uml';

const Page: NextPage = () => {
  return (
    <PageNextJs pathname="/visualize/sol2uml">
      <Sol2Uml/>
    </PageNextJs>
  );
};

export default Page;

export { base as getServerSideProps } from 'nextjs/getServerSideProps/main';
