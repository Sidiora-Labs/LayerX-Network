import getPageType, { PAGE_TYPE_DICT } from './getPageType';
import logEvent from './logEvent';
import reset from './reset';
import useInit from './useInit';
import useLogPageView from './useLogPageView';
import * as userProfile from './userProfile';
export * from './utils';

export {
  useInit,
  useLogPageView,
  logEvent,
  getPageType,
  userProfile,
  reset,
  PAGE_TYPE_DICT,
};
