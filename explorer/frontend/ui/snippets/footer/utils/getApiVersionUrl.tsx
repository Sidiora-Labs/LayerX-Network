export default function getApiVersionUrl(version: string | undefined): string | undefined {
  if (!version) {
    return;
  }

  const [ tag, commit ] = version.split('.+commit.');

  if (commit) {
    return `https://github.com/blockscout/blockscout/commit/${ commit }`;
  }

  return `https://github.com/blockscout/blockscout/tree/${ tag }`;
}
