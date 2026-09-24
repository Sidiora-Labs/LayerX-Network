import type { BlockFieldId } from 'types/views/block';
import { BLOCK_FIELDS_IDS } from 'types/views/block';

import { getEnvValue, parseEnvJson } from 'configs/app/utils';

const blockHiddenFields = (() => {
  const parsedValue = parseEnvJson<Array<BlockFieldId>>(getEnvValue('NEXT_PUBLIC_VIEWS_BLOCK_HIDDEN_FIELDS')) || [];

  if (!Array.isArray(parsedValue)) {
    return undefined;
  }

  const result = BLOCK_FIELDS_IDS.reduce((result, item) => {
    result[item] = parsedValue.includes(item);
    return result;
  }, {} as Record<BlockFieldId, boolean>);

  return result;
})();

const config = Object.freeze({
  hiddenFields: blockHiddenFields,
  pendingUpdateAlertEnabled: getEnvValue('NEXT_PUBLIC_VIEWS_BLOCK_PENDING_UPDATE_ALERT_ENABLED') !== 'false',
});

export default config;
