// ensure that path always starts with /
export default function formatFilePath(path: string) {
  if (path[0] === '.' && path[1] === '/') {
    return path.slice(1);
  }

  if (path[0] === '/') {
    return path;
  }

  return '/' + path;
}
