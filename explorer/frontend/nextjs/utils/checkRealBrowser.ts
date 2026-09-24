import type { GetServerSidePropsContext } from 'next';

const GENERIC_BOT_MARKERS = [
  'bot',
  'spider',
  'crawler',
  'httpclient',
  'headlesschrome',
  'phantomjs',
  'puppeteer',
  'playwright',
  'node-fetch',
  'axios',
  'curl',
  'wget',
  'python-requests',
  'go-http-client',
  'libwww-perl',
  'okhttp',
  'postman',
  'insomnia',
];

function toLower(input: string | null | undefined): string {
  return (input || '').toLowerCase();
}

function isGenericBotUA(userAgent: string): boolean {
  const ua = toLower(userAgent);
  return GENERIC_BOT_MARKERS.some((m) => ua.includes(m));
}

function hasBrowserHeuristics(req: GetServerSidePropsContext['req']): boolean {
  const acceptLanguage = req.headers['accept-language'];
  const secChUa = req.headers['sec-ch-ua'];
  const secFetchSite = req.headers['sec-fetch-site'];
  const secFetchMode = req.headers['sec-fetch-mode'];
  const upgradeInsecure = req.headers['upgrade-insecure-requests'];

  const hasLang = Boolean(acceptLanguage);
  const hasSecHeaders = Boolean(secChUa || secFetchSite || secFetchMode || upgradeInsecure);

  return hasLang && hasSecHeaders;
}

export function isLikelyHumanBrowser(req: GetServerSidePropsContext['req']): boolean {
  const userAgent = req.headers['user-agent'];
  if (!userAgent) return false;

  if (isGenericBotUA(userAgent)) return false;
  if (!hasBrowserHeuristics(req)) return false;

  // Inclusive check for common engines + brands across desktop and mobile
  const hasEngineToken = /applewebkit|gecko/i.test(userAgent);
  const hasKnownBrowserBrand = /chrome|safari|firefox|crios|fxios|edg|opr|samsungbrowser|vivaldi|whale|duckduckgo/i.test(userAgent);

  return hasEngineToken && hasKnownBrowserBrand;
}

export function isKnownBotRequest(req: GetServerSidePropsContext['req']): boolean {
  const ua = toLower(req.headers['user-agent']);
  if (!ua) return false;

  // Social preview bots
  if (ua.includes('twitter')) return true;
  if (ua.includes('facebook')) return true;
  if (ua.includes('telegram')) return true;
  if (ua.includes('slack')) return true;

  // Search engine bots
  if (ua.includes('googlebot')) return true;
  if (ua.includes('bingbot')) return true;
  if (ua.includes('yahoo')) return true;
  if (ua.includes('duckduck')) return true;

  // Generic markers
  if (isGenericBotUA(ua)) return true;

  return false;
}
