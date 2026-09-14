"use strict";Object.defineProperty(exports, "__esModule", {value: true}); function _interopRequireWildcard(obj) { if (obj && obj.__esModule) { return obj; } else { var newObj = {}; if (obj != null) { for (var key in obj) { if (Object.prototype.hasOwnProperty.call(obj, key)) { newObj[key] = obj[key]; } } } newObj.default = obj; return newObj; } } function _nullishCoalesce(lhs, rhsFn) { if (lhs != null) { return lhs; } else { return rhsFn(); } } function _optionalChain(ops) { let lastAccessLHS = undefined; let value = ops[0]; let i = 1; while (i < ops.length) { const op = ops[i]; const fn = ops[i + 1]; i += 2; if ((op === 'optionalAccess' || op === 'optionalCall') && value == null) { return undefined; } if (op === 'access' || op === 'optionalAccess') { lastAccessLHS = value; value = fn(value); } else if (op === 'call' || op === 'optionalCall') { value = fn((...args) => value.call(lastAccessLHS, ...args)); lastAccessLHS = undefined; } } return value; }"use client";


var _chunkIOJALOHMcjs = require('./chunk-IOJALOHM.cjs');




var _chunkLXK76CWPcjs = require('./chunk-LXK76CWP.cjs');




var _chunk7SUKWFMOcjs = require('./chunk-7SUKWFMO.cjs');





var _chunkKJXR3TMYcjs = require('./chunk-KJXR3TMY.cjs');


var _chunkI62LU2PGcjs = require('./chunk-I62LU2PG.cjs');


var _chunkRA3A4XJ2cjs = require('./chunk-RA3A4XJ2.cjs');


var _chunkMD6ORKN4cjs = require('./chunk-MD6ORKN4.cjs');

