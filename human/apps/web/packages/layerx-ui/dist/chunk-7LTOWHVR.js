"use client";
import {
  IconTile,
  List,
  ListItem
} from "./chunk-EP5CBYSP.js";
import {
  AmountText
} from "./chunk-R5UMX4ZN.js";
import {
  usePlatform
} from "./chunk-XORHQGZG.js";
import {
  downloadCsv,
  formatMoney
} from "./chunk-WM3FOCWV.js";
import {
  Badge
} from "./chunk-KTG64LT6.js";
import {
  Button
} from "./chunk-4X2DK7Y3.js";
import {
  cn
} from "./chunk-LXFZWLUU.js";

// src/components/money-list.tsx
import * as React from "react";
import { ArrowDownWideNarrow, ArrowUpNarrowWide, Download } from "lucide-react";
import { ArrowDownLeft, ArrowUpRight } from "lucide-react";
import { jsx, jsxs } from "react/jsx-runtime";
function defaultLeading(item) {
  const incoming = item.amount >= 0;
  return /* @__PURE__ */ jsx(
    IconTile,
    {
      shape: "circle",
      tone: incoming ? "accent" : "neutral",
      className: "size-10 [&_svg]:size-4",
      children: incoming ? /* @__PURE__ */ jsx(ArrowDownLeft, {}) : /* @__PURE__ */ jsx(ArrowUpRight, {})
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
  return /* @__PURE__ */ jsx("div", { className: "flex flex-col gap-5", children: groups.map((g) => /* @__PURE__ */ jsxs("section", { children: [
    /* @__PURE__ */ jsxs("div", { className: "flex items-baseline justify-between pb-1", children: [
      /* @__PURE__ */ jsx("h4", { className: "text-sm font-bold text-muted-foreground", children: g.label }),
      /* @__PURE__ */ jsx("span", { className: "text-sm font-semibold tabular-nums text-foreground-secondary", children: formatMoney(g.subtotal, { currency: g.currency, signed: false }) })
    ] }),
    /* @__PURE__ */ jsx(List, { children: g.items.map((item) => /* @__PURE__ */ jsx(
      ListItem,
      {
        leading: item.leading ?? defaultLeading(item),
        title: item.title,
        subtitle: item.status ? /* @__PURE__ */ jsxs("span", { className: "flex items-center gap-1.5", children: [
          item.subtitle,
          /* @__PURE__ */ jsx(Badge, { variant: statusTone(item.status), size: "sm", children: item.status })
        ] }) : item.subtitle,
        trailing: /* @__PURE__ */ jsx(AmountText, { value: item.amount, currency: item.currency }),
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
        i.subtitle ?? "",
        i.status ?? "",
        i.date.toISOString().slice(0, 10),
        i.amount.toFixed(2),
        i.currency ?? ""
      ])
    );
    downloadCsv(exportName, ["Group", "Title", "Subtitle", "Status", "Date", "Amount", "Currency"], rows);
  };
  const SortButton = ({ id, children }) => /* @__PURE__ */ jsxs(
    "button",
    {
      type: "button",
      onClick: () => toggleSort(id),
      className: cn(
        "inline-flex items-center gap-1 text-xs font-bold tracking-wide uppercase transition-colors",
        sortKey === id ? "text-foreground" : "text-faint-foreground hover:text-muted-foreground"
      ),
      children: [
        children,
        sortKey === id && (sortDir === "asc" ? /* @__PURE__ */ jsx(ArrowUpNarrowWide, { className: "size-3.5" }) : /* @__PURE__ */ jsx(ArrowDownWideNarrow, { className: "size-3.5" }))
      ]
    }
  );
  return /* @__PURE__ */ jsxs("div", { className: "overflow-hidden rounded-lg border border-border bg-surface", children: [
    /* @__PURE__ */ jsxs("div", { className: "flex items-center justify-between border-b border-border px-4 py-2.5", children: [
      /* @__PURE__ */ jsxs("span", { className: "text-sm font-semibold text-foreground-secondary", children: [
        groups.reduce((n, g) => n + g.items.length, 0),
        " records"
      ] }),
      /* @__PURE__ */ jsxs(Button, { variant: "soft", size: "sm", onClick: doExport, children: [
        /* @__PURE__ */ jsx(Download, {}),
        "Export"
      ] })
    ] }),
    /* @__PURE__ */ jsx("div", { className: "lx-scroll overflow-y-auto", style: { maxHeight }, children: /* @__PURE__ */ jsxs("table", { className: "w-full border-collapse text-sm", children: [
      /* @__PURE__ */ jsx("thead", { className: "sticky top-0 z-20 bg-surface", children: /* @__PURE__ */ jsxs("tr", { className: "border-b border-border text-left", children: [
        /* @__PURE__ */ jsx("th", { className: "px-4 py-2.5 font-medium", children: /* @__PURE__ */ jsx(SortButton, { id: "title", children: "Description" }) }),
        /* @__PURE__ */ jsx("th", { className: "px-4 py-2.5 font-medium", children: /* @__PURE__ */ jsx(SortButton, { id: "date", children: "Date" }) }),
        /* @__PURE__ */ jsx("th", { className: "px-4 py-2.5 font-medium", children: "Status" }),
        /* @__PURE__ */ jsx("th", { className: "px-4 py-2.5 text-right font-medium", children: /* @__PURE__ */ jsx("span", { className: "inline-flex justify-end", children: /* @__PURE__ */ jsx(SortButton, { id: "amount", children: "Amount" }) }) })
      ] }) }),
      sortedGroups.map((g) => /* @__PURE__ */ jsxs("tbody", { children: [
        /* @__PURE__ */ jsx("tr", { children: /* @__PURE__ */ jsx(
          "td",
          {
            colSpan: 4,
            className: "sticky top-[41px] z-10 border-b border-border bg-surface-sunken/70 px-4 py-1.5 backdrop-blur",
            children: /* @__PURE__ */ jsxs("div", { className: "flex items-baseline justify-between", children: [
              /* @__PURE__ */ jsx("span", { className: "text-xs font-bold text-muted-foreground", children: g.label }),
              /* @__PURE__ */ jsx("span", { className: "text-xs font-semibold tabular-nums text-foreground-secondary", children: formatMoney(g.subtotal, { currency: g.currency, signed: false }) })
            ] })
          }
        ) }),
        g.items.map((item) => /* @__PURE__ */ jsxs(
          "tr",
          {
            onClick: onItemClick ? () => onItemClick(item) : void 0,
            className: cn(
              "border-b border-border/60 transition-colors last:border-0 hover:bg-surface-sunken/40",
              onItemClick && "cursor-pointer"
            ),
            children: [
              /* @__PURE__ */ jsx("td", { className: "px-4 py-3", children: /* @__PURE__ */ jsxs("div", { className: "flex items-center gap-3", children: [
                item.leading ?? defaultLeading(item),
                /* @__PURE__ */ jsxs("div", { className: "flex min-w-0 flex-col", children: [
                  /* @__PURE__ */ jsx("span", { className: "truncate font-semibold text-foreground", children: item.title }),
                  item.subtitle && /* @__PURE__ */ jsx("span", { className: "truncate text-xs text-muted-foreground", children: item.subtitle })
                ] })
              ] }) }),
              /* @__PURE__ */ jsx("td", { className: "px-4 py-3 whitespace-nowrap text-muted-foreground tabular-nums", children: item.date.toLocaleDateString("en-US", {
                day: "numeric",
                month: "short",
                year: "numeric"
              }) }),
              /* @__PURE__ */ jsx("td", { className: "px-4 py-3", children: item.status && /* @__PURE__ */ jsx(Badge, { variant: statusTone(item.status), size: "sm", children: item.status }) }),
              /* @__PURE__ */ jsx("td", { className: "px-4 py-3 text-right", children: /* @__PURE__ */ jsx(AmountText, { value: item.amount, currency: item.currency }) })
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
  const resolved = usePlatform(platform);
  return /* @__PURE__ */ jsx("div", { className, children: resolved === "mobile" ? /* @__PURE__ */ jsx(MoneyBands, { groups, onItemClick }) : /* @__PURE__ */ jsx(MoneyTable, { groups, onItemClick, exportName }) });
}

export {
  MoneyList
};
//# sourceMappingURL=chunk-7LTOWHVR.js.map