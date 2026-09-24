import config from 'configs/app';
import NavigationDesktop from 'ui/snippets/navigation/horizontal/NavigationDesktop';

const EmptyComponent = () => null;

export default config.UI.navigation.layout === 'horizontal' ? NavigationDesktop : EmptyComponent;
