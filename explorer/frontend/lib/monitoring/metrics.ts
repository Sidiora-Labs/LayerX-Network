import * as promClient from 'prom-client';

const metrics = (() => {
  // eslint-disable-next-line no-restricted-properties
  if (process.env.PROMETHEUS_METRICS_ENABLED !== 'true') {
    return;
  }

  promClient.register.clear();

  const invalidApiSchema = new promClient.Counter({
    name: 'invalid_api_schema',
    help: 'Number of invalid external API schema events',
    labelNames: [ 'resource', 'url' ] as const,
  });

  const socialPreviewBotRequests = new promClient.Counter({
    name: 'social_preview_bot_requests_total',
    help: 'Number of incoming requests from social preview bots',
    labelNames: [ 'route', 'bot' ] as const,
  });

  const searchEngineBotRequests = new promClient.Counter({
    name: 'search_engine_bot_requests_total',
    help: 'Number of incoming requests from search engine bots',
    labelNames: [ 'route', 'bot' ] as const,
  });

  const apiRequestDuration = new promClient.Histogram({
    name: 'api_request_duration_seconds',
    help: 'Duration of requests to API in seconds',
    labelNames: [ 'route', 'code' ],
    buckets: [ 0.2, 0.5, 1, 3, 10 ],
  });

  return { invalidApiSchema, socialPreviewBotRequests, searchEngineBotRequests, apiRequestDuration };
})();

export default metrics;
