import type { NextRequest, NextResponse } from 'next/server';

import config from 'configs/app';
import * as cookiesLib from 'lib/cookies';

export default function scamTokensMiddleware(req: NextRequest, res: NextResponse) {
  if (config.UI.views.token.hideScamTokensEnabled) {
    const showScamTokensCookie = req.cookies.get(cookiesLib.NAMES.SHOW_SCAM_TOKENS);

    if (!showScamTokensCookie) {
      res.cookies.set(cookiesLib.NAMES.SHOW_SCAM_TOKENS, 'false', cookiesLib.getDefaultAttributes());
    }
  }
}
