import type { NextPage } from 'next';
import React from 'react';

import PageNextJs from 'nextjs/PageNextJs';

import PublicTagsSubmit from 'ui/pages/PublicTagsSubmit';

const Page: NextPage = () => {
  return (
    <PageNextJs pathname="/public-tags/submit">
      <PublicTagsSubmit/>
    </PageNextJs>
  );
};

export default Page;

export { publicTagsSubmit as getServerSideProps } from 'nextjs/getServerSideProps/main';