// src/components/filters.tsx
var _react = require('react'); var React = _interopRequireWildcard(_react);
var _lucidereact = require('lucide-react');
var _reactdaypicker = require('react-day-picker');
var _jsxruntime = require('react/jsx-runtime');
function isFilterActive(v) {
  if (!v) return false;
  if (typeof v === "string") return v.length > 0 && v !== "all";
  return Boolean(v.from);
}
function filterSummary(def, v) {
  if (!isFilterActive(v)) return null;
  if (def.type === "options" && typeof v === "string") {
    return _nullishCoalesce(_optionalChain([def, 'access', _ => _.options, 'optionalAccess', _2 => _2.find, 'call', _3 => _3((o) => o.value === v), 'optionalAccess', _4 => _4.label]), () => ( null));
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
  const resolved = _chunkI62LU2PGcjs.usePlatform.call(void 0, platform);
  const [sheetOpen, setSheetOpen] = React.useState(false);
  const [draft, setDraft] = React.useState(values);
  const appliedCount = filters.filter((f) => isFilterActive(values[f.id])).length;
  const openSheet = () => {
    setDraft(values);
    setSheetOpen(true);
  };
  if (resolved === "mobile") {
    return /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, _jsxruntime.Fragment, { children: [
      /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, 
        "button",
        {
          type: "button",
          onClick: openSheet,
          className: _chunkMD6ORKN4cjs.cn.call(void 0, 
            "flex h-11 items-center gap-2 rounded-full border border-border bg-surface px-4 text-sm font-semibold text-foreground-secondary transition-colors hover:bg-surface-sunken/60",
            className
          ),
          children: [
            /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _lucidereact.ListFilter, { className: "size-4", "aria-hidden": true }),
            "Filter",
            appliedCount > 0 && /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "span", { className: "inline-flex h-5 min-w-5 items-center justify-center rounded-full bg-accent px-1.5 text-xs font-bold text-accent-foreground", children: appliedCount })
          ]
        }
      ),
      /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, _chunkKJXR3TMYcjs.Sheet, { open: sheetOpen, onOpenChange: setSheetOpen, portalContainer, children: [
        /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _chunkKJXR3TMYcjs.SheetHeader, { title: "Filter" }),
        /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _chunkKJXR3TMYcjs.SheetBody, { className: "flex flex-col gap-6", children: filters.map((def) => /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "section", { className: "flex flex-col gap-1", children: [
          /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "h4", { className: "pb-1 text-[15px] font-bold text-foreground", children: def.label }),
          def.type === "options" ? /* @__PURE__ */ _jsxruntime.jsx.call(void 0, 
            _chunkIOJALOHMcjs.OptionList,
            {
              "aria-label": def.label,
              items: _nullishCoalesce(def.options, () => ( [])),
              value: _nullishCoalesce(_nullishCoalesce(draft[def.id], () => ( _optionalChain([def, 'access', _5 => _5.options, 'optionalAccess', _6 => _6[0], 'optionalAccess', _7 => _7.value]))), () => ( "")),
              onValueChange: (v) => setDraft((d) => ({ ...d, [def.id]: v }))
            }
          ) : /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "div", { className: "rounded-md border border-border p-2", children: /* @__PURE__ */ _jsxruntime.jsx.call(void 0, 
            _reactdaypicker.DayPicker,
            {
              mode: "range",
              numberOfMonths: 1,
              selected: _nullishCoalesce(draft[def.id], () => ( void 0)),
              onSelect: (r) => setDraft((d) => ({ ...d, [def.id]: r })),
              classNames: { ..._chunkLXK76CWPcjs.dayPickerClassNames, root: "w-full text-sm text-foreground" },
              modifiersClassNames: _chunkLXK76CWPcjs.dayPickerModifiersClassNames
            }
          ) })
        ] }, def.id)) }),
        /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, _chunkKJXR3TMYcjs.SheetFooter, { children: [
          /* @__PURE__ */ _jsxruntime.jsx.call(void 0, 
            _chunkRA3A4XJ2cjs.Button,
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
          /* @__PURE__ */ _jsxruntime.jsx.call(void 0, 
            _chunkRA3A4XJ2cjs.Button,
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
  return /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "div", { className: _chunkMD6ORKN4cjs.cn.call(void 0, "flex flex-wrap items-center gap-2", className), children: filters.map((def) => {
    const v = values[def.id];
    const summary = filterSummary(def, v);
    const active = isFilterActive(v);
    if (def.type === "date-range") {
      return /* @__PURE__ */ _jsxruntime.jsx.call(void 0, 
        _chunkLXK76CWPcjs.CalendarRangePicker,
        {
          value: _nullishCoalesce(v, () => ( void 0)),
          onChange: (r) => onChange({ ...values, [def.id]: r }),
          placeholder: def.label
        },
        def.id
      );
    }
    return /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, _chunk7SUKWFMOcjs.Popover, { children: [
      /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _chunk7SUKWFMOcjs.PopoverTrigger, { asChild: true, children: /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, 
        "button",
        {
          type: "button",
          className: _chunkMD6ORKN4cjs.cn.call(void 0, 
            "flex h-10 items-center gap-2 rounded-full border px-4 text-sm font-semibold transition-colors outline-none focus-visible:ring-2 focus-visible:ring-accent/30",
            active ? "border-accent/40 bg-accent-soft text-accent-strong" : "border-border bg-surface text-foreground-secondary hover:bg-surface-sunken/60"
          ),
          children: [
            _nullishCoalesce(summary, () => ( def.label)),
            /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _lucidereact.ChevronDown, { className: "size-4 opacity-60", "aria-hidden": true })
          ]
        }
      ) }),
      /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _chunk7SUKWFMOcjs.PopoverContent, { className: "w-[240px] p-2", children: /* @__PURE__ */ _jsxruntime.jsx.call(void 0, 
        _chunkIOJALOHMcjs.OptionList,
        {
          "aria-label": def.label,
          items: _nullishCoalesce(def.options, () => ( [])),
          value: _nullishCoalesce(v, () => ( "all")),
          onValueChange: (nv) => onChange({ ...values, [def.id]: nv })
        }
      ) })
    ] }, def.id);
  }) });
}




exports.isFilterActive = isFilterActive; exports.FilterBar = FilterBar;
//# sourceMappingURL=chunk-QC6WVVGU.cjs.map