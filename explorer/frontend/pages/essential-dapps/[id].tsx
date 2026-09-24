import type { NextPage } from 'next';
import dynamic from 'next/dynamic';
import React from 'react';

import type { Props } from 'nextjs/getServerSideProps/handlers';
import PageNextJs from 'nextjs/PageNextJs';

const MarketplaceEssentialDapp = dynamic(() => import('ui/pages/MarketplaceEssentialDapp'), { ssr: false });

const Page: NextPage<Props> = (props: Props) => (
  <PageNextJs pathname="/essential-dapps/[id]" query={ props.query }>
    <MarketplaceEssentialDapp/>
  </PageNextJs>
);

export default Page;

export { marketplaceEssentialDapp as getServerSideProps } from 'nextjs/getServerSideProps/main';
