import type { NextPage } from 'next';
import dynamic from 'next/dynamic';
import React from 'react';

import type { Props } from 'nextjs/getServerSideProps/handlers';
import PageNextJs from 'nextjs/PageNextJs';

const Block = dynamic(() => import('ui/pages/Block'), { ssr: false });

const Page: NextPage<Props> = (props: Props) => {
  return (
    <PageNextJs pathname="/block/[height_or_hash]" query={ props.query }>
      <Block/>
    </PageNextJs>
  );
};

export default Page;

export { block as getServerSideProps } from 'nextjs/getServerSideProps/main';
