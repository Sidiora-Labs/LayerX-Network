export default function getQueryParamString(param: string | Array<string> | undefined): string {
  if (Array.isArray(param)) {
    return param.join(',');
  }

  return param || '';
}
