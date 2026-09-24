export default function compileValue(template: string, params: Record<string, string | Array<string> | undefined>) {
  const PLACEHOLDER_REGEX = /%(\w+)%/g;
  return template.replaceAll(PLACEHOLDER_REGEX, (match, p1) => {
    const value = params[p1];

    if (Array.isArray(value)) {
      return value.join(', ');
    }

    if (value === undefined) {
      return '';
    }

    return value;
  });
}
