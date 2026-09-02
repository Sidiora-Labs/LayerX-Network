import type * as url from "node:url";

declare global {
  type URLPatternInput = url.URLPatternInput;
  interface URLPatternOptions extends url.URLPatternOptions {}
}

export {};
