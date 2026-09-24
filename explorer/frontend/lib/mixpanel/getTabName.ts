import { capitalize } from 'es-toolkit';

export default function getTabName(tab: string) {
  return tab !== '' ? capitalize(tab.replaceAll('_', ' ')) : 'Default';
}
