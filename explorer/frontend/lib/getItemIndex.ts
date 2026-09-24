const DEFAULT_PAGE_SIZE = 50;

export default function getItemIndex(index: number, page: number, pageSize: number = DEFAULT_PAGE_SIZE) {
  return (page - 1) * pageSize + index + 1;
};
