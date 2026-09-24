import { unparse } from 'papaparse';

export function downloadBlob(blob: Blob, filename: string) {
  const url = URL.createObjectURL(blob);
  const link = document.createElement('a');
  link.setAttribute('href', url);
  link.setAttribute('download', filename);
  link.click();

  link.remove();
  URL.revokeObjectURL(url);
}

export function saveAsCsv(headerRows: Array<string>, dataRows: Array<Array<string>>, filename: string) {
  const csv = unparse([
    headerRows,
    ...dataRows,
  ]);

  const blob = new Blob([ csv ], { type: 'text/csv;charset=utf-8;' });

  downloadBlob(blob, filename);
}
