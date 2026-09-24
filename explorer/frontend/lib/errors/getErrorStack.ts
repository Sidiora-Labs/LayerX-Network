export default function getErrorStack(error: Error | undefined): string | undefined {
  return (
    error && 'stack' in error &&
    typeof error.stack === 'string' && error.stack !== null &&
    error.stack
  ) ||
    undefined;
}
