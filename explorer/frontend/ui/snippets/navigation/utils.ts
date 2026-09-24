import type { NavItem } from 'types/client/navigation';

import config from 'configs/app';
import { isInternalItem } from 'lib/hooks/useNavItems';

export function checkRouteHighlight(item: NavItem | Array<NavItem> | Array<Array<NavItem>>): boolean {
  if (Array.isArray(item)) {
    return item.some((subItem) => checkRouteHighlight(subItem));
  }
  return isInternalItem(item) && (config.UI.navigation.highlightedRoutes.includes(item.nextRoute.pathname));
}
