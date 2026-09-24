export default function getValuesArrayFromQuery(val: string | Array<string> | undefined) {
  if (val === undefined) {
    return;
  }

  const valArray = [];
  if (typeof val === 'string') {
    valArray.push(...val.split(','));
  }
  if (Array.isArray(val)) {
    if (!val.length) {
      return;
    }
    val.forEach(el => valArray.push(...el.split(',')));
  }

  return valArray;
}
