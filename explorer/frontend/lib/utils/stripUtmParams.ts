import config from 'configs/app';

const UTM_PARAMS = [ 'utm_source', 'utm_medium', 'utm_campaign', 'utm_term', 'utm_content' ];

export default function stripUtmParams(url: string): string {
  if (!config.app.isPrivateMode) {
    return url;
  }

  try {
    const urlObj = new URL(url);
    UTM_PARAMS.forEach((param) => {
      urlObj.searchParams.delete(param);
    });
    return urlObj.toString();
  } catch (error) {
    // If URL parsing fails, return original URL
    return url;
  }
}
