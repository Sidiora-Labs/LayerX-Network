import type { JsxStyleProps } from '@chakra-ui/react';
import { Text, VStack, HStack } from '@chakra-ui/react';
import React from 'react';

import type { HighlightsBannerConfig } from 'types/homepage';

import config from 'configs/app';
import { useColorModeValue } from 'toolkit/chakra/color-mode';
import { Heading } from 'toolkit/chakra/heading';
import { Image } from 'toolkit/chakra/image';
import { Link } from 'toolkit/chakra/link';
import { Skeleton } from 'toolkit/chakra/skeleton';

interface ContainerProps extends Omit<Props, 'totalNum'>, JsxStyleProps {
  children: React.ReactNode;
}

const Container = ({ children, data, isLoading, ...rest }: ContainerProps) => {
  if ('page_path' in data) {
    return (
      <Link href={ config.app.baseUrl + data.page_path } loading={ isLoading } { ...rest }>
        { children }
      </Link>
    );
  }

  if ('redirect_url' in data) {
    return (
      <Link href={ data.redirect_url } loading={ isLoading } external noIcon { ...rest }>
        { children }
      </Link>
    );
  }

  return (
    <Skeleton loading={ isLoading } { ...rest }>
      { children }
    </Skeleton>
  );
};

interface Props {
  data: HighlightsBannerConfig;
  isLoading: boolean;
  totalNum: number;
}

const HighlightsItem = ({ data, isLoading, totalNum }: Props) => {

  const imageSrc = useColorModeValue(
    data.side_img_url?.[0],
    data.side_img_url?.[1] || data.side_img_url?.[0],
  );

  return (
    <Container
      data={ data }
      isLoading={ isLoading }
      display="flex"
      alignItems="center"
      minH="153px"
      pl={ 6 }
      w={ `calc((100% - ${ (totalNum - 1) * 12 }px) / ${ totalNum })` }
      overflow="hidden"
      borderRadius="md"
      bg={ !isLoading ? {
        _light: data.background?.[0] || '#EFF7FF',
        _dark: data.background?.[1] || data.background?.[0] || '#2A3340',
      } : undefined }
    >
      <HStack overflow="hidden" w="100%" gap={ 3 }>
        <VStack
          alignItems="flex-start"
          gap={ 3 }
          w={ totalNum === 2 ? '294px' : '193px' }
          flexShrink={ 0 }
        >
          <Heading
            level="3"
            color={{
              _light: data.title_color?.[0] || '#101112',
              _dark: data.title_color?.[1] || data.title_color?.[0] || '#F8FCFF',
            }}
          >
            { data.title }
          </Heading>
          <Text
            textStyle="sm"
            color={{
              _light: data.description_color?.[0] || '#718096',
              _dark: data.description_color?.[1] || data.description_color?.[0] || '#AEB1B6',
            }}
          >
            { data.description }
          </Text>
        </VStack>
        { imageSrc && !isLoading && (
          <Image
            src={ imageSrc }
            width="210px"
            height="112px"
            skeletonWidth="0px"
            objectFit="cover"
            ml="auto"
            mr={ 6 }
          />
        ) }
      </HStack>
    </Container>
  );
};

export default React.memo(HighlightsItem);
