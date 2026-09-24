import type { HTMLChakraProps } from '@chakra-ui/react';
import { Flex, Box } from '@chakra-ui/react';
import { useQuery } from '@tanstack/react-query';
import React from 'react';

import useFetch from 'lib/hooks/useFetch';
import { EmptyState } from 'toolkit/chakra/empty-state';
import { Tooltip } from 'toolkit/chakra/tooltip';
import { FilterInput } from 'toolkit/components/filters/FilterInput';
import { ContentLoader } from 'toolkit/components/loaders/ContentLoader';
import { useClipboard } from 'toolkit/hooks/useClipboard';
import DataFetchAlert from 'ui/shared/DataFetchAlert';
import type { IconName } from 'ui/shared/IconSvg';
import IconSvg from 'ui/shared/IconSvg';
import PageTitle from 'ui/shared/Page/PageTitle';

const formatFileSize = (fileSizeInBytes: number) => `${ (fileSizeInBytes / 1_024).toFixed(2) } Kb`;

interface IconInfo {
  name: string;
  file_size: number;
}

const Item = ({ name, file_size: fileSize, bgColor }: IconInfo & HTMLChakraProps<'div'>) => {
  const { hasCopied, copy } = useClipboard(name, 1000);
  const [ copied, setCopied ] = React.useState(false);

  React.useEffect(() => {
    if (hasCopied) {
      setCopied(true);
    } else {
      setCopied(false);
    }
  }, [ hasCopied ]);

  return (
    <Flex
      flexDir="column"
      alignItems="center"
      whiteSpace="pre-wrap"
      wordBreak="break-word"
      maxW="100px"
      textAlign="center"
      onClick={ copy }
      cursor="pointer"
    >
      <IconSvg name={ name.replace('.svg', '') as IconName } boxSize="100px" bgColor={ bgColor } borderRadius="base"/>
      <Tooltip content={ copied ? 'Copied' : 'Copy to clipboard' } open={ copied }>
        <Box fontWeight={ 500 } mt={ 2 }>{ name }</Box>
      </Tooltip>
      <Box color="text.secondary">{ formatFileSize(fileSize) }</Box>
    </Flex>
  );
};

const Sprite = () => {
  const [ searchTerm, setSearchTerm ] = React.useState('');

  const fetch = useFetch();
  const { data, isFetching, isError } = useQuery({
    queryKey: [ 'sprite' ],
    queryFn: () => {
      return fetch<Array<IconInfo>, unknown>('/icons/registry.json');
    },
  });

  const content = (() => {
    if (isFetching) {
      return <ContentLoader/>;
    }

    if (isError || !data || !Array.isArray(data)) {
      return <DataFetchAlert/>;
    }

    const items = data
      .filter((icon) => icon.name.includes(searchTerm))
      .sort((a, b) => a.name.localeCompare(b.name));

    if (items.length === 0) {
      return <EmptyState description="No icons found"/>;
    }

    return (
      <Flex flexWrap="wrap" fontSize="sm" columnGap={ 5 } rowGap={ 5 } justifyContent="flex-start">
        { items.map((item) => <Item key={ item.name } { ...item } bgColor={{ _light: 'blackAlpha.100', _dark: 'whiteAlpha.100' }}/>) }
      </Flex>
    );
  })();

  const total = React.useMemo(() => {
    if (!data || !Array.isArray(data)) {
      return;
    }
    return data.reduce((result, item) => {
      result.num++;
      result.fileSize += item.file_size;
      return result;
    }, { num: 0, fileSize: 0 });
  }, [ data ]);

  const searchInput = <FilterInput placeholder="Search by name..." onChange={ setSearchTerm } loading={ isFetching } minW={{ base: '100%', lg: '300px' }}/>;
  const totalEl = total ? <Box ml="auto">Items: { total.num } / Size: { formatFileSize(total.fileSize) }</Box> : null;

  const contentAfter = (
    <>
      { totalEl }
      { searchInput }
    </>
  );

  return (
    <div>
      <PageTitle title="SVG sprite 🥤" contentAfter={ contentAfter }/>
      { content }
    </div>
  );
};

export default React.memo(Sprite);
