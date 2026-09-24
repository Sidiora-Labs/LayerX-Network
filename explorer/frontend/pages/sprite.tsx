import type { NextPage } from 'next';
import React from 'react';

import PageNextJs from 'nextjs/PageNextJs';

import Sprite from 'ui/pages/Sprite';

const Page: NextPage = () => {
  return (
    <PageNextJs pathname="/sprite">
      <Sprite/>
    </PageNextJs>
  );
};

export default Page;

export { base as getServerSideProps } from 'nextjs/getServerSideProps/main';
