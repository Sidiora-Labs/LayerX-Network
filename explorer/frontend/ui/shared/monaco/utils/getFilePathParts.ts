export default function getFilePathParts(path: string, tabsPathChunks: Array<Array<string>>): [string, string | undefined] {
  const chunks = path.split('/');
  const fileName = chunks[chunks.length - 1];
  const folderName = getFolderName(chunks, tabsPathChunks);

  return [ fileName, folderName ];
}

function getFolderName(chunks: Array<string>, tabsPathChunks: Array<Array<string>>): string | undefined {
  const fileName = chunks[chunks.length - 1];
  const otherTabsPathChunks = tabsPathChunks.filter((item) => item.join('/') !== chunks.join('/'));
  const tabsWithSameFileName = otherTabsPathChunks.filter((tabChunks) => tabChunks[tabChunks.length - 1] === fileName);

  if (tabsWithSameFileName.length === 0 || chunks.length <= 1) {
    return;
  }

  if (chunks.length === 2) {
    return './' + chunks[chunks.length - 2];
  }

  let result = '/' + chunks[chunks.length - 2];

  for (let index = 3; index <= chunks.length; index++) {
    const element = chunks[chunks.length - index];

    if (element === '') {
      result = '.' + result;
    }

    const subFolderNames = tabsWithSameFileName.map((tab) => tab[tab.length - index]);
    if (subFolderNames.includes(element)) {
      result = '/' + element + result;
    } else {
      break;
    }
  }

  return result;
}
