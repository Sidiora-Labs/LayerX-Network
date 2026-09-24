import { useColorModeValue } from 'toolkit/chakra/color-mode';

import * as themes from './themes';

export default function useThemeColors() {
  const theme = useColorModeValue(themes.light, themes.dark);

  return theme.colors;
}
