"use strict";Object.defineProperty(exports, "__esModule", {value: true}); function _nullishCoalesce(lhs, rhsFn) { if (lhs != null) { return lhs; } else { return rhsFn(); } }"use client";

// src/lib/format.ts
function formatMoney(value, opts = {}) {
  const { currency, signed = true, decimals = 2, locale = "en-US" } = opts;
  const symbol = _nullishCoalesce(opts.symbol, () => ( (currency === void 0 ? "$" : "")));
  const abs = Math.abs(value);
  const num = abs.toLocaleString(locale, {
    minimumFractionDigits: decimals,
    maximumFractionDigits: decimals
  });
  const sign = value < 0 ? "- " : signed && value > 0 ? "+ " : "";
  const cur = currency ? ` ${currency}` : "";
  return `${sign}${symbol}${num}${cur}`;
}
function formatBalance(value, symbol = "$") {
  return `${symbol} ${value.toLocaleString("en-US", {
    minimumFractionDigits: 2,
    maximumFractionDigits: 2
  })}`;
}
function formatRecency(date, now = /* @__PURE__ */ new Date()) {
  const mins = Math.round((now.getTime() - date.getTime()) / 6e4);
  if (mins < 1) return "now";
  if (mins < 60) return `${mins}m ago`;
  const hours = Math.round(mins / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.round(hours / 24);
  return `${days}d ago`;
}
function monthBandLabel(date) {
  return date.toLocaleDateString("en-US", { month: "long", year: "numeric" });
}
function downloadCsv(filename, header, rows) {
  const escape = (v) => {
    const s = String(v);
    return /[",\n]/.test(s) ? `"${s.replace(/"/g, '""')}"` : s;
  };
  const csv = [header, ...rows].map((r) => r.map(escape).join(",")).join("\n");
  const blob = new Blob([csv], { type: "text/csv;charset=utf-8" });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = filename;
  a.click();
  URL.revokeObjectURL(url);
}







exports.formatMoney = formatMoney; exports.formatBalance = formatBalance; exports.formatRecency = formatRecency; exports.monthBandLabel = monthBandLabel; exports.downloadCsv = downloadCsv;
//# sourceMappingURL=chunk-W6TE4RUR.cjs.map