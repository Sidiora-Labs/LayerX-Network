import type { IncomingMessage } from 'http';

type SocialPreviewBot = 'twitter' | 'facebook' | 'telegram' | 'slack';
type SearchEngineBot = 'google' | 'bing' | 'yahoo' | 'duckduckgo';

type ReturnType = {
  type: 'social_preview';
  bot: SocialPreviewBot;
} | {
  type: 'search_engine';
  bot: SearchEngineBot;
} | undefined;

export default function detectBotRequest(req: IncomingMessage): ReturnType {
  const userAgent = req.headers['user-agent'];

  if (!userAgent) {
    return;
  }

  if (userAgent.toLowerCase().includes('twitter')) {
    return { type: 'social_preview', bot: 'twitter' };
  }

  if (userAgent.toLowerCase().includes('facebook')) {
    return { type: 'social_preview', bot: 'facebook' };
  }

  if (userAgent.toLowerCase().includes('telegram')) {
    return { type: 'social_preview', bot: 'telegram' };
  }

  if (userAgent.toLowerCase().includes('slack')) {
    return { type: 'social_preview', bot: 'slack' };
  }

  if (userAgent.toLowerCase().includes('googlebot')) {
    return { type: 'search_engine', bot: 'google' };
  }

  if (userAgent.toLowerCase().includes('bingbot')) {
    return { type: 'search_engine', bot: 'bing' };
  }

  if (userAgent.toLowerCase().includes('yahoo')) {
    return { type: 'search_engine', bot: 'yahoo' };
  }

  if (userAgent.toLowerCase().includes('duckduck')) {
    return { type: 'search_engine', bot: 'duckduckgo' };
  }
}
