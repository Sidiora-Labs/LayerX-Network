"use strict";Object.defineProperty(exports, "__esModule", {value: true}); function _interopRequireWildcard(obj) { if (obj && obj.__esModule) { return obj; } else { var newObj = {}; if (obj != null) { for (var key in obj) { if (Object.prototype.hasOwnProperty.call(obj, key)) { newObj[key] = obj[key]; } } } newObj.default = obj; return newObj; } } function _nullishCoalesce(lhs, rhsFn) { if (lhs != null) { return lhs; } else { return rhsFn(); } }"use client";




var _chunkZY7BNJKWcjs = require('./chunk-ZY7BNJKW.cjs');


var _chunk6RJVNFD3cjs = require('./chunk-6RJVNFD3.cjs');


var _chunkI62LU2PGcjs = require('./chunk-I62LU2PG.cjs');



var _chunkW6TE4RURcjs = require('./chunk-W6TE4RUR.cjs');


var _chunkR7YNCUV3cjs = require('./chunk-R7YNCUV3.cjs');


var _chunkRA3A4XJ2cjs = require('./chunk-RA3A4XJ2.cjs');


var _chunkMD6ORKN4cjs = require('./chunk-MD6ORKN4.cjs');

// src/components/money-list.tsx
var _react = require('react'); var React = _interopRequireWildcard(_react);
var _lucidereact = require('lucide-react');

