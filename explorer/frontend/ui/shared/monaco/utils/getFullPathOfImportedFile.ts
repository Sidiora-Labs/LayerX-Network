import { stripLeadingSlash, stripTrailingSlash } from 'toolkit/utils/url';

// FIXME support multiline imports - https://base-goerli.blockscout.com/address/0x3442844D5d4938CA70f8C227dB88F6069C0b82A9?tab=contract

export default function getFullPathOfImportedFile(baseFilePath: string, importedFilePath: string, compilerRemappings?: Array<string>) {
  if (importedFilePath[0] !== '.') {
    let result = importedFilePath;

    // how remappings work - https://docs.soliditylang.org/en/v0.8.13/path-resolution.html#import-remapping
    if (compilerRemappings && compilerRemappings.length > 0) {
      const remappings = formatCompilerRemappings(compilerRemappings);

      const { prefix, target } = remappings.find(({ context, prefix }) => {
        if (context) {
          const contextPart = '/' + stripLeadingSlash(stripTrailingSlash(context));
          return baseFilePath.startsWith(contextPart + '/') && importedFilePath.startsWith(prefix);
        }

        return importedFilePath.startsWith(prefix);
      }) || {};

      if (prefix && target) {
        result = importedFilePath.replace(prefix, target);
      }
    }

    return result[0] === '/' ? result : '/' + result;
  }

  const baseFileChunks = stripLeadingSlash(baseFilePath).split('/');
  const importedFileChunks = importedFilePath.split('/');

  const result: Array<string> = baseFileChunks.slice(0, -1);

  for (let index = 0; index < importedFileChunks.length; index++) {
    const element = importedFileChunks[index];

    if (element === '.') {
      continue;
    }

    if (element === '..') {
      if (result.length === 0) {
        break;
      }
      result.pop();
      continue;
    }

    result.push(element);
  }

  if (result.length === 0) {
    return;
  }

  return '/' + result.join('/');
}

interface Remapping {
  context?: string;
  prefix: string;
  target: string;
}

function formatCompilerRemappings(remappings: Array<string>): Array<Remapping> {
  return remappings.map((item) => {
    const chunks = item.split(':');

    const [ prefix, target ] = chunks[chunks.length - 1].split('=');
    return {
      context: chunks.length > 1 ? chunks[0] : undefined,
      prefix,
      target,
    };
  });
}
