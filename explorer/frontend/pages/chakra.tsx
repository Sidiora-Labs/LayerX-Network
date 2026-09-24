import type { NextPage } from 'next';
import React from 'react';

import PageNextJs from 'nextjs/PageNextJs';

import Chakra from 'ui/pages/Chakra';

const Page: NextPage = () => {
  return (
    <PageNextJs pathname="/chakra">
      <Chakra/>
    </PageNextJs>
  );
};

export default Page;

export { base as getServerSideProps } from 'nextjs/getServerSideProps/main';
