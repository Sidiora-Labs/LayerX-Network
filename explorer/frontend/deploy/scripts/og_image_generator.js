/* eslint-disable no-console */
import fs from 'fs';
import path from 'path';

console.log('🎨 Generating OG image...');

const targetFile = path.resolve(process.cwd(), 'public/static/og_image.png');

function copyPlaceholderImage() {
  const sourceFile = path.resolve(process.cwd(), 'public/static/og_placeholder.png');
  fs.copyFileSync(sourceFile, targetFile);
}

if (process.env.NEXT_PUBLIC_OG_IMAGE_URL) {
  console.log('⏩ NEXT_PUBLIC_OG_IMAGE_URL is set. Skipping OG image generation...');
} else if (!process.env.NEXT_PUBLIC_NETWORK_NAME) {
  console.log('⏩ NEXT_PUBLIC_NETWORK_NAME is not set. Copying placeholder image...');
  copyPlaceholderImage();
} else if (!process.env.NEXT_PUBLIC_NETWORK_LOGO && !process.env.NEXT_PUBLIC_HOMEPAGE_HERO_BANNER_CONFIG) {
  console.log('⏩ Neither NEXT_PUBLIC_NETWORK_LOGO nor NEXT_PUBLIC_HOMEPAGE_HERO_BANNER_CONFIG is set. Copying placeholder image...');
  copyPlaceholderImage();
} else {
  try {
    const bannerConfig = JSON.parse(process.env.NEXT_PUBLIC_HOMEPAGE_HERO_BANNER_CONFIG?.replaceAll('\'', '"') || '{}');
    const data = {
      title: `${ process.env.NEXT_PUBLIC_NETWORK_NAME } explorer`,
      logo_url: process.env.NEXT_PUBLIC_NETWORK_LOGO_DARK ?? process.env.NEXT_PUBLIC_NETWORK_LOGO,
      background: bannerConfig.background?.[0],
      title_color: bannerConfig.text_color?.[0],
      invert_logo: !process.env.NEXT_PUBLIC_NETWORK_LOGO_DARK,
      app_url: process.env.NEXT_PUBLIC_APP_HOST,
    };

    console.log('⏳ Making request to OG image generator service...');

    const controller = new AbortController();
    const timeoutId = setTimeout(() => controller.abort(), 30_000);

    const response = await fetch('https://bigs.services.blockscout.com/generate/og', {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
      },
      body: JSON.stringify(data),
      signal: controller.signal,
    });

    clearTimeout(timeoutId);

    if (response.ok) {
      console.log('⬇️  Downloading the image...');
      const buffer = await response.arrayBuffer();
      const imageBuffer = Buffer.from(buffer);
      fs.writeFileSync(targetFile, imageBuffer);
    } else {
      const payload = response.headers.get('Content-type')?.includes('application/json') ? await response.json() : await response.text();
      console.error('🛑 Failed to generate OG image. Response:', payload);
      console.log('Copying placeholder image...');
      copyPlaceholderImage();
    }
  } catch (error) {
    console.error('🛑 Failed to generate OG image. Error:', error?.message);
    console.log('Copying placeholder image...');
    copyPlaceholderImage();
  }
}

console.log('✅ Done.');
