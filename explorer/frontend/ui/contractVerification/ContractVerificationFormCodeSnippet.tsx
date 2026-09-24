import { Code } from '@chakra-ui/react';
import React from 'react';

interface Props {
  code: string;
}

const ContractVerificationFormCodeSnippet = ({ code }: Props) => {
  return (
    <Code whiteSpace="pre-wrap" wordBreak="break-all" p={ 2 } borderRadius="base">
      { code }
    </Code>
  );
};

export default React.memo(ContractVerificationFormCodeSnippet);
