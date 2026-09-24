import { Text } from '@chakra-ui/react';

import { route } from 'nextjs-routes';

import { Button } from 'toolkit/chakra/button';
import { useColorModeValue } from 'toolkit/chakra/color-mode';
import { Image } from 'toolkit/chakra/image';
import { LinkOverlay, LinkBox } from 'toolkit/chakra/link';

type Props = {
  id: string;
  title: string;
  description: string;
  buttonText: string;
  imageUrl: string;
  darkImageUrl: string;
};

const EssentialDappCard = ({ id, title, description, buttonText, imageUrl, darkImageUrl }: Props) => {
  const imageSrc = useColorModeValue(imageUrl, darkImageUrl);

  return (
    <LinkBox
      display="flex"
      flexDirection="column"
      alignItems="flex-start"
      flex={{ base: 1, md: 'unset' }}
      minW="130px"
      maxW={{ base: '160px', md: '200px' }}
      w={{ base: 'auto', md: '200px' }}
      p={{ base: 3, md: 5 }}
      border="1px solid"
      borderColor={{ _light: 'blackAlpha.300', _dark: 'whiteAlpha.300' }}
      borderRadius="base"
      _hover={{ boxShadow: 'md' }}
      _focusWithin={{ boxShadow: 'md' }}
      className="group"
    >
      <Image src={ imageSrc } alt={ title } h={{ base: '37px', md: '50px' }} mb={ 6 }/>
      <Text textStyle={{ base: 'sm', md: 'xl' }} fontWeight="600" mb={ 2 }>{ title }</Text>
      <Text textStyle={{ base: 'xs', md: 'sm' }} mb={ 3 }>{ description }</Text>
      <LinkOverlay
        href={ route({ pathname: '/essential-dapps/[id]', query: { id } }) }
        w={{ base: 'full', md: 'auto' }}
        mt="auto"
      >
        <Button size="sm" variant="outline" w="full" _groupHover={{ borderColor: 'hover', color: 'hover' }}>
          { buttonText }
        </Button>
      </LinkOverlay>
    </LinkBox>
  );
};

export default EssentialDappCard;
