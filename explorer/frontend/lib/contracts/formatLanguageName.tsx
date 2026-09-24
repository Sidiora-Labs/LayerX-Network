export default function formatLanguageName(language: string) {
  return language.replace(/_/g, ' ').replace(/\b\w/g, char => char.toUpperCase());
}
