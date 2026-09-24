import { uniq } from 'es-toolkit';

import config from 'configs/app';
import { isBrowser } from 'toolkit/utils/isBrowser';

const RECENT_KEYWORDS_LS_KEY = 'recent_search_keywords';
const MAX_KEYWORDS_NUMBER = 10;

const parseKeywordsArray = (keywordsStr: string) => {
  if (!keywordsStr) {
    return [];
  }

  try {
    const parsedResult = JSON.parse(keywordsStr);
    if (Array.isArray(parsedResult)) {
      return parsedResult;
    }
    return [];
  } catch (error) {
    return [];
  }
};

export function saveToRecentKeywords(value: string) {
  if (config.app.isPrivateMode) {
    return;
  }
  if (!value) {
    return;
  }

  const keywordsArr = getRecentSearchKeywords();
  const result = uniq([ value, ...keywordsArr ]).slice(0, MAX_KEYWORDS_NUMBER - 1);
  window.localStorage.setItem(RECENT_KEYWORDS_LS_KEY, JSON.stringify(result));
}

export function getRecentSearchKeywords(input?: string) {
  if (!isBrowser() || config.app.isPrivateMode) {
    return [];
  }
  const keywordsStr = window.localStorage.getItem(RECENT_KEYWORDS_LS_KEY) || '';
  const keywordsArr = parseKeywordsArray(keywordsStr) as Array<string>;
  if (!input) {
    return keywordsArr;
  }

  return keywordsArr.filter(kw => kw.includes(input));
}

export function removeRecentSearchKeyword(value: string) {

  const keywordsArr = getRecentSearchKeywords();
  const result = keywordsArr.filter(kw => kw !== value);
  window.localStorage.setItem(RECENT_KEYWORDS_LS_KEY, JSON.stringify(result));
}

export function clearRecentSearchKeywords() {
  window.localStorage.setItem(RECENT_KEYWORDS_LS_KEY, '');
}
