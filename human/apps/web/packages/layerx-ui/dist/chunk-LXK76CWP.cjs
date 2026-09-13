"use strict";Object.defineProperty(exports, "__esModule", {value: true}); function _optionalChain(ops) { let lastAccessLHS = undefined; let value = ops[0]; let i = 1; while (i < ops.length) { const op = ops[i]; const fn = ops[i + 1]; i += 2; if ((op === 'optionalAccess' || op === 'optionalCall') && value == null) { return undefined; } if (op === 'access' || op === 'optionalAccess') { lastAccessLHS = value; value = fn(value); } else if (op === 'call' || op === 'optionalCall') { value = fn((...args) => value.call(lastAccessLHS, ...args)); lastAccessLHS = undefined; } } return value; }"use client";




var _chunk7SUKWFMOcjs = require('./chunk-7SUKWFMO.cjs');


var _chunkMD6ORKN4cjs = require('./chunk-MD6ORKN4.cjs');

// src/components/calendar-range-picker.tsx
var _reactdaypicker = require('react-day-picker');
var _datefns = require('date-fns');
var _lucidereact = require('lucide-react');
var _jsxruntime = require('react/jsx-runtime');
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
  const label = _optionalChain([value, 'optionalAccess', _ => _.from]) && _optionalChain([value, 'optionalAccess', _2 => _2.to]) ? `${_datefns.format.call(void 0, value.from, "MMM d, yyyy")} \u2013 ${_datefns.format.call(void 0, value.to, "MMM d, yyyy")}` : _optionalChain([value, 'optionalAccess', _3 => _3.from]) ? `${_datefns.format.call(void 0, value.from, "MMM d, yyyy")} \u2013 \u2026` : placeholder;
  return /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, _chunk7SUKWFMOcjs.Popover, { children: [
    /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _chunk7SUKWFMOcjs.PopoverTrigger, { asChild: true, children: /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, 
      "button",
      {
        type: "button",
        className: _chunkMD6ORKN4cjs.cn.call(void 0, 
          "flex h-11 items-center justify-between gap-2 rounded-md border border-border bg-surface px-3.5 text-sm font-medium transition-colors",
          "hover:bg-surface-sunken/50 focus-visible:ring-2 focus-visible:ring-accent/30 outline-none",
          _optionalChain([value, 'optionalAccess', _4 => _4.from]) ? "text-foreground" : "text-faint-foreground",
          className
        ),
        children: [
          /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "span", { className: "flex items-center gap-2", children: [
            /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _lucidereact.CalendarDays, { className: "size-4 text-muted-foreground", "aria-hidden": true }),
            label
          ] }),
          /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _lucidereact.ChevronDown, { className: "size-4 text-faint-foreground", "aria-hidden": true })
        ]
      }
    ) }),
    /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _chunk7SUKWFMOcjs.PopoverContent, { className: "p-3", align: "end", children: /* @__PURE__ */ _jsxruntime.jsx.call(void 0, 
      _reactdaypicker.DayPicker,
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





exports.dayPickerClassNames = dayPickerClassNames; exports.dayPickerModifiersClassNames = dayPickerModifiersClassNames; exports.CalendarRangePicker = CalendarRangePicker;
//# sourceMappingURL=chunk-LXK76CWP.cjs.map