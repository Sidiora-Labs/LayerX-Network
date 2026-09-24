import { Flex, chakra } from '@chakra-ui/react';
import React from 'react';

import config from 'configs/app';
import { Button } from 'toolkit/chakra/button';
import { Image } from 'toolkit/chakra/image';
import { Link } from 'toolkit/chakra/link';
import { PopoverRoot, PopoverTrigger, PopoverContent, PopoverBody } from 'toolkit/chakra/popover';
import { Skeleton } from 'toolkit/chakra/skeleton';
import { useDisclosure } from 'toolkit/hooks/useDisclosure';
import IconSvg from 'ui/shared/IconSvg';

interface Props {
  className?: string;
  hash: string;
  isLoading?: boolean;
}

const ContractCodeIde = ({ className, hash, isLoading }: Props) => {
  const { open, onOpenChange } = useDisclosure();

  const ideLinks = React.useMemo(() => {
    return config.UI.ides.items
      .map((ide) => {
        const url = decodeURIComponent(ide.url.replace('{hash}', hash).replace('{domain}', config.app.host || ''));
        const icon = 'icon_url' in ide ?
          <Image boxSize={ 5 } mr={ 2 } src={ ide.icon_url } alt={ `${ ide.title } icon` }/> :
          <IconSvg name="ABI" boxSize={ 5 } color="icon.primary" mr={ 2 }/>;

        return (
          <Link external key={ ide.title } href={ url } display="inline-flex" alignItems="center">
            { icon }
            { ide.title }
          </Link>
        );
      });
  }, [ hash ]);

  if (isLoading) {
    return <Skeleton loading h={ 8 } w="92px" borderRadius="base"/>;
  }

  if (ideLinks.length === 0) {
    return null;
  }

  return (
    <PopoverRoot open={ open } onOpenChange={ onOpenChange }>
      <PopoverTrigger>
        <Button
          className={ className }
          size="sm"
          variant="dropdown"
          aria-label="Open source code in IDE"
          fontWeight={ 500 }
          gap={ 0 }
          h="32px"
          flexShrink={ 0 }
        >
          <span>Open in</span>
          <IconSvg name="arrows/east-mini" transform={ open ? 'rotate(90deg)' : 'rotate(-90deg)' } transitionDuration="faster" boxSize={ 5 }/>
        </Button>
      </PopoverTrigger>
      <PopoverContent w="240px">
        <PopoverBody >
          <chakra.span color="text.secondary" fontSize="xs">Redactors</chakra.span>
          <Flex
            flexDir="column"
            alignItems="flex-start"
            columnGap={ 6 }
            rowGap={ 3 }
            mt={ 3 }
          >
            { ideLinks }
          </Flex>
        </PopoverBody>
      </PopoverContent>
    </PopoverRoot>
  );
};

export default React.memo(chakra(ContractCodeIde));
