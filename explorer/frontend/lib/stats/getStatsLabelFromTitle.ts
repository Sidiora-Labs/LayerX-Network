export default function getStatsLabelFromTitle(title: string) {
  return title.replace(/\s*\([^)]*\)\s*$/, '');
}
