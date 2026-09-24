import type { NextApiRequest, NextApiResponse } from 'next';

import { httpLogger } from 'nextjs/utils/logger';

export default async function logHandler(req: NextApiRequest, res: NextApiResponse) {
  httpLogger(req, res);

  res.status(200).send('ok');
}
