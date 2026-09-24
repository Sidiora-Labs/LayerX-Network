import type { NextPage } from 'next';
import dynamic from 'next/dynamic';
import React from 'react';

import type { Props } from 'nextjs/getServerSideProps/handlers';
import PageNextJs from 'nextjs/PageNextJs';

const BlockCountdown = dynamic(() => import('ui/pages/BlockCountdown'), { ssr: false });

const Page: NextPage<Props> = (props: Props) => {
  return (
    <PageNextJs pathname="/block/countdown/[height]" query={ props.query }>
      <BlockCountdown/>
    </PageNextJs>
  );
};

export default Page;

export { block as getServerSideProps } from 'nextjs/getServerSideProps/main';
