import http from 'k6/http';
import { check, fail } from 'k6';
import exec from 'k6/execution';
import { SharedArray } from 'k6/data';

export const options = {
  scenarios: {
    hosted_gateway: {
      executor: 'ramping-arrival-rate',
      startRate: Number(__ENV.LAYERX_GATEWAY_LOAD_START_RATE || 10),
      timeUnit: '1s',
      preAllocatedVUs: Number(__ENV.LAYERX_GATEWAY_LOAD_PREALLOCATED_VUS || 50),
      maxVUs: Number(__ENV.LAYERX_GATEWAY_LOAD_MAX_VUS || 500),
      stages: [
        { target: Number(__ENV.LAYERX_GATEWAY_LOAD_TARGET_RATE || 100), duration: __ENV.LAYERX_GATEWAY_LOAD_RAMP || '2m' },
        { target: Number(__ENV.LAYERX_GATEWAY_LOAD_TARGET_RATE || 100), duration: __ENV.LAYERX_GATEWAY_LOAD_HOLD || '10m' },
      ],
    },
  },
  thresholds: {
    http_req_failed: [`rate<${__ENV.LAYERX_GATEWAY_MAX_ERROR_RATE || '0.001'}`],
    http_req_duration: [`p(99)<${__ENV.LAYERX_GATEWAY_P99_MS || '750'}`],
    checks: ['rate>0.999'],
  },
};

const baseUrl = (__ENV.LAYERX_GATEWAY_URL || '').replace(/\/$/, '');
const activities = new SharedArray('signed hosted activities', () => {
  if (!__ENV.LAYERX_GATEWAY_ACTIVITY_CORPUS) return [];
  return JSON.parse(open(__ENV.LAYERX_GATEWAY_ACTIVITY_CORPUS));
});

export function setup() {
  if (!baseUrl || !__ENV.LAYERX_GATEWAY_SESSION || !__ENV.LAYERX_GATEWAY_SIGNER_PUBLIC_KEY || activities.length === 0) {
    fail('real gateway URL, session, signer key and signed activity corpus are required');
  }
  const issued = http.post(`${baseUrl}/v1/keys`, JSON.stringify({
    signer_public_key: __ENV.LAYERX_GATEWAY_SIGNER_PUBLIC_KEY,
    quota_requests: Number(__ENV.LAYERX_GATEWAY_LOAD_QUOTA || 1000000),
    quota_window_seconds: Number(__ENV.LAYERX_GATEWAY_LOAD_WINDOW_SECONDS || 3600),
  }), {
    headers: {
      Authorization: `Bearer ${__ENV.LAYERX_GATEWAY_SESSION}`,
      'Content-Type': 'application/json',
      'Idempotency-Key': `load-key-${Date.now()}`,
    },
  });
  if (issued.status !== 201) fail(`key issue refused with ${issued.status}`);
  const body = issued.json();
  return { authorization: `LayerX-Key ${body.key.id}:${body.key.secret}` };
}

export default function (data) {
  const activity = activities[exec.scenario.iterationInTest];
  if (!activity || !activity.activity || !activity.idempotency_key) {
    fail('signed activity corpus was exhausted or malformed');
  }
  const submitted = http.post(`${baseUrl}/v1/activities`, JSON.stringify({ activity: activity.activity }), {
    headers: {
      Authorization: data.authorization,
      'Content-Type': 'application/json',
      'Idempotency-Key': activity.idempotency_key,
    },
  });
  check(submitted, {
    'activity has verified synchronous result': (response) => response.status === 200 && Boolean(response.json('result.receipt')),
  });
  if (submitted.status !== 200) return;
  const activityId = submitted.json('result.activity_id');
  const receipt = http.get(`${baseUrl}/v1/receipts/${activityId}`, { headers: { Authorization: data.authorization } });
  check(receipt, {
    'receipt lookup is contract-identical': (response) => response.status === 200 && response.json('result.activity_id') === activityId && Boolean(response.json('result.receipt')),
  });
}