var _jsxruntime = require('react/jsx-runtime');
function defaultLeading(item) {
  const incoming = item.amount >= 0;
  return /* @__PURE__ */ _jsxruntime.jsx.call(void 0,
    _chunkZY7BNJKWcjs.IconTile,
    {
      shape: "circle",
      tone: incoming ? "accent" : "neutral",
      className: "size-10 [&_svg]:size-4",
      children: incoming ? /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _lucidereact.ArrowDownLeft, {}) : /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _lucidereact.ArrowUpRight, {})
    }
  );
}
function statusTone(status) {
  if (!status) return "neutral";
  const s = status.toLowerCase();
  if (s === "settled" || s === "completed" || s === "active") return "success";
  if (s === "failed" || s === "blocked") return "destructive";
  return "neutral";
}
function MoneyBands({
  groups,
  onItemClick
}) {
  return /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "div", { className: "flex flex-col gap-5", children: groups.map((g) => /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "section", { children: [
    /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "div", { className: "flex items-baseline justify-between pb-1", children: [
      /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "h4", { className: "text-sm font-bold text-muted-foreground", children: g.label }),
      /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "span", { className: "text-sm font-semibold tabular-nums text-foreground-secondary", children: _chunkW6TE4RURcjs.formatMoney.call(void 0, g.subtotal, { currency: g.currency, signed: false }) })
    ] }),
    /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _chunkZY7BNJKWcjs.List, { children: g.items.map((item) => /* @__PURE__ */ _jsxruntime.jsx.call(void 0,
      _chunkZY7BNJKWcjs.ListItem,
      {
        leading: _nullishCoalesce(item.leading, () => ( defaultLeading(item))),
        title: item.title,
        subtitle: item.status ? /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "span", { className: "flex items-center gap-1.5", children: [
          item.subtitle,
          /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _chunkR7YNCUV3cjs.Badge, { variant: statusTone(item.status), size: "sm", children: item.status })
        ] }) : item.subtitle,
        trailing: /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _chunk6RJVNFD3cjs.AmountText, { value: item.amount, currency: item.currency }),
        trailingCaption: item.date.toLocaleDateString("en-US", {
          day: "numeric",
          month: "short",
          year: "numeric"
        }),
        onClick: onItemClick ? () => onItemClick(item) : void 0
      },
      item.id
    )) })
  ] }, g.id)) });
}
function MoneyTable({
  groups,
  onItemClick,
  exportName = "transactions.csv",
  maxHeight = 560
}) {
  const [sortKey, setSortKey] = React.useState("date");
  const [sortDir, setSortDir] = React.useState("desc");
  const toggleSort = (key) => {
    if (key === sortKey) setSortDir((d) => d === "asc" ? "desc" : "asc");
    else {
      setSortKey(key);
      setSortDir("desc");
    }
  };
  const sortedGroups = React.useMemo(() => {
    const cmp = (a, b) => {
      let v = 0;
      if (sortKey === "date") v = a.date.getTime() - b.date.getTime();
      if (sortKey === "title") v = a.title.localeCompare(b.title);
      if (sortKey === "amount") v = a.amount - b.amount;
      return sortDir === "asc" ? v : -v;
    };
    return groups.map((g) => ({ ...g, items: [...g.items].sort(cmp) }));
  }, [groups, sortKey, sortDir]);
  const doExport = () => {
    const rows = groups.flatMap(
      (g) => g.items.map((i) => [
        g.label,
        i.title,
        _nullishCoalesce(i.subtitle, () => ( "")),
        _nullishCoalesce(i.status, () => ( "")),
        i.date.toISOString().slice(0, 10),
        i.amount.toFixed(2),
        _nullishCoalesce(i.currency, () => ( ""))
      ])
    );
    _chunkW6TE4RURcjs.downloadCsv.call(void 0, exportName, ["Group", "Title", "Subtitle", "Status", "Date", "Amount", "Currency"], rows);
  };
  const SortButton = ({ id, children }) => /* @__PURE__ */ _jsxruntime.jsxs.call(void 0,
    "button",
    {
      type: "button",
      onClick: () => toggleSort(id),
      className: _chunkMD6ORKN4cjs.cn.call(void 0,
        "inline-flex items-center gap-1 text-xs font-bold tracking-wide uppercase transition-colors",
        sortKey === id ? "text-foreground" : "text-faint-foreground hover:text-muted-foreground"
      ),
      children: [
        children,
        sortKey === id && (sortDir === "asc" ? /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _lucidereact.ArrowUpNarrowWide, { className: "size-3.5" }) : /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _lucidereact.ArrowDownWideNarrow, { className: "size-3.5" }))
      ]
    }
  );
  return /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "div", { className: "overflow-hidden rounded-lg border border-border bg-surface", children: [
    /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "div", { className: "flex items-center justify-between border-b border-border px-4 py-2.5", children: [
      /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "span", { className: "text-sm font-semibold text-foreground-secondary", children: [
        groups.reduce((n, g) => n + g.items.length, 0),
        " records"
      ] }),
      /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, _chunkRA3A4XJ2cjs.Button, { variant: "soft", size: "sm", onClick: doExport, children: [
        /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _lucidereact.Download, {}),
        "Export"
      ] })
    ] }),
    /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "div", { className: "lx-scroll overflow-y-auto", style: { maxHeight }, children: /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "table", { className: "w-full border-collapse text-sm", children: [
      /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "thead", { className: "sticky top-0 z-20 bg-surface", children: /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "tr", { className: "border-b border-border text-left", children: [
        /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "th", { className: "px-4 py-2.5 font-medium", children: /* @__PURE__ */ _jsxruntime.jsx.call(void 0, SortButton, { id: "title", children: "Description" }) }),
        /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "th", { className: "px-4 py-2.5 font-medium", children: /* @__PURE__ */ _jsxruntime.jsx.call(void 0, SortButton, { id: "date", children: "Date" }) }),
        /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "th", { className: "px-4 py-2.5 font-medium", children: "Status" }),
        /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "th", { className: "px-4 py-2.5 text-right font-medium", children: /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "span", { className: "inline-flex justify-end", children: /* @__PURE__ */ _jsxruntime.jsx.call(void 0, SortButton, { id: "amount", children: "Amount" }) }) })
      ] }) }),
      sortedGroups.map((g) => /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "tbody", { children: [
        /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "tr", { children: /* @__PURE__ */ _jsxruntime.jsx.call(void 0,
          "td",
          {
            colSpan: 4,
            className: "sticky top-[41px] z-10 border-b border-border bg-surface-sunken/70 px-4 py-1.5 backdrop-blur",
            children: /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "div", { className: "flex items-baseline justify-between", children: [
              /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "span", { className: "text-xs font-bold text-muted-foreground", children: g.label }),
              /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "span", { className: "text-xs font-semibold tabular-nums text-foreground-secondary", children: _chunkW6TE4RURcjs.formatMoney.call(void 0, g.subtotal, { currency: g.currency, signed: false }) })
            ] })
          }
        ) }),
        g.items.map((item) => /* @__PURE__ */ _jsxruntime.jsxs.call(void 0,
          "tr",
          {
            onClick: onItemClick ? () => onItemClick(item) : void 0,
            className: _chunkMD6ORKN4cjs.cn.call(void 0,
              "border-b border-border/60 transition-colors last:border-0 hover:bg-surface-sunken/40",
              onItemClick && "cursor-pointer"
            ),
            children: [
              /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "td", { className: "px-4 py-3", children: /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "div", { className: "flex items-center gap-3", children: [
                _nullishCoalesce(item.leading, () => ( defaultLeading(item))),
                /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "div", { className: "flex min-w-0 flex-col", children: [
                  /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "span", { className: "truncate font-semibold text-foreground", children: item.title }),
                  item.subtitle && /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "span", { className: "truncate text-xs text-muted-foreground", children: item.subtitle })
                ] })
              ] }) }),
              /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "td", { className: "px-4 py-3 whitespace-nowrap text-muted-foreground tabular-nums", children: item.date.toLocaleDateString("en-US", {
                day: "numeric",
                month: "short",
                year: "numeric"
              }) }),
              /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "td", { className: "px-4 py-3", children: item.status && /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _chunkR7YNCUV3cjs.Badge, { variant: statusTone(item.status), size: "sm", children: item.status }) }),
              /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "td", { className: "px-4 py-3 text-right", children: /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _chunk6RJVNFD3cjs.AmountText, { value: item.amount, currency: item.currency }) })
            ]
          },
          item.id
        ))
      ] }, g.id))
    ] }) })
  ] });
}
function MoneyList({
  groups,
  onItemClick,
  platform,
  exportName,
  className
}) {
  const resolved = _chunkI62LU2PGcjs.usePlatform.call(void 0, platform);
  return /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "div", { className, children: resolved === "mobile" ? /* @__PURE__ */ _jsxruntime.jsx.call(void 0, MoneyBands, { groups, onItemClick }) : /* @__PURE__ */ _jsxruntime.jsx.call(void 0, MoneyTable, { groups, onItemClick, exportName }) });
}



exports.MoneyList = MoneyList;
//# sourceMappingURL=chunk-PBMLJAZN.cjs.map