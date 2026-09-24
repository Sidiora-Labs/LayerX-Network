export default function capitalizeFirstLetter(text: string) {
  if (!text || !text.length) {
    return '';
  }

  return text.charAt(0).toUpperCase() + text.slice(1);
}
