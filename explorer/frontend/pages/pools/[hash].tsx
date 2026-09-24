import type { NextPage } from 'next';
import dynamic from 'next/dynamic';
import React from 'react';

import type { Props } from 'nextjs/getServerSideProps/handlers';
import PageNextJs from 'nextjs/PageNextJs';

const Pool = dynamic(() => import('ui/pages/Pool'), { ssr: false });

const Page: NextPage<Props> = (props: Props) => {
  return (
    <PageNextJs pathname="/pools/[hash]" query={ props.query }>
      <Pool/>
    </PageNextJs>
  );
};

export default Page;

export { pools as getServerSideProps } from 'nextjs/getServerSideProps/main';
