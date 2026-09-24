export default function getErrorMessage(error: Record<string, Array<string>> | undefined, field: string) {
  return error?.[field]?.join(', ');
}
