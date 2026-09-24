// Mock for @specify-sh/sdk to avoid build issues in Playwright component tests

export const ImageFormat = {
  SHORT_BANNER: 'short_banner',
  LONG_BANNER: 'long_banner',
};

export default class Specify {
  constructor(options) {
    this.publisherKey = options.publisherKey;
  }

  async serve() {
    // Mock implementation that returns null to simulate no ad
    return null;
  }
}

export const SpecifyAd = {
  imageUrl: '',
  headline: '',
  ctaUrl: '',
};
