import { useBreakpointValue } from '@chakra-ui/react';

// The default behavior of useBreakpointValue was changed in the commit - https://github.com/chakra-ui/chakra-ui/commit/7f30a7b7eebae236b55fe639a202bbf354677143
// So, with ssr = true during the initial render it will return base value
// which can cause issues in some components (for example, in AdaptiveTabs while calculating tabs cut)
// Since we don't use SSR in our project, the default value should be false
export default function useIsMobile(ssr = false) {
  return useBreakpointValue({ base: true, lg: false }, { ssr });
}
