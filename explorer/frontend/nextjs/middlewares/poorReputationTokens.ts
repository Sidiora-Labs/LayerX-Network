import type { NextRequest, NextResponse } from 'next/server';

import config from 'configs/app';
import * as cookiesLib from 'lib/cookies';

export default function poorReputationTokensMiddleware(req: NextRequest, res: NextResponse) {
  if (config.features.multichain.isEnabled) {
    const showPoorReputationTokensCookie = req.cookies.get(cookiesLib.NAMES.SHOW_POOR_REPUTATION_TOKENS);

    if (!showPoorReputationTokensCookie) {
      res.cookies.set(cookiesLib.NAMES.SHOW_POOR_REPUTATION_TOKENS, 'false', cookiesLib.getDefaultAttributes());
    }
  }
}
