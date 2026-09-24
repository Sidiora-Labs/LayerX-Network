import * as regexp from 'toolkit/utils/regexp';

export default function urlParser(maybeUrl: string): URL | undefined {
  try {
    return constructUrl(maybeUrl);
  } catch (error) {}
}

function constructUrl(maybeUrl: string) {
  if (regexp.IPFS_PREFIX.test(maybeUrl)) {
    return new URL(maybeUrl.replace(regexp.IPFS_PREFIX, 'https://ipfs.io/ipfs/'));
  }

  if (regexp.URL_PREFIX.test(maybeUrl)) {
    return new URL(maybeUrl);
  }
}
