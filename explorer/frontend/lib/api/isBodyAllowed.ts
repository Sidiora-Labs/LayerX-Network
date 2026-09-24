export default function isBodyAllowed(method: string | undefined | null) {
  return method && ![ 'GET', 'HEAD' ].includes(method);
}
