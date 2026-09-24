import config from 'configs/app';

const maxWidthVerticalNavigation = config.UI.maxContentWidth ? 1_920 : 10_000;
const maxWidthHorizontalNavigation = config.UI.maxContentWidth ? 1_440 : 10_000;

export const CONTENT_MAX_WIDTH = config.UI.navigation.layout === 'horizontal' ? maxWidthHorizontalNavigation : maxWidthVerticalNavigation;
