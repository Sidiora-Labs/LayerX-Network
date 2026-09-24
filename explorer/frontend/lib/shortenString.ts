export default function shortenString(string: string | null, charNumber: number | undefined = 8) {
  if (!string) {
    return '';
  }

  if (string.length <= charNumber) {
    return string;
  }

  const tailLength = charNumber < 8 ? 2 : 4;

  return string.slice(0, charNumber - tailLength) + '...' + string.slice(-tailLength);
}
