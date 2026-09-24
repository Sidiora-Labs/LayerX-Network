import { stripLeadingSlash } from '../../../../utils/url';

// Function to get all files in drop directory
export async function getAllFileEntries(dataTransferItemList: DataTransferItemList): Promise<Array<FileSystemFileEntry>> {
  const fileEntries: Array<FileSystemFileEntry> = [];

  // Use BFS to traverse entire directory/file structure
  const queue: Array<FileSystemFileEntry | FileSystemDirectoryEntry> = [];

  // Unfortunately dataTransferItemList is not iterable i.e. no forEach
  for (let i = 0; i < dataTransferItemList.length; i++) {
    // Note webkitGetAsEntry a non-standard feature and may change
    // Usage is necessary for handling directories
    // + typescript types are kinda wrong - https://developer.mozilla.org/en-US/docs/Web/API/DataTransferItem/webkitGetAsEntry
    const item = dataTransferItemList[i].webkitGetAsEntry() as FileSystemFileEntry | FileSystemDirectoryEntry | null;
    item && queue.push(item);
  }

  while (queue.length > 0) {
    const entry = queue.shift();
    if (entry?.isFile) {
      fileEntries.push(entry as FileSystemFileEntry);
    } else if (entry?.isDirectory && 'createReader' in entry) {
      queue.push(...await readAllDirectoryEntries(entry.createReader()));
    }
  }
  return fileEntries;
}

// Get all the entries (files or sub-directories) in a directory
// by calling readEntries until it returns empty array
async function readAllDirectoryEntries(directoryReader: FileSystemDirectoryReader) {
  const entries: Array<FileSystemFileEntry> = [];
  let readEntries = await readEntriesPromise(directoryReader);

  while (readEntries && readEntries.length > 0) {
    entries.push(...readEntries);
    readEntries = await readEntriesPromise(directoryReader);
  }
  return entries;
}

// Wrap readEntries in a promise to make working with readEntries easier
// readEntries will return only some of the entries in a directory
// e.g. Chrome returns at most 100 entries at a time
async function readEntriesPromise(directoryReader: FileSystemDirectoryReader): Promise<Array<FileSystemFileEntry> | undefined> {
  try {
    return await new Promise((resolve, reject) => {
      directoryReader.readEntries(
        (fileEntry) => {
          resolve(fileEntry as Array<FileSystemFileEntry>);
        },
        reject,
      );
    });
  } catch (err) {}
}

export function convertFileEntryToFile(entry: FileSystemFileEntry, fullFilePath?: boolean): Promise<File> {
  return new Promise((resolve) => {
    entry.file(async(file: File) => {
      const newFile = fullFilePath ?
        new File([ file ], stripLeadingSlash(entry.fullPath), { lastModified: file.lastModified, type: file.type }) :
        file;
      resolve(newFile);
    });
  });
}
