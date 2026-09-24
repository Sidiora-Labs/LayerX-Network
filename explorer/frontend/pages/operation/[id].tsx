import type { NextPage } from 'next';
import dynamic from 'next/dynamic';
import React from 'react';

import type { Props } from 'nextjs/getServerSideProps/handlers';
import PageNextJs from 'nextjs/PageNextJs';

const TacOperation = dynamic(() => import('ui/pages/TacOperation'), { ssr: false });

const Page: NextPage<Props> = (props: Props) => {
  return (
    <PageNextJs pathname="/operation/[id]" query={ props.query }>
      <TacOperation/>
    </PageNextJs>
  );
};

export default Page;

export { tac as getServerSideProps } from 'nextjs/getServerSideProps/main';
