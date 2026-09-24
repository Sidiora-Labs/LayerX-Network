import config from 'configs/app';
import NavigationDesktop from 'ui/snippets/navigation/vertical/NavigationDesktop';

const EmptyComponent = () => null;

export default config.UI.navigation.layout === 'horizontal' ? EmptyComponent : NavigationDesktop;
