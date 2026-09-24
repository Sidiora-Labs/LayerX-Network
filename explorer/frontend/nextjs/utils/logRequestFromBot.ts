import type { IncomingMessage, ServerResponse } from 'http';

import metrics from 'lib/monitoring/metrics';

import detectBotRequest from './detectBotRequest';

export default async function logRequestFromBot(req: IncomingMessage | undefined, res: ServerResponse<IncomingMessage> | undefined, pathname: string) {
  if (!req || !res || !metrics) {
    return;
  }

  const botInfo = detectBotRequest(req);

  if (!botInfo) {
    return;
  }

  switch (botInfo.type) {
    case 'search_engine': {
      metrics.searchEngineBotRequests.inc({ route: pathname, bot: botInfo.bot });
      return;
    }
    case 'social_preview': {
      metrics.socialPreviewBotRequests.inc({ route: pathname, bot: botInfo.bot });
      return;
    }
  }
}
