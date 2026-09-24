export const stripTrailingSlash = (str: string) => str[str.length - 1] === '/' ? str.slice(0, -1) : str;

export const stripLeadingSlash = (str: string) => str[0] === '/' ? str.slice(1) : str;

export function makePrettyLink(url: string | undefined): { href: string; domain: string } | undefined {
  try {
    const urlObj = new URL(url ?? '');
    return {
      href: urlObj.href,
      domain: urlObj.hostname,
    };
  } catch (error) {}
}
