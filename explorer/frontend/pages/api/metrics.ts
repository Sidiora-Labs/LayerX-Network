import type { NextApiRequest, NextApiResponse } from 'next';
import * as promClient from 'prom-client';

const isEnabled = process.env.PROMETHEUS_METRICS_ENABLED === 'true';

isEnabled && promClient.collectDefaultMetrics({ prefix: 'frontend_' });

export default async function metricsHandler(req: NextApiRequest, res: NextApiResponse) {
  if (!isEnabled) {
    res.status(404).json({ error: 'Not found' });
    return;
  }

  const metrics = await promClient.register.metrics();
  res.setHeader('Content-type', promClient.register.contentType);
  res.send(metrics);
}
