import capitalizeFirstLetter from 'lib/capitalizeFirstLetter';

export function camelCaseToSentence(camelCaseString: string | undefined) {
  if (!camelCaseString) {
    return '';
  }

  let sentence = camelCaseString.replace(/([a-z])([A-Z])/g, '$1 $2');
  sentence = sentence.replace(/([A-Z])([A-Z][a-z])/g, '$1 $2');
  sentence = capitalizeFirstLetter(sentence);

  return sentence;
}
