import * as yup from 'yup';
import { replaceQuotes } from 'configs/app/utils';
import type { ApiDocsTabId } from 'types/views/apiDocs';
import { API_DOCS_TABS } from 'types/views/apiDocs';

export const apiDocsSchema = yup
  .object()
  .shape({
    NEXT_PUBLIC_API_DOCS_TABS: yup.array()
      .transform(replaceQuotes)
      .json()
      .of(yup.string<ApiDocsTabId>().oneOf(API_DOCS_TABS)),
  });