"use client";
import {
  Popover,
  PopoverContent,
  PopoverTrigger
} from "./chunk-XNUMZ4ML.js";
import {
  cn
} from "./chunk-LXFZWLUU.js";

// src/components/calendar-range-picker.tsx
import { DayPicker } from "react-day-picker";
import { format } from "date-fns";
import { CalendarDays, ChevronDown } from "lucide-react";
import { jsx, jsxs } from "react/jsx-runtime";
var dayPickerClassNames = {
  root: "text-sm text-foreground",
  months: "flex gap-4",
  month_caption: "flex items-center justify-between px-1 pb-2 font-semibold",
  caption_label: "text-sm font-semibold",
  nav: "flex items-center gap-1",
  button_previous: "inline-flex size-7 items-center justify-center rounded-full hover:bg-surface-sunken",
  button_next: "inline-flex size-7 items-center justify-center rounded-full hover:bg-surface-sunken",
  weekdays: "flex",
  weekday: "w-9 flex-1 text-center text-xs font-medium text-faint-foreground py-1",
  week: "flex",
  day: "w-9 flex-1 p-0",
  day_button: "size-9 w-full rounded-full text-sm transition-colors outline-none hover:bg-surface-sunken",
  outside: "text-faint-foreground/60",
  disabled: "opacity-40"
};
var dayPickerModifiersClassNames = {
  today: "font-bold text-accent-strong",
  selected: "bg-accent text-accent-foreground hover:bg-accent",
  range_start: "rounded-full",
  range_end: "rounded-full",
  range_middle: "bg-accent-soft! text-foreground! rounded-none!"
};
function CalendarRangePicker({
  value,
  onChange,
  placeholder = "Select range",
  className
}) {
  const label = value?.from && value?.to ? `${format(value.from, "MMM d, yyyy")} \u2013 ${format(value.to, "MMM d, yyyy")}` : value?.from ? `${format(value.from, "MMM d, yyyy")} \u2013 \u2026` : placeholder;
  return /* @__PURE__ */ jsxs(Popover, { children: [
    /* @__PURE__ */ jsx(PopoverTrigger, { asChild: true, children: /* @__PURE__ */ jsxs(
      "button",
      {
        type: "button",
        className: cn(
          "flex h-11 items-center justify-between gap-2 rounded-md border border-border bg-surface px-3.5 text-sm font-medium transition-colors",
          "hover:bg-surface-sunken/50 focus-visible:ring-2 focus-visible:ring-accent/30 outline-none",
          value?.from ? "text-foreground" : "text-faint-foreground",
          className
        ),
        children: [
          /* @__PURE__ */ jsxs("span", { className: "flex items-center gap-2", children: [
            /* @__PURE__ */ jsx(CalendarDays, { className: "size-4 text-muted-foreground", "aria-hidden": true }),
            label
          ] }),
          /* @__PURE__ */ jsx(ChevronDown, { className: "size-4 text-faint-foreground", "aria-hidden": true })
        ]
      }
    ) }),
    /* @__PURE__ */ jsx(PopoverContent, { className: "p-3", align: "end", children: /* @__PURE__ */ jsx(
      DayPicker,
      {
        mode: "range",
        selected: value,
        onSelect: onChange,
        numberOfMonths: 1,
        classNames: dayPickerClassNames,
        modifiersClassNames: dayPickerModifiersClassNames
      }
    ) })
  ] });
}

export {
  dayPickerClassNames,
  dayPickerModifiersClassNames,
  CalendarRangePicker
};
//# sourceMappingURL=chunk-SNSZPVVW.js.map