"use strict";Object.defineProperty(exports, "__esModule", {value: true}); function _interopRequireWildcard(obj) { if (obj && obj.__esModule) { return obj; } else { var newObj = {}; if (obj != null) { for (var key in obj) { if (Object.prototype.hasOwnProperty.call(obj, key)) { newObj[key] = obj[key]; } } } newObj.default = obj; return newObj; } } function _nullishCoalesce(lhs, rhsFn) { if (lhs != null) { return lhs; } else { return rhsFn(); } }"use client";


var _chunkR7YNCUV3cjs = require('./chunk-R7YNCUV3.cjs');


var _chunkMD6ORKN4cjs = require('./chunk-MD6ORKN4.cjs');

// src/components/bank-card.tsx
var _react = require('react'); var React = _interopRequireWildcard(_react);
var _jsxruntime = require('react/jsx-runtime');
function BankCard({ data, className }) {
  const dark = data.theme === "dark";
  return /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, 
    "div",
    {
      className: _chunkMD6ORKN4cjs.cn.call(void 0, 
        "relative flex aspect-[8/5] w-full flex-col justify-between overflow-hidden rounded-lg p-5 shadow-card",
        dark ? "bg-[#101418] text-white" : "bg-[linear-gradient(135deg,#eef3fa_0%,#e2eaf5_55%,#dbe5f2_100%)] text-foreground",
        className
      ),
      children: [
        /* @__PURE__ */ _jsxruntime.jsx.call(void 0, 
          "div",
          {
            "aria-hidden": true,
            className: _chunkMD6ORKN4cjs.cn.call(void 0, 
              "pointer-events-none absolute inset-0",
              dark ? "bg-[radial-gradient(120%_90%_at_80%_0%,rgb(255_255_255/0.08),transparent_60%)]" : "bg-[radial-gradient(120%_90%_at_80%_0%,rgb(255_255_255/0.7),transparent_60%)]"
            )
          }
        ),
        /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "div", { className: "relative flex items-start justify-between gap-3", children: [
          /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "span", { className: "text-[13px] font-bold tracking-[0.12em] uppercase", children: data.holder }),
          data.status && /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _chunkR7YNCUV3cjs.Badge, { variant: _nullishCoalesce(data.status.tone, () => ( "success")), size: "sm", className: "bg-surface/80", children: data.status.label })
        ] }),
        /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "div", { className: "relative flex items-end justify-between gap-4", children: [
          /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "div", { className: "flex flex-col gap-1", children: [
            /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "span", { className: _chunkMD6ORKN4cjs.cn.call(void 0, "text-xs", dark ? "text-white/60" : "text-muted-foreground"), children: _nullishCoalesce(data.kind, () => ( "Virtual card")) }),
            /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "span", { className: "text-[15px] font-semibold tracking-[0.06em] tabular-nums", children: data.number })
          ] }),
          /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "div", { className: "flex flex-col items-end gap-1", children: [
            /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "span", { className: _chunkMD6ORKN4cjs.cn.call(void 0, "text-xs", dark ? "text-white/60" : "text-muted-foreground"), children: data.balance ? _nullishCoalesce(data.balanceLabel, () => ( "Balance")) : "Expiry" }),
            /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "span", { className: "text-[15px] font-semibold tabular-nums", children: _nullishCoalesce(data.balance, () => ( data.expiry)) })
          ] })
        ] }),
        /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "div", { className: "relative flex items-center justify-between", children: [
          /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "span", { className: _chunkMD6ORKN4cjs.cn.call(void 0, "text-lg font-black tracking-tight", dark ? "text-white" : "text-foreground"), children: "\u224B" }),
          /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "span", { className: "text-sm font-black tracking-[0.08em] uppercase italic", children: _nullishCoalesce(data.brand, () => ( "VISA")) })
        ] })
      ]
    }
  );
}
function CardCarousel({
  cards,
  renderCard,
  className
}) {
  const [active, setActive] = React.useState(0);
  const trackRef = React.useRef(null);
  const onScroll = () => {
    const el = trackRef.current;
    if (!el) return;
    const i = Math.round(el.scrollLeft / el.clientWidth);
    setActive(Math.max(0, Math.min(cards.length - 1, i)));
  };
  return /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "div", { className: _chunkMD6ORKN4cjs.cn.call(void 0, "flex flex-col gap-3", className), children: [
    /* @__PURE__ */ _jsxruntime.jsx.call(void 0, 
      "div",
      {
        ref: trackRef,
        onScroll,
        className: "lx-scroll flex snap-x snap-mandatory gap-3 overflow-x-auto",
        children: cards.map((c, i) => /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "div", { className: "w-full shrink-0 snap-center", children: renderCard ? renderCard(c, i) : /* @__PURE__ */ _jsxruntime.jsx.call(void 0, BankCard, { data: c }) }, i))
      }
    ),
    cards.length > 1 && /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "div", { className: "flex items-center justify-center gap-1.5", "aria-hidden": true, children: cards.map((_, i) => /* @__PURE__ */ _jsxruntime.jsx.call(void 0, 
      "span",
      {
        className: _chunkMD6ORKN4cjs.cn.call(void 0, 
          "size-1.5 rounded-full transition-colors",
          i === active ? "bg-foreground" : "bg-border-strong"
        )
      },
      i
    )) })
  ] });
}




exports.BankCard = BankCard; exports.CardCarousel = CardCarousel;
//# sourceMappingURL=chunk-BANTC3AR.cjs.map