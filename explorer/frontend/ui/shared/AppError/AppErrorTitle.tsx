import React from 'react';

import { Heading } from 'toolkit/chakra/heading';

interface Props {
  title: string;
}

const AppErrorTitle = ({ title }: Props) => {
  return <Heading mt={ 8 } textStyle="heading.xl" as="h1">{ title }</Heading>;
};

export default AppErrorTitle;
