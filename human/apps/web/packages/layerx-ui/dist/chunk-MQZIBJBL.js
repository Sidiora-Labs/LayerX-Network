"use client";
import {
  OptionList
} from "./chunk-NAQ2BI4Y.js";
import {
  CalendarRangePicker,
  dayPickerClassNames,
  dayPickerModifiersClassNames
} from "./chunk-SNSZPVVW.js";
import {
  Popover,
  PopoverContent,
  PopoverTrigger
} from "./chunk-XNUMZ4ML.js";
import {
  Sheet,
  SheetBody,
  SheetFooter,
  SheetHeader
} from "./chunk-MJDZCIQM.js";
import {
  usePlatform
} from "./chunk-XORHQGZG.js";
import {
  Button
} from "./chunk-4X2DK7Y3.js";
import {
  cn
} from "./chunk-LXFZWLUU.js";

// src/components/filters.tsx
import * as React from "react";
import { ChevronDown, ListFilter } from "lucide-react";
import { DayPicker } from "react-day-picker";
import { Fragment, jsx, jsxs } from "react/jsx-runtime";
function isFilterActive(v) {
  if (!v) return false;
  if (typeof v === "string") return v.length > 0 && v !== "all";
  return Boolean(v.from);
}
function filterSummary(def, v) {
  if (!isFilterActive(v)) return null;
  if (def.type === "options" && typeof v === "string") {
    return def.options?.find((o) => o.value === v)?.label ?? null;
  }
  return "Custom range";
}
function FilterBar({
  filters,
  values,
  onChange,
  platform,
  portalContainer,
  className
}) {
  const resolved = usePlatform(platform);
  const [sheetOpen, setSheetOpen] = React.useState(false);
  const [draft, setDraft] = React.useState(values);
  const appliedCount = filters.filter((f) => isFilterActive(values[f.id])).length;
  const openSheet = () => {
    setDraft(values);
    setSheetOpen(true);
  };
  if (resolved === "mobile") {
    return /* @__PURE__ */ jsxs(Fragment, { children: [
      /* @__PURE__ */ jsxs(
        "button",
        {
          type: "button",
          onClick: openSheet,
          className: cn(
            "flex h-11 items-center gap-2 rounded-full border border-border bg-surface px-4 text-sm font-semibold text-foreground-secondary transition-colors hover:bg-surface-sunken/60",
            className
          ),
          children: [
            /* @__PURE__ */ jsx(ListFilter, { className: "size-4", "aria-hidden": true }),
            "Filter",
            appliedCount > 0 && /* @__PURE__ */ jsx("span", { className: "inline-flex h-5 min-w-5 items-center justify-center rounded-full bg-accent px-1.5 text-xs font-bold text-accent-foreground", children: appliedCount })
          ]
        }
      ),
      /* @__PURE__ */ jsxs(Sheet, { open: sheetOpen, onOpenChange: setSheetOpen, portalContainer, children: [
        /* @__PURE__ */ jsx(SheetHeader, { title: "Filter" }),
        /* @__PURE__ */ jsx(SheetBody, { className: "flex flex-col gap-6", children: filters.map((def) => /* @__PURE__ */ jsxs("section", { className: "flex flex-col gap-1", children: [
          /* @__PURE__ */ jsx("h4", { className: "pb-1 text-[15px] font-bold text-foreground", children: def.label }),
          def.type === "options" ? /* @__PURE__ */ jsx(
            OptionList,
            {
              "aria-label": def.label,
              items: def.options ?? [],
              value: draft[def.id] ?? def.options?.[0]?.value ?? "",
              onValueChange: (v) => setDraft((d) => ({ ...d, [def.id]: v }))
            }
          ) : /* @__PURE__ */ jsx("div", { className: "rounded-md border border-border p-2", children: /* @__PURE__ */ jsx(
            DayPicker,
            {
              mode: "range",
              numberOfMonths: 1,
              selected: draft[def.id] ?? void 0,
              onSelect: (r) => setDraft((d) => ({ ...d, [def.id]: r })),
              classNames: { ...dayPickerClassNames, root: "w-full text-sm text-foreground" },
              modifiersClassNames: dayPickerModifiersClassNames
            }
          ) })
        ] }, def.id)) }),
        /* @__PURE__ */ jsxs(SheetFooter, { children: [
          /* @__PURE__ */ jsx(
            Button,
            {
              variant: "secondary",
              size: "lg",
              onClick: () => {
                const cleared = {};
                filters.forEach((f) => cleared[f.id] = f.type === "options" ? "all" : void 0);
                setDraft(cleared);
                onChange(cleared);
                setSheetOpen(false);
              },
              children: "Clear"
            }
          ),
          /* @__PURE__ */ jsx(
            Button,
            {
              size: "lg",
              onClick: () => {
                onChange(draft);
                setSheetOpen(false);
              },
              children: "Apply"
            }
          )
        ] })
      ] })
    ] });
  }
  return /* @__PURE__ */ jsx("div", { className: cn("flex flex-wrap items-center gap-2", className), children: filters.map((def) => {
    const v = values[def.id];
    const summary = filterSummary(def, v);
    const active = isFilterActive(v);
    if (def.type === "date-range") {
      return /* @__PURE__ */ jsx(
        CalendarRangePicker,
        {
          value: v ?? void 0,
          onChange: (r) => onChange({ ...values, [def.id]: r }),
          placeholder: def.label
        },
        def.id
      );
    }
    return /* @__PURE__ */ jsxs(Popover, { children: [
      /* @__PURE__ */ jsx(PopoverTrigger, { asChild: true, children: /* @__PURE__ */ jsxs(
        "button",
        {
          type: "button",
          className: cn(
            "flex h-10 items-center gap-2 rounded-full border px-4 text-sm font-semibold transition-colors outline-none focus-visible:ring-2 focus-visible:ring-accent/30",
            active ? "border-accent/40 bg-accent-soft text-accent-strong" : "border-border bg-surface text-foreground-secondary hover:bg-surface-sunken/60"
          ),
          children: [
            summary ?? def.label,
            /* @__PURE__ */ jsx(ChevronDown, { className: "size-4 opacity-60", "aria-hidden": true })
          ]
        }
      ) }),
      /* @__PURE__ */ jsx(PopoverContent, { className: "w-[240px] p-2", children: /* @__PURE__ */ jsx(
        OptionList,
        {
          "aria-label": def.label,
          items: def.options ?? [],
          value: v ?? "all",
          onValueChange: (nv) => onChange({ ...values, [def.id]: nv })
        }
      ) })
    ] }, def.id);
  }) });
}

export {
  isFilterActive,
  FilterBar
};
//# sourceMappingURL=chunk-MQZIBJBL.js.map