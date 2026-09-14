"use strict";Object.defineProperty(exports, "__esModule", {value: true});"use client";

// src/lib/types.ts
function recencyOf(date, now = /* @__PURE__ */ new Date()) {
  const days = (now.getTime() - date.getTime()) / 864e5;
  if (days < 1) return "today";
  if (days < 7) return "week";
  return "month";
}



exports.recencyOf = recencyOf;
//# sourceMappingURL=chunk-UF45QVNG.cjs.map