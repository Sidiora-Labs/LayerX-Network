import type CspDev from 'csp-dev';
import { uniq } from 'es-toolkit';

export const KEY_WORDS = {
  BLOB: 'blob:',
  DATA: 'data:',
  NONE: '\'none\'',
  REPORT_SAMPLE: `'report-sample'`,
  SELF: '\'self\'',
  STRICT_DYNAMIC: `'strict-dynamic'`,
  UNSAFE_INLINE: '\'unsafe-inline\'',
  UNSAFE_EVAL: '\'unsafe-eval\'',
};

export function mergeDescriptors(...descriptors: Array<CspDev.DirectiveDescriptor>) {
  return descriptors.reduce((result, item) => {
    for (const _key in item) {
      const key = _key as CspDev.Directive;
      const value = item[key];

      if (!value) {
        continue;
      }

      if (result[key]) {
        result[key]?.push(...value);
      } else {
        result[key] = [ ...value ];
      }
    }

    return result;
  }, {} as CspDev.DirectiveDescriptor);
}

export function makePolicyString(policyDescriptor: CspDev.DirectiveDescriptor) {
  return Object.entries(policyDescriptor)
    .map(([ key, value ]) => {
      if (!value || value.length === 0) {
        return;
      }

      const uniqueValues = uniq(value);
      return [ key, uniqueValues.join(' ') ].join(' ');
    })
    .filter(Boolean)
    .join(';');
}
