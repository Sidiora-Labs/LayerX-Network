import { chakra } from '@chakra-ui/react';
import React from 'react';

import type { IconName } from 'ui/shared/IconSvg';
import IconSvg from 'ui/shared/IconSvg';

interface Props {
  className?: string;
  fileName: string;
}

const CodeEditorFileIcon = ({ className, fileName }: Props) => {
  const name: IconName = (() => {
    if (/.vy$/.test(fileName)) {
      return 'monaco/vyper';
    }

    if (/.sol|.yul$/.test(fileName)) {
      return 'monaco/solidity';
    }

    if (/.rs$/.test(fileName)) {
      return 'monaco/rust';
    }

    if (/^Cargo\./.test(fileName)) {
      return 'monaco/cargo';
    }

    if (/.toml$/.test(fileName)) {
      return 'monaco/toml';
    }

    return 'monaco/file';
  })();

  return <IconSvg className={ className } name={ name } boxSize="16px"/>;
};

export default React.memo(chakra(CodeEditorFileIcon));
