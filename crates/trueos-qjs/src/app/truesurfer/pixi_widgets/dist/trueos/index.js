"use strict";
var __extends = (this && this.__extends) || (function () {
    var extendStatics = function (d, b) {
        extendStatics = Object.setPrototypeOf ||
            ({ __proto__: [] } instanceof Array && function (d, b) { d.__proto__ = b; }) ||
            function (d, b) { for (var p in b) if (Object.prototype.hasOwnProperty.call(b, p)) d[p] = b[p]; };
        return extendStatics(d, b);
    };
    return function (d, b) {
        if (typeof b !== "function" && b !== null)
            throw new TypeError("Class extends value " + String(b) + " is not a constructor or null");
        extendStatics(d, b);
        function __() { this.constructor = d; }
        d.prototype = b === null ? Object.create(b) : (__.prototype = b.prototype, new __());
    };
})();
var __generator = (this && this.__generator) || function (thisArg, body) {
    var _ = { label: 0, sent: function() { if (t[0] & 1) throw t[1]; return t[1]; }, trys: [], ops: [] }, f, y, t, g = Object.create((typeof Iterator === "function" ? Iterator : Object).prototype);
    return g.next = verb(0), g["throw"] = verb(1), g["return"] = verb(2), typeof Symbol === "function" && (g[Symbol.iterator] = function() { return this; }), g;
    function verb(n) { return function (v) { return step([n, v]); }; }
    function step(op) {
        if (f) throw new TypeError("Generator is already executing.");
        while (g && (g = 0, op[0] && (_ = 0)), _) try {
            if (f = 1, y && (t = op[0] & 2 ? y["return"] : op[0] ? y["throw"] || ((t = y["return"]) && t.call(y), 0) : y.next) && !(t = t.call(y, op[1])).done) return t;
            if (y = 0, t) op = [op[0] & 2, t.value];
            switch (op[0]) {
                case 0: case 1: t = op; break;
                case 4: _.label++; return { value: op[1], done: false };
                case 5: _.label++; y = op[1]; op = [0]; continue;
                case 7: op = _.ops.pop(); _.trys.pop(); continue;
                default:
                    if (!(t = _.trys, t = t.length > 0 && t[t.length - 1]) && (op[0] === 6 || op[0] === 2)) { _ = 0; continue; }
                    if (op[0] === 3 && (!t || (op[1] > t[0] && op[1] < t[3]))) { _.label = op[1]; break; }
                    if (op[0] === 6 && _.label < t[1]) { _.label = t[1]; t = op; break; }
                    if (t && _.label < t[2]) { _.label = t[2]; _.ops.push(op); break; }
                    if (t[2]) _.ops.pop();
                    _.trys.pop(); continue;
            }
            op = body.call(thisArg, _);
        } catch (e) { op = [6, e]; y = 0; } finally { f = t = 0; }
        if (op[0] & 5) throw op[1]; return { value: op[0] ? op[1] : void 0, done: true };
    }
};
var __values = (this && this.__values) || function(o) {
    var s = typeof Symbol === "function" && Symbol.iterator, m = s && o[s], i = 0;
    if (m) return m.call(o);
    if (o && typeof o.length === "number") return {
        next: function () {
            if (o && i >= o.length) o = void 0;
            return { value: o && o[i++], done: !o };
        }
    };
    throw new TypeError(s ? "Object is not iterable." : "Symbol.iterator is not defined.");
};
var __read = (this && this.__read) || function (o, n) {
    var m = typeof Symbol === "function" && o[Symbol.iterator];
    if (!m) return o;
    var i = m.call(o), r, ar = [], e;
    try {
        while ((n === void 0 || n-- > 0) && !(r = i.next()).done) ar.push(r.value);
    }
    catch (error) { e = { error: error }; }
    finally {
        try {
            if (r && !r.done && (m = i["return"])) m.call(i);
        }
        finally { if (e) throw e.error; }
    }
    return ar;
};
var __spreadArray = (this && this.__spreadArray) || function (to, from, pack) {
    if (pack || arguments.length === 2) for (var i = 0, l = from.length, ar; i < l; i++) {
        if (ar || !(i in from)) {
            if (!ar) ar = Array.prototype.slice.call(from, 0, i);
            ar[i] = from[i];
        }
    }
    return to.concat(ar || Array.prototype.slice.call(from));
};
(function () {
    var ar = Object.defineProperty, Ji = Object.defineProperties;
    var Qi = Object.getOwnPropertyDescriptors;
    var sr = Object.getOwnPropertySymbols;
    var Zi = Object.prototype.hasOwnProperty, qi = Object.prototype.propertyIsEnumerable;
    var On = function (t, e, n) { return e in t ? ar(t, e, { enumerable: !0, configurable: !0, writable: !0, value: n }) : t[e] = n; }, ae = function (t, e) {
        var e_1, _a;
        for (var n in e || (e = {}))
            Zi.call(e, n) && On(t, n, e[n]);
        if (sr)
            try {
                for (var _b = __values(sr(e)), _c = _b.next(); !_c.done; _c = _b.next()) {
                    var n = _c.value;
                    qi.call(e, n) && On(t, n, e[n]);
                }
            }
            catch (e_1_1) { e_1 = { error: e_1_1 }; }
            finally {
                try {
                    if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
                }
                finally { if (e_1) throw e_1.error; }
            }
        return t;
    }, Se = function (t, e) { return Ji(t, Qi(e)); };
    var to = function (t, e) { return function () { return (t && (e = t(t = 0)), e); }; };
    var eo = function (t, e) { for (var n in e)
        ar(t, n, { get: e[n], enumerable: !0 }); };
    var It = function (t, e, n) { return On(t, typeof e != "symbol" ? e + "" : e, n); };
    var nn = function (t, e, n) { return new Promise(function (r, i) { var o = function (a) { try {
        c(n.next(a));
    }
    catch (h) {
        i(h);
    } }, s = function (a) { try {
        c(n.throw(a));
    }
    catch (h) {
        i(h);
    } }, c = function (a) { return a.done ? r(a.value) : Promise.resolve(a.value).then(o, s); }; c((n = n.apply(t, e)).next()); }); };
    var Mi = {};
    eo(Mi, { default: function () { return Jo; } });
    var Jo, Si = to(function () { Jo = {}; });
    var Be = /** @class */ (function () {
        function Be(e, n) {
            if (e === void 0) { e = 0; }
            if (n === void 0) { n = 0; }
            It(this, "x");
            It(this, "y");
            this.x = Number(e) || 0, this.y = Number(n) || 0;
        }
        Be.prototype.set = function (e, n) {
            if (e === void 0) { e = 0; }
            if (n === void 0) { n = e; }
            this.x = Number(e) || 0, this.y = Number(n) || 0;
        };
        return Be;
    }()), Rt = /** @class */ (function () {
        function Rt(e, n, r, i) {
            if (e === void 0) { e = 0; }
            if (n === void 0) { n = 0; }
            if (r === void 0) { r = 0; }
            if (i === void 0) { i = 0; }
            It(this, "x");
            It(this, "y");
            It(this, "width");
            It(this, "height");
            this.x = Number(e) || 0, this.y = Number(n) || 0, this.width = Number(r) || 0, this.height = Number(i) || 0;
        }
        return Rt;
    }()), Cn = /** @class */ (function () {
        function Cn() {
            It(this, "parent");
            It(this, "children");
            It(this, "label");
            It(this, "name");
            It(this, "position");
            It(this, "scale");
            It(this, "pivot");
            It(this, "visible");
            It(this, "alpha");
            It(this, "mask");
            It(this, "rotation");
            It(this, "zIndex");
            It(this, "eventMode");
            It(this, "cursor");
            It(this, "hitArea");
            It(this, "listeners");
            this.parent = null, this.position = new Be, this.scale = new Be(1, 1), this.pivot = new Be, this.visible = !0, this.alpha = 1, this.mask = null, this.rotation = 0, this.zIndex = 0, this.eventMode = null, this.cursor = null, this.hitArea = null, this.listeners = {};
        }
        Object.defineProperty(Cn.prototype, "x", {
            get: function () { return this.position.x; },
            set: function (e) { this.position.x = Number(e) || 0; },
            enumerable: false,
            configurable: true
        });
        Object.defineProperty(Cn.prototype, "y", {
            get: function () { return this.position.y; },
            set: function (e) { this.position.y = Number(e) || 0; },
            enumerable: false,
            configurable: true
        });
        Cn.prototype.on = function (e, n) { return this; };
        Cn.prototype.removeAllListeners = function (e) { return e == null ? this.listeners = {} : delete this.listeners[String(e)], this; };
        Cn.prototype.removeFromParent = function () { var e; return (e = this.parent) == null || e.removeChild(this), this; };
        Cn.prototype.destroy = function (e) { this.removeFromParent(), this.removeAllListeners(); };
        Cn.prototype.toLocal = function (e) { var n = e || {}; return { x: (Number(n.x) || 0) - this.getGlobalX(), y: (Number(n.y) || 0) - this.getGlobalY() }; };
        Cn.prototype.getGlobalPosition = function () { return { x: this.getGlobalX(), y: this.getGlobalY() }; };
        Cn.prototype.getGlobalX = function () { return (this.parent ? this.parent.getGlobalX() : 0) + this.x; };
        Cn.prototype.getGlobalY = function () { return (this.parent ? this.parent.getGlobalY() : 0) + this.y; };
        return Cn;
    }()), Ot = /** @class */ (function (_super) {
        __extends(Ot, _super);
        function Ot() {
            var _this = _super.call(this) || this;
            It(_this, "children");
            It(_this, "sortableChildren");
            _this.children = [], _this.sortableChildren = !1;
            return _this;
        }
        Ot.prototype.addChild = function () {
            var e_2, _a;
            var n = [];
            for (var _b = 0; _b < arguments.length; _b++) {
                n[_b] = arguments[_b];
            }
            var r;
            try {
                for (var n_1 = __values(n), n_1_1 = n_1.next(); !n_1_1.done; n_1_1 = n_1.next()) {
                    var i = n_1_1.value;
                    i && ((r = i.parent) == null || r.removeChild(i), i.parent = this, this.children.push(i));
                }
            }
            catch (e_2_1) { e_2 = { error: e_2_1 }; }
            finally {
                try {
                    if (n_1_1 && !n_1_1.done && (_a = n_1.return)) _a.call(n_1);
                }
                finally { if (e_2) throw e_2.error; }
            }
            return n[0];
        };
        Ot.prototype.addChildAt = function (n, r) { var o; (o = n.parent) == null || o.removeChild(n), n.parent = this; var i = Math.max(0, Math.min(Number(r) | 0, this.children.length)); return this.children.splice(i, 0, n), n; };
        Ot.prototype.removeChild = function () {
            var e_3, _a;
            var n = [];
            for (var _b = 0; _b < arguments.length; _b++) {
                n[_b] = arguments[_b];
            }
            try {
                for (var n_2 = __values(n), n_2_1 = n_2.next(); !n_2_1.done; n_2_1 = n_2.next()) {
                    var r = n_2_1.value;
                    var i = this.children.indexOf(r);
                    i >= 0 && this.children.splice(i, 1), r && (r.parent = null);
                }
            }
            catch (e_3_1) { e_3 = { error: e_3_1 }; }
            finally {
                try {
                    if (n_2_1 && !n_2_1.done && (_a = n_2.return)) _a.call(n_2);
                }
                finally { if (e_3) throw e_3.error; }
            }
            return n[0];
        };
        Ot.prototype.removeChildren = function (n, r) {
            var e_4, _a;
            if (n === void 0) { n = 0; }
            if (r === void 0) { r = this.children.length; }
            var i = Math.max(0, Number(n) | 0), o = Math.max(i, Math.min(Number(r) | 0, this.children.length)), s = this.children.splice(i, o - i);
            try {
                for (var s_1 = __values(s), s_1_1 = s_1.next(); !s_1_1.done; s_1_1 = s_1.next()) {
                    var c = s_1_1.value;
                    c.parent = null;
                }
            }
            catch (e_4_1) { e_4 = { error: e_4_1 }; }
            finally {
                try {
                    if (s_1_1 && !s_1_1.done && (_a = s_1.return)) _a.call(s_1);
                }
                finally { if (e_4) throw e_4.error; }
            }
            return s;
        };
        Ot.prototype.setChildIndex = function (n, r) { var i = this.children.indexOf(n); if (i < 0)
            return; this.children.splice(i, 1); var o = Math.max(0, Math.min(Number(r) | 0, this.children.length)); this.children.splice(o, 0, n); };
        Ot.prototype.getChildIndex = function (n) { return this.children.indexOf(n); };
        Ot.prototype.getChildByLabel = function (n) { for (var r = 0; r < this.children.length; r += 1) {
            var i = this.children[r];
            if (i && i.label === n)
                return i;
        } return null; };
        return Ot;
    }(Cn)), Pt = /** @class */ (function (_super) {
        __extends(Pt, _super);
        function Pt() {
            var _this = _super.call(this) || this;
            It(_this, "commands");
            _this.commands = [];
            return _this;
        }
        Pt.prototype.clear = function () { return this.commands.length = 0, this; };
        Pt.prototype.rect = function (n, r, i, o) { return this.commands.push(["rect", n, r, i, o]), this; };
        Pt.prototype.roundRect = function (n, r, i, o, s) {
            if (s === void 0) { s = 0; }
            return this.commands.push(["roundRect", n, r, i, o, s]), this;
        };
        Pt.prototype.circle = function (n, r, i) { return this.commands.push(["circle", n, r, i]), this; };
        Pt.prototype.ellipse = function (n, r, i, o) { return this.commands.push(["ellipse", n, r, i, o]), this; };
        Pt.prototype.moveTo = function (n, r) { return this.commands.push(["moveTo", n, r]), this; };
        Pt.prototype.lineTo = function (n, r) { return this.commands.push(["lineTo", n, r]), this; };
        Pt.prototype.closePath = function () { return this.commands.push(["closePath"]), this; };
        Pt.prototype.poly = function (n) { return this.commands.push(["poly", n]), this; };
        Pt.prototype.fill = function (n) { return this.commands.push(["fill", n]), this; };
        Pt.prototype.stroke = function (n) { return this.commands.push(["stroke", n]), this; };
        Pt.prototype.image = function (n, r, i, o, s) { return this.commands.push(["image", n, r, i, o, s]), this; };
        Pt.prototype.svg = function (n) { return this.commands.push(["svg", n]), this; };
        return Pt;
    }(Ot)), ie = /** @class */ (function (_super) {
        __extends(ie, _super);
        function ie(n) {
            if (n === void 0) { n = ""; }
            var r, i;
            var _this = _super.call(this) || this;
            It(_this, "_text");
            It(_this, "_style");
            It(_this, "_resolution");
            _this._text = "", _this._style = {}, _this._resolution = 1, typeof n == "string" ? _this._text = n : (_this._text = String((r = n.text) != null ? r : ""), _this._style = ae({}, (i = n.style) != null ? i : {}));
            return _this;
        }
        Object.defineProperty(ie.prototype, "text", {
            get: function () { return this._text; },
            set: function (n) { this._text = String(n != null ? n : ""); },
            enumerable: false,
            configurable: true
        });
        Object.defineProperty(ie.prototype, "style", {
            get: function () { return this._style; },
            set: function (n) { this._style = n != null ? n : {}; },
            enumerable: false,
            configurable: true
        });
        Object.defineProperty(ie.prototype, "resolution", {
            get: function () { return this._resolution; },
            set: function (n) { this._resolution = Math.max(1, Number(n) || 1); },
            enumerable: false,
            configurable: true
        });
        Object.defineProperty(ie.prototype, "width", {
            get: function () { var n = Number(this._style.fontSize) || 16; return this._text.length * n * .58; },
            enumerable: false,
            configurable: true
        });
        Object.defineProperty(ie.prototype, "height", {
            get: function () { var n = Number(this._style.fontSize) || 16; return Number(this._style.lineHeight) || n * 1.25; },
            enumerable: false,
            configurable: true
        });
        ie.prototype.setSize = function (n, r) { return this; };
        return ie;
    }(Ot)), Ce = /** @class */ (function () {
        function Ce(e) {
            if (e === void 0) { e = {}; }
            It(this, "options");
            this.options = e;
        }
        Ce.prototype.addAttribute = function (e, n) { return this; };
        Ce.prototype.destroy = function () { };
        return Ce;
    }()), rn = /** @class */ (function (_super) {
        __extends(rn, _super);
        function rn(n) {
            if (n === void 0) { n = {}; }
            var r, i;
            var _this = _super.call(this) || this;
            It(_this, "geometry");
            It(_this, "shader");
            _this.geometry = (r = n.geometry) != null ? r : new Ce, _this.shader = (i = n.shader) != null ? i : new Ue;
            return _this;
        }
        return rn;
    }(Ot)), on = /** @class */ (function () {
        function on(e) {
            if (e === void 0) { e = {}; }
            It(this, "options");
            this.options = e;
        }
        return on;
    }()), An = { VERTEX: 1, COPY_DST: 2 }, Ue = /** @class */ (function () {
        function Ue(e) {
            if (e === void 0) { e = {}; }
            It(this, "options");
            this.options = e;
        }
        return Ue;
    }());
    var lr = "", cr = "", ur = "", sn = /** @class */ (function () {
        function sn() {
            var _this = this;
            It(this, "stage");
            It(this, "screen");
            It(this, "canvas");
            It(this, "renderer");
            It(this, "ticker");
            var e = Math.max(1, Number(globalThis.innerWidth || 1920) | 0), n = Math.max(1, Number(globalThis.innerHeight || 1080) | 0);
            this.stage = new Ot, this.screen = new Rt(0, 0, e, n), this.canvas = document.createElement("canvas"), this.ticker = { stop: function () { }, add: function () { }, remove: function () { } }, this.renderer = { width: e, height: n, screen: this.screen, render: function (r) { return r; }, resize: function (r, i) { var o = Math.max(1, Number(r || e) | 0), s = Math.max(1, Number(i || n) | 0); _this.renderer.width = o, _this.renderer.height = s, _this.screen.width = o, _this.screen.height = s; } };
        }
        sn.prototype.init = function (e) { return nn(this, null, function () { return __generator(this, function (_a) {
            return [2 /*return*/];
        }); }); };
        return sn;
    }());
    var ne = { fontFamily: "system-ui, -apple-system, Segoe UI, Arial", fontSize: 16, background: 16777215, text: 1118481, mutedText: 6710886, boxBorder: 14540253, hr: 13421772, control: { border: 0, focusBorder: 3900150, background: 16777215, accent: 3900150, radius: 0, button: { fill: 15921906, hoverFill: 15395562, activeFill: 14737632, border: 6710886, text: 1118481, radius: 0 }, progress: { border: 10066329, background: 16777215, fill: 6990335 }, table: { border: 10066329, cellBorder: 11579568, headerFill: 16250871 } } };
    var Ae = 24, Mt = 1;
    function Zt(t) { var i, o; var e = t.wrapWidth, n = (i = t.wordWrap) != null ? i : e != null, r = (o = t.wordWrapWidth) != null ? o : e == null ? void 0 : Math.max(0, Math.ceil(e) + Ae); return new ie({ text: t.text, style: { fontFamily: t.fontFamily, fontSize: t.fontSize, fill: t.fill, fontWeight: t.bold ? "700" : "400", wordWrap: n, wordWrapWidth: r } }); }
    function Nn(t, e) { var n = t.children; if (!Array.isArray(n))
        return null; for (var r = 0; r < n.length; r += 1) {
        var i = n[r];
        if (i && i.label === e)
            return i;
    } return null; }
    function ue(t, e) { var n = Nn(t, e); if (n)
        return n; var r = new Ot; return r.label = e, t.addChild(r), r; }
    function $t(t, e) { var n = Nn(t, e); if (n)
        return n; var r = new Pt; return r.label = e, t.addChild(r), r; }
    function Ht(t, e, n) { var r = Nn(t, e); if (r)
        return r; var i = new ie({ text: "" }); return i.label = e, n == null || n(i), t.addChild(i), i; }
    function vt(t) { t.clear(), t.removeAllListeners(), t.hitArea = null; }
    function Jt(t) { t.removeAllListeners(); }
    function we(t, e, n) {
        var r = String(t != null ? t : ""), i = [], o = 0;
        for (var s = 0; s <= r.length; s++) {
            if (!(s === r.length || r[s] === "\n"))
                continue;
            var a = o, h = s;
            if (a === h)
                i.push({ start: a, end: h, text: "" });
            else {
                var m = a, d = -1;
                for (var b = m; b < h; b++) {
                    r[b] === " " && (d = b);
                    var w = r.slice(m, b + 1);
                    if (n(w) <= e || b === m)
                        continue;
                    var u = d >= m ? d + 1 : b;
                    u <= m && (u = Math.min(h, m + 1)), i.push({ start: m, end: u, text: r.slice(m, u) }), m = u, b = m - 1, d = -1;
                }
                m <= h && i.push({ start: m, end: h, text: r.slice(m, h) });
            }
            o = s + 1;
        }
        return i;
    }
    function Te(t, e) { return e <= 0 ? [] : t.length <= e ? t : t.slice(0, e); }
    function Ne(t) { var e = t.fullText, n = t.lines, r = t.localX, i = t.localY, o = t.lineHeight, s = t.measure; if (n.length === 0)
        return 0; var c = Math.max(0, r), a = Math.max(0, i), h = Math.max(1, o), m = Math.max(0, Math.min(n.length - 1, Math.floor(a / h))), d = n[m], b = d.start, y = Number.POSITIVE_INFINITY; for (var w = d.start; w <= d.end; w++) {
        var u = s(e.slice(d.start, w)), _ = Math.abs(u - c);
        _ < y && (y = _, b = w);
    } return b; }
    function dr(t) { var w, u, _, p; var e = t.node, n = t.graphics, r = t.w, i = t.h, o = t.theme, s = Math.max(0, Math.round(r)), c = Math.max(0, Math.round(i)); n.rect(.5, .5, Math.max(0, s - 1), Math.max(0, c - 1)), n.fill(o.control.progress.background), n.stroke({ width: 1, color: o.control.progress.border }); var a = Number((u = (w = e.attrs) == null ? void 0 : w.value) != null ? u : "0"), h = Number((p = (_ = e.attrs) == null ? void 0 : _.max) != null ? p : "1"), m = h > 0 ? Math.max(0, Math.min(1, a / h)) : 0, d = 3, b = Math.max(0, s - d * 2), y = Math.max(0, c - d * 2); n.rect(d, d, Math.max(0, b * m), y), n.fill(o.control.progress.fill); }
    function hr(t, e) { t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0), t.setHeight(14), t.setMinWidth(240); }
    function Pe(t, e, n) { var c; var r = t.get(e); if (r)
        return r; var i = Number((c = n == null ? void 0 : n.value) != null ? c : "0"), o = Number.isFinite(i) ? i : 0, s = { value: Math.max(0, Math.min(1, o)) }; return t.set(e, s), s; }
    function mr(t, e) { t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0), t.setHeight(14), t.setMinWidth(240); }
    function fr(t) { var e = t.node, n = t.Yoga, r = t.measurer, i = n.Node.create(); return i.setPadding(n.EDGE_LEFT, 0), i.setPadding(n.EDGE_RIGHT, 0), i.setPadding(n.EDGE_TOP, 0), i.setPadding(n.EDGE_BOTTOM, 0), i.setMargin(n.EDGE_RIGHT, 6), i.setMeasureFunc(function () { var o = r.measure("100"); return { width: o.width, height: o.height }; }), { yogaNode: i, buildBox: function () { return ({ kind: "block", key: e.key, tagName: e.tagName, attrs: e.attrs, x: i.getComputedLeft(), y: i.getComputedTop(), width: i.getComputedWidth(), height: i.getComputedHeight(), children: [] }); } }; }
    function pr(t) { var h, m; var e = t.node, n = t.container, r = t.theme, i = t.sliderStates, o = (h = e.attrs) == null ? void 0 : h["data-slider-key"], s = null; if (o) {
        var d = i.get(o);
        if (d)
            s = d;
        else {
            var b = (m = e.attrs) == null ? void 0 : m["data-slider-init"];
            s = Pe(i, o, b != null ? { value: String(b) } : void 0);
        }
    } var c = s ? Math.round(s.value * 100) : 0, a = Ht(n, "__pct", function (d) { d.style = { fontFamily: r.fontFamily, fontSize: r.fontSize, fill: r.text, fontWeight: "400", wordWrap: !1 }; }); a.text = String(c), a.position.set(0, Mt); }
    function an(t) { var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.absX, c = t.absY, a = t.theme, h = t.sliderStates, m = t.sliderBounds, d = t.sliderDrags, b = t.requestPaint, y = e.key, w = y ? Pe(h, y, e.attrs) : null, u = Math.max(0, Math.round(i)), _ = Math.max(0, Math.round(o)), p = 3; y && m.set(y, { x: s, y: c, w: u, h: _, innerPad: p }), r.rect(.5, .5, Math.max(0, u - 1), Math.max(0, _ - 1)), r.fill(a.control.progress.background), r.stroke({ width: 1, color: a.control.progress.border }); var S = w ? Math.max(0, Math.min(1, w.value)) : 0, O = Math.max(0, u - p * 2), D = Math.max(0, _ - p * 2); r.rect(p, p, Math.max(0, O * S), D), r.fill(a.control.progress.fill); var U = p + O * S, T = D / 2; r.moveTo(U, p - T), r.lineTo(U, p + D + T), r.stroke({ width: 2, color: a.text }), y && (Jt(n), n.eventMode = "static", n.cursor = "pointer", n.hitArea = new Rt(0, 0, Math.max(0, u), Math.max(0, _)), n.on("pointerdown", function (A) {
        var e_5, _a;
        var H, q, rt, ot, K, et;
        if ((A == null ? void 0 : A.button) === 2)
            return;
        var G = t.getPointerId ? t.getPointerId(A) : Number((rt = (q = A == null ? void 0 : A.pointerId) != null ? q : (H = A == null ? void 0 : A.data) == null ? void 0 : H.pointerId) != null ? rt : 0);
        if (G <= 0)
            return;
        try {
            for (var _b = __values(d.entries()), _c = _b.next(); !_c.done; _c = _b.next()) {
                var _d = __read(_c.value, 2), C = _d[0], V = _d[1];
                V.key === y && C !== G && d.delete(C);
            }
        }
        catch (e_5_1) { e_5 = { error: e_5_1 }; }
        finally {
            try {
                if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
            }
            finally { if (e_5) throw e_5.error; }
        }
        d.set(G, { key: y });
        var k = m.get(y), P = (K = (ot = A.global) == null ? void 0 : ot.x) != null ? K : 0, X = k ? P - k.x : 0, j = k ? Math.max(1, k.w - k.innerPad * 2) : 1, f = (X - ((et = k == null ? void 0 : k.innerPad) != null ? et : 0)) / j, v = Pe(h, y, e.attrs);
        v.value = Math.max(0, Math.min(1, f)), b == null || b();
    })); }
    function gr(t) { var u, _; var e = t.node, n = t.container, r = t.w, i = t.h, o = t.theme, s = t.detailsOpen, c = t.requestRerender, a = (u = e.attrs) == null ? void 0 : u["data-details-key"], h = e.attrs ? Object.prototype.hasOwnProperty.call(e.attrs, "data-details-open") : !1, m = a && s.has(a) ? s.get(a) === !0 : h, d = function (p) { var D; if (!a || (p == null ? void 0 : p.button) === 2)
        return; var O = !(s.has(a) ? s.get(a) === !0 : h); s.set(a, O), c == null || c(), (D = p == null ? void 0 : p.stopPropagation) == null || D.call(p); }, b = 16, y = (_ = n.children) == null ? void 0 : _.find(function (p) { return (p == null ? void 0 : p.label) === "__arrow"; }); y && (vt(y), y.visible = !1); var w = Ht(n, "__arrowText", function (p) { p.style = { fontFamily: o.fontFamily, fontSize: o.fontSize, fill: o.text, fontWeight: "700" }; }); w.visible = !0, w.text = m ? "v" : ">", w.style.fontFamily = o.fontFamily, w.style.fontSize = o.fontSize, w.style.fill = o.text, w.style.fontWeight = "700", w.position.set(5, Math.max(0, (i - o.fontSize) / 2) + Mt), a && (Jt(n), n.eventMode = "static", n.cursor = "pointer", n.hitArea = new Rt(0, 0, Math.max(0, r), Math.max(0, i)), n.on("pointerdown", d)); }
    function br(t, e) { t.setFlexDirection(e.FLEX_DIRECTION_ROW), t.setAlignItems(e.ALIGN_CENTER), t.setJustifyContent(e.JUSTIFY_SPACE_BETWEEN), t.setPadding(e.EDGE_TOP, 6), t.setPadding(e.EDGE_BOTTOM, 6), t.setPadding(e.EDGE_LEFT, 26), t.setPadding(e.EDGE_RIGHT, 12), t.setMinHeight(36); }
    function _r(t, e) { t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0); }
    function yr(t, e) { var i, o, s; if (!t || t.tagName !== "details" || !t.key)
        return (i = t == null ? void 0 : t.children) != null ? i : []; var n = t.attrs ? Object.prototype.hasOwnProperty.call(t.attrs, "open") : !1; return (e.has(t.key) ? e.get(t.key) === !0 : n) ? (o = t.children) != null ? o : [] : ((s = t.children) != null ? s : []).filter(function (c) { return c && c.kind === "block" && c.tagName === "summary"; }); }
    function xr(t) { var e = t.graphics, n = t.w, r = t.theme; e.rect(0, 0, Math.round(n), 1), e.fill(r.hr); }
    function wr(t, e) { t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0), t.setMargin(e.EDGE_TOP, 2), t.setMargin(e.EDGE_BOTTOM, 2), t.setHeight(1); }
    function Tr(t) { var _, p; var e = t.container, n = t.graphics, r = t.w, i = t.h, o = t.label, s = t.theme, c = t.registerHoverHandlers, a = function (S) { n.clear(); var O = 1, D = O / 2; s.control.button.radius > 0 ? n.roundRect(D, D, Math.max(0, r - O), Math.max(0, i - O), s.control.button.radius) : n.rect(D, D, Math.max(0, r - O), Math.max(0, i - O)), n.fill(S), n.stroke({ width: O, color: s.control.button.border }); }; a(s.control.button.fill); var h = Ht(e, "__label", function (S) { S.style = { fontFamily: s.fontFamily, fontSize: s.fontSize, fill: s.control.button.text, fontWeight: "400", wordWrap: !1, wordWrapWidth: 0 }; }), m = String(o != null ? o : "").trim(); h.text = m, h.visible = m.length > 0, h.style = Se(ae({}, h.style), { fontFamily: s.fontFamily, fontSize: s.fontSize, fill: s.control.button.text, wordWrap: !1, wordWrapWidth: Math.max(0, Math.ceil(r - 16)) }); var d = Number((_ = h.width) != null ? _ : 0), b = Number((p = h.height) != null ? p : 0), y = s.fontSize * 1.25; h.position.set(d > 0 ? Math.max(8, Math.floor((r - d) / 2)) : 8, Math.max(0, Math.floor((i - (b > 0 ? b : y)) / 2)) + Mt); var w = function () { return a(s.control.button.hoverFill); }, u = function () { return a(s.control.button.fill); }; c == null || c({ over: w, out: u }), Jt(e), e.eventMode = "static", e.cursor = "pointer", e.on("pointerover", w), e.on("pointerout", u), e.on("pointerdown", function () { return a(s.control.button.activeFill); }), e.on("pointerup", function () { return a(s.control.button.hoverFill); }); }
    function Er(t, e) { t.setFlexDirection(e.FLEX_DIRECTION_ROW), t.setPadding(e.EDGE_TOP, 6), t.setPadding(e.EDGE_BOTTOM, 6), t.setMinHeight(36), t.setMinWidth(100), t.setAlignItems(e.ALIGN_CENTER), t.setJustifyContent(e.JUSTIFY_CENTER); }
    function Ir(t) { var e = t.graphics, n = t.w, r = t.h, i = t.boxBorder, o = Math.max(0, Math.round(n)), s = Math.max(0, Math.round(r)); e.rect(0, 0, o, s), e.stroke({ width: 1, color: i, alignment: 0 }); }
    function Mr(t) { var e = t.nodeTag, n = t.graphics, r = t.w, i = t.h, o = t.theme; e === "th" && (n.rect(0, 0, r, i), n.fill(o.control.table.headerFill)), n.rect(0, 0, r, i), n.stroke({ width: 1, color: o.control.table.cellBorder }); }
    function Sr(t, e) { t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0); }
    function Pr(t, e) { t.setFlexDirection(e.FLEX_DIRECTION_ROW), t.setFlexWrap(e.WRAP_NO_WRAP), t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0), t.setMargin(e.EDGE_BOTTOM, 0); }
    function kr(t, e) { t.setFlexGrow(1), t.setFlexShrink(1), t.setMinWidth(80), t.setPadding(e.EDGE_LEFT, 8), t.setPadding(e.EDGE_RIGHT, 8), t.setPadding(e.EDGE_TOP, 6), t.setPadding(e.EDGE_BOTTOM, 6), t.setMargin(e.EDGE_BOTTOM, 0); }
    function Ln(t) { var e = String(t != null ? t : "").toLowerCase(); if (e.length !== 2 || e.charAt(0) !== "h")
        return !1; var n = e.charCodeAt(1); return n >= 49 && n <= 54; }
    function Rr(t, e) { t.setPadding(e.EDGE_TOP, 6), t.setPadding(e.EDGE_BOTTOM, 6), t.setMinHeight(36), t.setJustifyContent(e.JUSTIFY_CENTER); }
    function Or(t, e) {
        var n = Math.max(1, Math.floor(t)), r = Math.max(1, Math.floor(e));
        return "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<svg viewBox=\"0 0 ".concat(n, " ").concat(r, "\" xmlns=\"http://www.w3.org/2000/svg\">\n  <rect x=\"0\" y=\"0\" width=\"").concat(n, "\" height=\"").concat(r, "\" fill=\"#f6f6f6\"/>\n  <rect x=\"0.5\" y=\"0.5\" width=\"").concat(Math.max(0, n - 1), "\" height=\"").concat(Math.max(0, r - 1), "\" fill=\"none\" stroke=\"#999\"/>\n  <path d=\"M2 2 L").concat(Math.max(2, n - 2), " ").concat(Math.max(2, r - 2), "\" stroke=\"#c8c8c8\"/>\n  <path d=\"M").concat(Math.max(2, n - 2), " 2 L2 ").concat(Math.max(2, r - 2), "\" stroke=\"#c8c8c8\"/>\n</svg>");
    }
    function Cr(_a) {
        var _b = _a === void 0 ? {} : _a, _c = _b.ring, t = _c === void 0 ? 34 : _c, _d = _b.core, e = _d === void 0 ? 14 : _d, _f = _b.hueA, n = _f === void 0 ? "#00e5ff" : _f, _g = _b.hueB, r = _g === void 0 ? "#ff2bd6" : _g;
        var i = Math.max(0, t - 10), o = Math.max(0, e * .35);
        return "\n<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 100 100\">\n  <rect width=\"100\" height=\"100\" fill=\"#ffffff\"/>\n  <rect width=\"100\" height=\"100\" fill=\"".concat(n, "\" opacity=\"0.08\"/>\n\n  <circle cx=\"50\" cy=\"50\" r=\"").concat(t, "\" fill=\"none\" stroke=\"").concat(r, "\" stroke-width=\"4\" opacity=\"0.95\"/>\n  <circle cx=\"50\" cy=\"50\" r=\"").concat(i, "\" fill=\"none\" stroke=\"").concat(n, "\" stroke-width=\"1\" opacity=\"0.35\"/>\n\n  <circle cx=\"50\" cy=\"50\" r=\"").concat(e, "\" fill=\"").concat(n, "\" opacity=\"0.9\"/>\n  <circle cx=\"43\" cy=\"43\" r=\"").concat(o, "\" fill=\"#ffffff\" opacity=\"0.55\"/>\n\n  <path d=\"M50 16 L52 22 L58 24 L52 26 L50 32 L48 26 L42 24 L48 22 Z\" fill=\"#ffffff\" opacity=\"0.85\"/>\n  <path d=\"M82 52 L85 56 L90 57 L85 58 L82 62 L79 58 L74 57 L79 56 Z\" fill=\"#ffffff\" opacity=\"0.70\"/>\n  <path d=\"M20 70 L22 74 L27 75 L22 76 L20 80 L18 76 L13 75 L18 74 Z\" fill=\"#ffffff\" opacity=\"0.65\"/>\n</svg>\n");
    }
    var Ar = new Map;
    function Xe() { var t = globalThis; return !0; }
    function Dr(t) { var e = globalThis, n = String(t != null ? t : "").trim(); if (!n)
        return null; var r = e.__TRUEOS_READY_IMAGE_TEXTURE_CACHE__; (!r || typeof r.get != "function" || typeof r.set != "function") && (r = new Map, e.__TRUEOS_READY_IMAGE_TEXTURE_CACHE__ = r); var i = r.get(n); if (i)
        return i; var o = e.__trueosResolveReadyImageTexture; if (typeof o != "function")
        return null; var s = o(n); if (s && typeof s.then == "function") {
        var c = s.then(function (a) { return (r.set(n, a), a); }).catch(function (a) { throw r.delete(n), a; });
        return r.set(n, c), c;
    } return r.set(n, s), s; }
    function no(t) { var s; var e = globalThis, n = String(t != null ? t : "").trim(); if (!n)
        return null; var r = e.__trueosPeekReadyImageTexture; if (typeof r != "function")
        return null; var i = r(n); return !i || typeof i.then == "function" ? null : Math.max(0, Number((s = i == null ? void 0 : i.texId) != null ? s : 0) | 0) > 0 ? i : null; }
    function ro(t, e) { var n = globalThis, r = String(t != null ? t : "").trim(); if (!r || !e)
        return; var i = n.__TRUEOS_READY_IMAGE_TEXTURE_CACHE__; (!i || typeof i.get != "function" || typeof i.set != "function") && (i = new Map, n.__TRUEOS_READY_IMAGE_TEXTURE_CACHE__ = i), i.set(r, e); }
    function vr(t, e) { var r, i, o, s, c; var n = Math.max(0, Number((r = e == null ? void 0 : e.texId) != null ? r : 0) | 0); if (n <= 0)
        throw new Error("image texture missing"); t.state = "ready", t.texId = n, t.width = Math.max(0, Number((o = (i = e == null ? void 0 : e.pixelWidth) != null ? i : e == null ? void 0 : e.width) != null ? o : 0) | 0), t.height = Math.max(0, Number((c = (s = e == null ? void 0 : e.pixelHeight) != null ? s : e == null ? void 0 : e.height) != null ? c : 0) | 0); }
    function io(t, e) { var n = no(t) || Dr(t); return !n || typeof n.then == "function" ? !1 : (vr(e, n), ro(t, n), !0); }
    function Nr(t, e) { var n = String(t != null ? t : "").trim(); if (!n)
        return null; var r = Ar.get(n); if (r) {
        if (Xe() && r.state === "loading")
            try {
                io(n, r);
            }
            catch (c) {
                r.state = "error";
            }
        return r;
    } if (Xe())
        return null; var i = { state: "loading", texId: 0, width: 0, height: 0 }; Ar.set(n, i); var o = function (c) { vr(i, c), Xe() || e == null || e(); }, s = function () { i.state = "error", Xe() || e == null || e(); }; try {
        var c = Dr(n);
        if (!c)
            return i;
        if (c && typeof c.then == "function") {
            if (Xe())
                return i;
            c.then(o).catch(s);
        }
        else
            o(c);
    }
    catch (c) {
        s();
    } return i; }
    function oo(t) { var e = String(t != null ? t : ""); if (!e.startsWith("data:image/svg+xml"))
        return null; var n = e.indexOf(","); if (n === -1)
        return null; var r = e.slice(0, n).toLowerCase(), i = e.slice(n + 1); try {
        return r.includes(";base64") ? atob(i) : decodeURIComponent(i);
    }
    catch (o) {
        return null;
    } }
    function so(t) { return Lr(Lr(String(t), "tspan"), "text"); }
    function ao(t) { return "data:image/svg+xml;charset=utf-8,".concat(encodeURIComponent(t)); }
    function Lr(t, e) { var n = "", r = 0, i = t.toLowerCase(), o = "<" + e, s = "</" + e; for (; r < t.length;) {
        var c = i.indexOf(o, r);
        if (c < 0) {
            n += t.slice(r);
            break;
        }
        n += t.slice(r, c);
        var a = i.indexOf(s, c + o.length);
        if (a < 0)
            break;
        var h = t.indexOf(">", a + s.length);
        r = h < 0 ? t.length : h + 1;
    } return n; }
    function Gr(t) { var D, U, T, A; var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.theme, c = t.requestRerender, a = (U = (D = e.attrs) == null ? void 0 : D.alt) != null ? U : "", h = (A = (T = e.attrs) == null ? void 0 : T.src) != null ? A : "", m = h.trim().length > 0, d = a.trim().length > 0 ? a : h.trim().length > 0 ? h : "img", b = r.image, y = m ? Nr(h, c) : null; if ((y == null ? void 0 : y.state) === "ready" && y.texId > 0 && typeof b == "function") {
        b.call(r, y.texId, 0, 0, Math.max(0, i), Math.max(0, o));
        var G = n.getChildByLabel ? n.getChildByLabel("__label") : n.children.find(function (k) { return (k == null ? void 0 : k.label) === "__label"; });
        G && (G.visible = !1);
        return;
    } var w = m ? oo(h) : null, u = so(w != null ? w : m ? Or(i, o) : Cr({ ring: 34, core: 14 })), _ = $t(n, "__svg"), p = Nr(ao(u), c); if ((p == null ? void 0 : p.state) === "ready" && p.texId > 0 && typeof _.image == "function") {
        var G = "texture:".concat(p.texId, ":").concat(Math.round(i), "x").concat(Math.round(o));
        if (_.__key !== G && (vt(_), _.image(p.texId, 0, 0, Math.max(0, i), Math.max(0, o)), _.__key = G), _.scale.set(1), _.position.set(0, 0), !m) {
            var k = n.getChildByLabel ? n.getChildByLabel("__label") : n.children.find(function (P) { return (P == null ? void 0 : P.label) === "__label"; });
            k && (k.visible = !1);
            return;
        }
        if (d.trim().length > 0) {
            var k = Ht(n, "__label", function (P) { P.style = { fontFamily: s.fontFamily, fontSize: s.fontSize, fill: s.mutedText, fontWeight: "400", wordWrap: !0, wordWrapWidth: 0 }; });
            k.text = d, k.style.wordWrapWidth = Math.max(0, Math.ceil(i - 16) + Ae), k.position.set(8, 8 + Mt), k.visible = !0;
        }
        return;
    }
    else
        vt(_); var S = _.svg; if (0 && _.__key !== G)
        try { }
        catch (P) { } r.rect(.5, .5, Math.max(0, i - 1), Math.max(0, o - 1)), r.fill(16185078), r.stroke({ width: 1, color: s.control.border }); var O = Ht(n, "__label", function (G) { G.style = { fontFamily: s.fontFamily, fontSize: s.fontSize, fill: s.mutedText, fontWeight: "400", wordWrap: !0, wordWrapWidth: 0 }; }); O.text = d, O.style.wordWrapWidth = Math.max(0, Math.ceil(i - 16) + Ae), O.position.set(8, 8 + Mt); }
    function Hr(t, e, n) { var h, m, d, b; t.setPadding(n.EDGE_LEFT, 0), t.setPadding(n.EDGE_RIGHT, 0), t.setPadding(n.EDGE_TOP, 0), t.setPadding(n.EDGE_BOTTOM, 0); var r = Number((m = (h = e.attrs) == null ? void 0 : h.width) != null ? m : "0"), i = Number((b = (d = e.attrs) == null ? void 0 : d.height) != null ? b : "0"), o = Number.isFinite(r) && r > 0, s = Number.isFinite(i) && i > 0, c = o ? r : 240, a = s ? i : 140; (o || s) && (t.setAlignSelf(n.ALIGN_FLEX_START), t.setFlexGrow(0), t.setFlexShrink(0)), t.setWidth(c), t.setHeight(a), t.setMinWidth(120), t.setMinHeight(80); }
    var Wr = new Map;
    function Ye() { var t = globalThis; return !0; }
    function $r(t) { var e = globalThis, n = String(t != null ? t : "").trim(); if (!n)
        return null; var r = e.__TRUEOS_READY_IMAGE_TEXTURE_CACHE__; (!r || typeof r.get != "function" || typeof r.set != "function") && (r = new Map, e.__TRUEOS_READY_IMAGE_TEXTURE_CACHE__ = r); var i = r.get(n); if (i)
        return i; var o = e.__trueosResolveReadyImageTexture; if (typeof o != "function")
        return null; var s = o(n); if (s && typeof s.then == "function") {
        var c = s.then(function (a) { return (r.set(n, a), a); }).catch(function (a) { throw r.delete(n), a; });
        return r.set(n, c), c;
    } return r.set(n, s), s; }
    function lo(t) { var s; var e = globalThis, n = String(t != null ? t : "").trim(); if (!n)
        return null; var r = e.__trueosPeekReadyImageTexture; if (typeof r != "function")
        return null; var i = r(n); return !i || typeof i.then == "function" ? null : Math.max(0, Number((s = i == null ? void 0 : i.texId) != null ? s : 0) | 0) > 0 ? i : null; }
    function co(t, e) { var n = globalThis, r = String(t != null ? t : "").trim(); if (!r || !e)
        return; var i = n.__TRUEOS_READY_IMAGE_TEXTURE_CACHE__; (!i || typeof i.get != "function" || typeof i.set != "function") && (i = new Map, n.__TRUEOS_READY_IMAGE_TEXTURE_CACHE__ = i), i.set(r, e); }
    function Br(t, e) { var r, i, o, s, c; var n = Math.max(0, Number((r = e == null ? void 0 : e.texId) != null ? r : 0) | 0); if (n <= 0)
        throw new Error("svg texture missing"); t.state = "ready", t.texId = n, t.width = Math.max(0, Number((o = (i = e == null ? void 0 : e.pixelWidth) != null ? i : e == null ? void 0 : e.width) != null ? o : 0) | 0), t.height = Math.max(0, Number((c = (s = e == null ? void 0 : e.pixelHeight) != null ? s : e == null ? void 0 : e.height) != null ? c : 0) | 0); }
    function uo(t, e) { var n = lo(t) || $r(t); return !n || typeof n.then == "function" ? !1 : (Br(e, n), co(t, n), !0); }
    function ho(t) { return Fr(Fr(String(t), "tspan"), "text"); }
    function Fr(t, e) { var n = "", r = 0, i = t.toLowerCase(), o = "<" + e, s = "</" + e; for (; r < t.length;) {
        var c = i.indexOf(o, r);
        if (c < 0) {
            n += t.slice(r);
            break;
        }
        n += t.slice(r, c);
        var a = i.indexOf(s, c + o.length);
        if (a < 0)
            break;
        var h = t.indexOf(">", a + s.length);
        r = h < 0 ? t.length : h + 1;
    } return n; }
    function Ur(t) { var e = String(t), r = e.toLowerCase().indexOf("viewbox"); if (r < 0)
        return null; var i = e.indexOf("=", r + 7); if (i < 0)
        return null; var o = i + 1; for (; o < e.length;) {
        var y = e.charCodeAt(o);
        if (y !== 32 && y !== 9 && y !== 10 && y !== 13 && y !== 12)
            break;
        o += 1;
    } var s = e.charAt(o); if (s !== '"' && s !== "'")
        return null; var c = e.indexOf(s, o + 1); if (c < 0)
        return null; var a = mo(e.slice(o + 1, c)); if (a.length < 4)
        return null; var h = Number(a[0]), m = Number(a[1]), d = Number(a[2]), b = Number(a[3]); return ![h, m, d, b].every(function (y) { return Number.isFinite(y); }) || d <= 0 || b <= 0 ? null : { minX: h, minY: m, w: d, h: b }; }
    function mo(t) { var e = [], n = ""; for (var r = 0; r < t.length; r += 1) {
        var i = t.charCodeAt(r);
        i === 32 || i === 9 || i === 10 || i === 13 || i === 12 ? n.length > 0 && (e.push(n), n = "") : n += t.charAt(r);
    } return n.length > 0 && e.push(n), e; }
    function fo(t, e) { var n = String(t != null ? t : ""); if (!n.trim())
        return null; var r = Wr.get(n), i = "data:image/svg+xml;charset=utf-8,".concat(encodeURIComponent(n)); if (r) {
        if (Ye() && r.state === "loading")
            try {
                uo(i, r);
            }
            catch (a) {
                r.state = "error";
            }
        return r;
    } if (Ye())
        return null; var o = { state: "loading", texId: 0, width: 0, height: 0 }; Wr.set(n, o); var s = function (a) { Br(o, a), Ye() || e == null || e(); }, c = function () { o.state = "error", Ye() || e == null || e(); }; try {
        var a = $r(i);
        if (!a)
            return o;
        if (a && typeof a.then == "function") {
            if (Ye())
                return o;
            a.then(s).catch(c);
        }
        else
            s(a);
    }
    catch (a) {
        c();
    } return o; }
    function po(t, e, n) { var r = Math.max(0, e), i = Math.max(0, n), o = Ur(t); if (!o || r <= 0 || i <= 0)
        return { x: 0, y: 0, w: r, h: i }; var s = r / o.w, c = i / o.h, a = Math.min(s, c), h = Math.max(0, o.w * a), m = Math.max(0, o.h * a); return { x: Math.max(0, (r - h) / 2), y: Math.max(0, (i - m) / 2), w: h, h: m }; }
    function Xr(t, e, n) { var h, m, d, b; t.setPadding(n.EDGE_LEFT, 0), t.setPadding(n.EDGE_RIGHT, 0), t.setPadding(n.EDGE_TOP, 0), t.setPadding(n.EDGE_BOTTOM, 0); var r = Number((m = (h = e.attrs) == null ? void 0 : h.width) != null ? m : "0"), i = Number((b = (d = e.attrs) == null ? void 0 : d.height) != null ? b : "0"), o = Number.isFinite(r) && r > 0, s = Number.isFinite(i) && i > 0, c = o ? r : 300, a = s ? i : 150; (o || s) && (t.setAlignSelf(n.ALIGN_FLEX_START), t.setFlexGrow(0), t.setFlexShrink(0)), t.setWidth(c), t.setHeight(a), t.setMinWidth(Math.min(120, c)), t.setMinHeight(Math.min(80, a)); }
    function Yr(t) { var e = t.svgMarkup, n = t.container, r = t.w, i = t.h, o = t.requestRerender, s = ho(e), c = $t(n, "__svg"), a = c.__svgString, h = c.__w, m = c.__h, d = a !== s, b = fo(s, o); if (c.scale.set(1), c.position.set(0, 0), (b == null ? void 0 : b.state) === "ready" && b.texId > 0 && typeof c.image == "function") {
        if (d || h !== r || m !== i || c.__texId !== b.texId) {
            var w = po(s, r, i);
            vt(c), c.image(b.texId, w.x, w.y, w.w, w.h), c.__svgString = s, c.__w = r, c.__h = i, c.__texId = b.texId;
        }
        return;
    } vt(c); return; if (typeof y == "function") {
        if (d || h !== r || m !== i) {
            vt(c);
            var u = void 0;
            try {
                u = y.call(c, s);
            }
            catch (_) {
                u = null;
            }
            u && typeof u.then == "function" && u.then(function () { return o == null ? void 0 : o(); }).catch(function () { }), c.__svgString = s, c.__w = r, c.__h = i;
        }
        var w = Ur(s);
        if (w) {
            var u = r / w.w, _ = i / w.h, p = Math.min(u, _), S = w.w * p, O = w.h * p;
            c.scale.set(p), c.position.set(-w.minX * p + (r - S) / 2, -w.minY * p + (i - O) / 2);
        }
        return;
    } }
    function Kr(t, e, n) { var h, m, d, b; t.setPadding(n.EDGE_LEFT, 0), t.setPadding(n.EDGE_RIGHT, 0), t.setPadding(n.EDGE_TOP, 0), t.setPadding(n.EDGE_BOTTOM, 0); var r = Number((m = (h = e.attrs) == null ? void 0 : h.width) != null ? m : "0"), i = Number((b = (d = e.attrs) == null ? void 0 : d.height) != null ? b : "0"), o = Number.isFinite(r) && r > 0, s = Number.isFinite(i) && i > 0, c = o ? r : 300, a = s ? i : 150; (o || s) && (t.setAlignSelf(n.ALIGN_FLEX_START), t.setFlexGrow(0), t.setFlexShrink(0)), t.setWidth(c), t.setHeight(a), t.setMinWidth(Math.min(120, c)), t.setMinHeight(Math.min(80, a)); }
    function zr(t) { var e = t.graphics, n = t.container, r = t.w, i = t.h, o = t.theme, s = 1, c = s / 2; e.rect(c, c, Math.max(0, r - s), Math.max(0, i - s)), e.fill(16777215), e.stroke({ width: s, color: o.control.border, alignment: 0 }), e.moveTo(6, i - 6), e.lineTo(r - 6, 6), e.stroke({ width: 1, color: 0, alpha: .1 }); var a = Zt({ text: "canvas", fontFamily: o.fontFamily, fontSize: Math.max(10, Math.floor(o.fontSize * .85)), fill: o.mutedText, wordWrap: !1 }); a.position.set(8, 8 + Mt), n.addChild(a); }
    function jr(t, e, n) { var m, d, b, y, w, u; var r = String((d = (m = e.attrs) == null ? void 0 : m["data-root"]) != null ? d : "") === "1"; if (t.setFlexDirection(n.FLEX_DIRECTION_COLUMN), t.setAlignItems(n.ALIGN_STRETCH), r) {
        t.setPadding(n.EDGE_LEFT, 0), t.setPadding(n.EDGE_RIGHT, 0), t.setPadding(n.EDGE_TOP, 0), t.setPadding(n.EDGE_BOTTOM, 0), t.setAlignSelf(n.ALIGN_STRETCH), t.setFlexGrow(1), t.setFlexShrink(1), t.setMinWidth(0), t.setMinHeight(0);
        return;
    } t.setPadding(n.EDGE_LEFT, 8), t.setPadding(n.EDGE_RIGHT, 8), t.setPadding(n.EDGE_BOTTOM, 8), t.setPadding(n.EDGE_TOP, 34); var i = Number((y = (b = e.attrs) == null ? void 0 : b.width) != null ? y : "0"), o = Number((u = (w = e.attrs) == null ? void 0 : w.height) != null ? u : "0"), s = Number.isFinite(i) && i > 0, c = Number.isFinite(o) && o > 0, a = s ? i : 420, h = c ? o : 240; (s || c) && (t.setAlignSelf(n.ALIGN_FLEX_START), t.setFlexGrow(0), t.setFlexShrink(0)), t.setWidth(a), t.setHeight(h), t.setMinWidth(Math.min(200, a)), t.setMinHeight(Math.min(160, h)); }
    function Vr(t) { var y, w, u, _; var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.theme; if (String((w = (y = e.attrs) == null ? void 0 : y["data-root"]) != null ? w : "") === "1")
        return; var a = 1, h = a / 2; r.rect(h, h, Math.max(0, i - a), Math.max(0, o - a)), r.fill(16777215), r.stroke({ width: a, color: s.control.border, alignment: 0 }), r.rect(h, h, Math.max(0, i - a), 26), r.fill({ color: 0, alpha: .04 }); var d = String((_ = (u = e.attrs) == null ? void 0 : u.srcdoc) != null ? _ : "").trim().length > 0 ? "srcdoc" : "empty", b = Ht(n, "__title", function (p) { p.style = { fontFamily: s.fontFamily, fontSize: Math.max(10, Math.floor(s.fontSize * .85)), fill: s.mutedText, fontWeight: "400", wordWrap: !1 }; }); b.text = "iframe (".concat(d, ")"), b.position.set(8, 6 + Mt), n.eventMode = "static", n.cursor = "default", n.hitArea = new Rt(0, 0, Math.max(0, i), Math.max(0, o)); }
    function Jr(t, e, n) { var i, o; var r = ((o = (i = e.attrs) == null ? void 0 : i.type) != null ? o : "text").toLowerCase(); r === "checkbox" || r === "radio" ? (t.setPadding(n.EDGE_LEFT, 0), t.setPadding(n.EDGE_RIGHT, 0), t.setPadding(n.EDGE_TOP, 0), t.setPadding(n.EDGE_BOTTOM, 0), t.setWidth(16), t.setHeight(16), t.setMinWidth(16), t.setMargin(n.EDGE_RIGHT, 6)) : (t.setPadding(n.EDGE_TOP, 6), t.setPadding(n.EDGE_BOTTOM, 6), t.setHeight(36), t.setMinHeight(36), t.setMinWidth(220)); }
    function Qr(t) {
        var e_6, _a, e_7, _b;
        var X, j, f, v, H, q, rt, ot, K, et;
        var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.absX, c = t.absY, a = t.theme, h = t.textMeasure, m = t.uiState, d = t.getOrInitInputState, b = t.clamp, y = t.radioGroups, w = t.textDrags, u = t.requestPaint, _ = ((j = (X = e.attrs) == null ? void 0 : X.type) != null ? j : "text").toLowerCase(), p = e.key, S = p ? d(p, e.attrs) : void 0, O = (f = t.showCaret) != null ? f : !1, D = (v = t.caretPointerId) != null ? v : null, U = t.focusColor, T = t.getCursorColor;
        n.sortableChildren = !0;
        try {
            for (var _c = __values(n.children), _d = _c.next(); !_d.done; _d = _c.next()) {
                var C = _d.value;
                var V = C.label;
                V && (V.startsWith("__sel:") || V === "__caret") && (C.visible = !1);
            }
        }
        catch (e_6_1) { e_6 = { error: e_6_1 }; }
        finally {
            try {
                if (_d && !_d.done && (_a = _c.return)) _a.call(_c);
            }
            finally { if (e_6) throw e_6.error; }
        }
        var A = 8, G = 6 + Mt, k = 5, P = a.fontSize * 1.25;
        if (_ === "checkbox")
            r.rect(.5, .5, Math.max(0, i - 1), Math.max(0, o - 1)), r.fill(a.control.background), r.stroke({ width: 1, color: a.control.border }), S != null && S.indeterminate ? (r.moveTo(4, 4), r.lineTo(Math.max(4, i - 4), Math.max(4, o - 4)), r.stroke({ width: 2, color: a.control.accent }), r.moveTo(Math.max(4, i - 4), 4), r.lineTo(4, Math.max(4, o - 4)), r.stroke({ width: 2, color: a.control.accent })) : S != null && S.checked && (r.rect(3, 3, Math.max(0, i - 3 * 2), Math.max(0, o - 3 * 2)), r.fill(a.control.accent));
        else if (_ === "radio") {
            {
                var Y = Math.max(0, Math.min(i, o) / 2 - .5);
                r.circle(i / 2, o / 2, Y), r.fill(a.control.background), r.stroke({ width: 1, color: a.control.border });
            }
            if (S != null && S.checked) {
                var C = Math.max(0, Math.min(i, o) / 2 - 4.5);
                r.circle(i / 2, o / 2, C), r.fill(a.control.accent);
            }
        }
        else {
            var C = U != null ? 2 : 1, V = C / 2;
            a.control.radius > 0 ? r.roundRect(V, V, Math.max(0, i - C), Math.max(0, o - C), a.control.radius) : r.rect(V, V, Math.max(0, i - C), Math.max(0, o - C)), r.fill(a.control.background), r.stroke({ width: C, color: U != null ? U : a.control.border });
            var Y = _ === "password" ? "\u2022".repeat(((H = S == null ? void 0 : S.value) != null ? H : "").length) : (q = S == null ? void 0 : S.value) != null ? q : "", $ = Math.max(0, i - A * 2);
            p && m.fieldBounds.set(p, { x: s, y: c, w: i, h: o, innerLeft: A, innerTop: G, innerWidth: $, maxLines: k, isPassword: _ === "password" });
            var tt = we(Y, $, h), W = Te(tt, k), F = W.length > 0 ? W[W.length - 1].end : 0;
            if (p && S && typeof S.value == "string") {
                var lt = S.selections;
                if (lt && lt.size > 0)
                    try {
                        for (var _f = __values(lt.entries()), _g = _f.next(); !_g.done; _g = _f.next()) {
                            var _h = __read(_g.value, 2), gt = _h[0], g = _h[1];
                            var x = b((rt = g.start) != null ? rt : 0, 0, Y.length), L = b((ot = g.end) != null ? ot : x, 0, Y.length), I = b(Math.min(x, L), 0, F), E = b(Math.max(x, L), 0, F);
                            if (I === E)
                                continue;
                            var R = $t(n, "__sel:".concat(gt));
                            vt(R), R.zIndex = 0, R.visible = !0;
                            for (var M = 0; M < W.length; M++) {
                                var N = W[M], J = Math.max(I, N.start), Q = Math.min(E, N.end);
                                if (J >= Q)
                                    continue;
                                var z = A + h(Y.slice(N.start, J)), at = h(Y.slice(J, Q));
                                R.rect(z, G + M * P, at, P);
                            }
                            R.fill({ color: T(gt), alpha: .22 });
                        }
                    }
                    catch (e_7_1) { e_7 = { error: e_7_1 }; }
                    finally {
                        try {
                            if (_g && !_g.done && (_b = _f.return)) _b.call(_f);
                        }
                        finally { if (e_7) throw e_7.error; }
                    }
                if (O && D != null) {
                    var gt = (K = S.selections) == null ? void 0 : K.get(D), g = gt ? gt.end : 0, x = b(g, 0, F), L = Math.max(0, W.length - 1);
                    for (var M = 0; M < W.length; M++) {
                        var N = W[M];
                        if (x >= N.start && x <= N.end) {
                            L = M;
                            break;
                        }
                    }
                    var I = (et = W[L]) != null ? et : { start: 0, end: 0, text: "" }, E = A + h(Y.slice(I.start, x)), R = $t(n, "__caret");
                    vt(R), R.zIndex = 2, R.visible = !0, R.moveTo(E, G + L * P), R.lineTo(E, G + L * P + P), R.stroke({ width: 1, color: U != null ? U : a.control.focusBorder });
                }
            }
            var ut = W.map(function (lt) { return lt.text; }).join("\n"), dt = Ht(n, "__valueText", function (lt) { lt.style = { fontFamily: a.fontFamily, fontSize: a.fontSize, fill: a.text, fontWeight: "400", wordWrap: !1 }, lt.zIndex = 1; });
            dt.text = ut, dt.position.set(A, G);
        }
        p && (Jt(n), n.eventMode = "static", n.cursor = "text", n.on("pointerdown", function (C) {
            var e_8, _a, e_9, _b, e_10, _c;
            var Y, $, tt, W, F, ut, dt, lt, gt, g, x, L, I;
            if ((C == null ? void 0 : C.button) === 2)
                return;
            var V = t.getPointerId ? t.getPointerId(C) : Number((tt = ($ = C == null ? void 0 : C.pointerId) != null ? $ : (Y = C == null ? void 0 : C.data) == null ? void 0 : Y.pointerId) != null ? tt : 0);
            if (!(V <= 0)) {
                if (m.focusedKeyByPointer.set(V, p), m.keyboardOwnerPointerId = V, _ === "checkbox") {
                    var E = d(p, e.attrs), R = E.indeterminate === !0, M = E.checked === !0;
                    !M && !R ? (E.checked = !0, E.indeterminate = !1) : M && !R ? (E.checked = !1, E.indeterminate = !0) : (E.checked = !1, E.indeterminate = !1);
                }
                else if (_ === "radio") {
                    var R = "radio:".concat((F = (W = e.attrs) == null ? void 0 : W.name) != null ? F : "__default__"), M = (ut = y.get(R)) != null ? ut : [];
                    try {
                        for (var M_1 = __values(M), M_1_1 = M_1.next(); !M_1_1.done; M_1_1 = M_1.next()) {
                            var N = M_1_1.value;
                            var J = d(N, void 0);
                            J.checked = N === p;
                        }
                    }
                    catch (e_8_1) { e_8 = { error: e_8_1 }; }
                    finally {
                        try {
                            if (M_1_1 && !M_1_1.done && (_a = M_1.return)) _a.call(M_1);
                        }
                        finally { if (e_8) throw e_8.error; }
                    }
                }
                else {
                    var E = d(p, e.attrs);
                    if (typeof E.value == "string") {
                        try {
                            for (var _d = __values(m.inputs.entries()), _f = _d.next(); !_f.done; _f = _d.next()) {
                                var _g = __read(_f.value, 2), Z = _g[0], yt = _g[1];
                                Z !== p && ((dt = yt.selections) == null || dt.delete(V));
                            }
                        }
                        catch (e_9_1) { e_9 = { error: e_9_1 }; }
                        finally {
                            try {
                                if (_f && !_f.done && (_b = _d.return)) _b.call(_d);
                            }
                            finally { if (e_9) throw e_9.error; }
                        }
                        var R = _ === "password" ? "\u2022".repeat(E.value.length) : E.value, M = m.fieldBounds.get(p), N = (lt = M == null ? void 0 : M.innerWidth) != null ? lt : Math.max(0, i - A * 2), J = Te(we(R, N, h), k), Q = ((g = (gt = C.global) == null ? void 0 : gt.x) != null ? g : 0) - s - A, z = ((L = (x = C.global) == null ? void 0 : x.y) != null ? L : 0) - c - G, at = Ne({ fullText: R, lines: J, localX: Q, localY: z, lineHeight: P, measure: h });
                        E.selections || (E.selections = new Map), E.selections.set(V, { start: at, end: at });
                        try {
                            for (var _h = __values(w.entries()), _j = _h.next(); !_j.done; _j = _h.next()) {
                                var _k = __read(_j.value, 2), Z = _k[0], yt = _k[1];
                                yt.key === p && Z !== V && w.delete(Z);
                            }
                        }
                        catch (e_10_1) { e_10 = { error: e_10_1 }; }
                        finally {
                            try {
                                if (_j && !_j.done && (_c = _h.return)) _c.call(_h);
                            }
                            finally { if (e_10) throw e_10.error; }
                        }
                        w.set(V, { key: p, anchor: at });
                    }
                }
                (_ === "checkbox" || _ === "radio") && ((I = C.stopPropagation) == null || I.call(C)), u == null || u();
            }
        }), (_ === "checkbox" || _ === "radio") && (n.cursor = "pointer"), n.hitArea = new Rt(0, 0, Math.max(0, i), Math.max(0, o)));
    }
    function Zr(t, e) { t.setPadding(e.EDGE_TOP, 6), t.setPadding(e.EDGE_BOTTOM, 6), t.setHeight(108), t.setMinHeight(108), t.setMinWidth(220); }
    function qr(t) {
        var e_11, _a, e_12, _b;
        var ot, K, et, C, V, Y, $, tt;
        var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.absX, c = t.absY, a = t.theme, h = t.textMeasure, m = t.uiState, d = t.getOrInitInputState, b = t.clamp, y = t.textDrags, w = t.requestPaint, u = e.key, _ = u ? d(u, Se(ae({}, (ot = e.attrs) != null ? ot : {}), { type: "text" })) : void 0, p = (K = t.showCaret) != null ? K : !1, S = (et = t.caretPointerId) != null ? et : null, O = t.focusColor, D = t.getCursorColor;
        n.sortableChildren = !0;
        try {
            for (var _c = __values(n.children), _d = _c.next(); !_d.done; _d = _c.next()) {
                var W = _d.value;
                var F = W.label;
                F && (F.startsWith("__sel:") || F === "__caret") && (W.visible = !1);
            }
        }
        catch (e_11_1) { e_11 = { error: e_11_1 }; }
        finally {
            try {
                if (_d && !_d.done && (_a = _c.return)) _a.call(_c);
            }
            finally { if (e_11) throw e_11.error; }
        }
        var U = 8, T = 6 + Mt, A = 5, G = a.fontSize * 1.25, k = O != null ? 2 : 1, P = k / 2;
        a.control.radius > 0 ? r.roundRect(P, P, Math.max(0, i - k), Math.max(0, o - k), a.control.radius) : r.rect(P, P, Math.max(0, i - k), Math.max(0, o - k)), r.fill(a.control.background), r.stroke({ width: k, color: O != null ? O : a.control.border });
        var X = (C = _ == null ? void 0 : _.value) != null ? C : "", j = Math.max(0, i - U * 2);
        u && m.fieldBounds.set(u, { x: s, y: c, w: i, h: o, innerLeft: U, innerTop: T, innerWidth: j, maxLines: A, isPassword: !1 });
        var f = we(X, j, h), v = Te(f, A), H = v.length > 0 ? v[v.length - 1].end : 0;
        if (u && _ && typeof _.value == "string") {
            var W = _.selections;
            if (W && W.size > 0)
                try {
                    for (var _f = __values(W.entries()), _g = _f.next(); !_g.done; _g = _f.next()) {
                        var _h = __read(_g.value, 2), F = _h[0], ut = _h[1];
                        var dt = b((V = ut.start) != null ? V : 0, 0, X.length), lt = b((Y = ut.end) != null ? Y : dt, 0, X.length), gt = b(Math.min(dt, lt), 0, H), g = b(Math.max(dt, lt), 0, H);
                        if (gt === g)
                            continue;
                        var x = $t(n, "__sel:".concat(F));
                        vt(x), x.zIndex = 0, x.visible = !0;
                        for (var L = 0; L < v.length; L++) {
                            var I = v[L], E = Math.max(gt, I.start), R = Math.min(g, I.end);
                            if (E >= R)
                                continue;
                            var M = U + h(X.slice(I.start, E)), N = h(X.slice(E, R));
                            x.rect(M, T + L * G, N, G);
                        }
                        x.fill({ color: D(F), alpha: .22 });
                    }
                }
                catch (e_12_1) { e_12 = { error: e_12_1 }; }
                finally {
                    try {
                        if (_g && !_g.done && (_b = _f.return)) _b.call(_f);
                    }
                    finally { if (e_12) throw e_12.error; }
                }
            if (p && S != null) {
                var F = ($ = _.selections) == null ? void 0 : $.get(S), ut = F ? F.end : 0, dt = b(ut, 0, H), lt = Math.max(0, v.length - 1);
                for (var L = 0; L < v.length; L++) {
                    var I = v[L];
                    if (dt >= I.start && dt <= I.end) {
                        lt = L;
                        break;
                    }
                }
                var gt = (tt = v[lt]) != null ? tt : { start: 0, end: 0, text: "" }, g = U + h(X.slice(gt.start, dt)), x = $t(n, "__caret");
                vt(x), x.zIndex = 2, x.visible = !0, x.moveTo(g, T + lt * G), x.lineTo(g, T + lt * G + G), x.stroke({ width: 1, color: O != null ? O : a.control.focusBorder });
            }
        }
        var q = v.map(function (W) { return W.text; }).join("\n"), rt = Ht(n, "__valueText", function (W) { W.style = { fontFamily: a.fontFamily, fontSize: a.fontSize, fill: a.text, fontWeight: "400", wordWrap: !1 }, W.zIndex = 1; });
        rt.text = q, rt.position.set(U, T), u && (Jt(n), n.eventMode = "static", n.cursor = "text", n.hitArea = new Rt(0, 0, Math.max(0, i), Math.max(0, o)), n.on("pointerdown", function (W) {
            var e_13, _a, e_14, _b;
            var dt, lt, gt, g, x, L, I, E, R, M;
            if ((W == null ? void 0 : W.button) === 2)
                return;
            var F = t.getPointerId ? t.getPointerId(W) : Number((gt = (lt = W == null ? void 0 : W.pointerId) != null ? lt : (dt = W == null ? void 0 : W.data) == null ? void 0 : dt.pointerId) != null ? gt : 0);
            if (F <= 0)
                return;
            m.focusedKeyByPointer.set(F, u), m.keyboardOwnerPointerId = F;
            var ut = d(u, Se(ae({}, (g = e.attrs) != null ? g : {}), { type: "text" }));
            if (typeof ut.value == "string") {
                try {
                    for (var _c = __values(m.inputs.entries()), _d = _c.next(); !_d.done; _d = _c.next()) {
                        var _f = __read(_d.value, 2), kt = _f[0], Tt = _f[1];
                        kt !== u && ((x = Tt.selections) == null || x.delete(F));
                    }
                }
                catch (e_13_1) { e_13 = { error: e_13_1 }; }
                finally {
                    try {
                        if (_d && !_d.done && (_a = _c.return)) _a.call(_c);
                    }
                    finally { if (e_13) throw e_13.error; }
                }
                var N = m.fieldBounds.get(u), J = (L = N == null ? void 0 : N.innerWidth) != null ? L : Math.max(0, i - U * 2), Q = ut.value, z = Te(we(Q, J, h), A), at = ((E = (I = W.global) == null ? void 0 : I.x) != null ? E : 0) - s - U, Z = ((M = (R = W.global) == null ? void 0 : R.y) != null ? M : 0) - c - T, yt = Ne({ fullText: Q, lines: z, localX: at, localY: Z, lineHeight: G, measure: h });
                ut.selections || (ut.selections = new Map), ut.selections.set(F, { start: yt, end: yt });
                try {
                    for (var _g = __values(y.entries()), _h = _g.next(); !_h.done; _h = _g.next()) {
                        var _j = __read(_h.value, 2), kt = _j[0], Tt = _j[1];
                        Tt.key === u && kt !== F && y.delete(kt);
                    }
                }
                catch (e_14_1) { e_14 = { error: e_14_1 }; }
                finally {
                    try {
                        if (_h && !_h.done && (_b = _g.return)) _b.call(_g);
                    }
                    finally { if (e_14) throw e_14.error; }
                }
                y.set(F, { key: u, anchor: yt });
            }
            w == null || w();
        }));
    }
    function ti(t, e) { t.setFlexDirection(e.FLEX_DIRECTION_ROW), t.setAlignItems(e.ALIGN_CENTER), t.setJustifyContent(e.JUSTIFY_FLEX_START), t.setPadding(e.EDGE_LEFT, 8), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0); }
    function go(t, e, n, r, i) { t.circle(e, n, r), t.stroke({ width: 2, color: i }); var o = e + r * .65, s = n + r * .65, c = e + r * 1.55, a = n + r * 1.55; t.moveTo(o, s), t.lineTo(c, a), t.stroke({ width: 2, color: i }); }
    function ei(t, e) { t.setFlexDirection(e.FLEX_DIRECTION_ROW), t.setFlexWrap(e.WRAP_NO_WRAP), t.setAlignItems(e.ALIGN_CENTER), t.setJustifyContent(e.JUSTIFY_FLEX_START), t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0); }
    function ni(t, e) { t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0), t.setWidth(36), t.setHeight(36), t.setMinWidth(36), t.setMinHeight(36), t.setFlexGrow(0), t.setFlexShrink(0), t.setMargin(e.EDGE_RIGHT, 6); }
    function ri(t) { var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.theme, c = t.uiState, a = t.getPointerId, h = t.focusInputKey, m = t.requestPaint, d = function (y) { r.clear(); var w = 1, u = w / 2; s.control.button.radius > 0 ? r.roundRect(u, u, Math.max(0, i - w), Math.max(0, o - w), s.control.button.radius) : r.rect(u, u, Math.max(0, i - w), Math.max(0, o - w)), r.fill(y), r.stroke({ width: w, color: s.control.button.border }); var _ = i / 2 - 2, p = o / 2 - 2, S = Math.max(5, Math.min(7, Math.min(i, o) * .22)); go(r, _, p, S, s.text); }; d(s.control.button.fill), Jt(n), n.eventMode = "static", n.cursor = "pointer", n.hitArea = new Rt(0, 0, Math.max(0, i), Math.max(0, o)), n.on("pointerover", function () { return d(s.control.button.hoverFill); }), n.on("pointerout", function () { return d(s.control.button.fill); }), n.on("pointerdown", function (y) { var w; if ((y == null ? void 0 : y.button) !== 2) {
        if (d(s.control.button.activeFill), h) {
            var u = a(y);
            u > 0 && (c.focusedKeyByPointer.set(u, h), c.keyboardOwnerPointerId = u);
        }
        m == null || m(), (w = y.stopPropagation) == null || w.call(y);
    } }), n.on("pointerup", function () { return d(s.control.button.hoverFill); }); var b = e.attrs; }
    function ln(t, e) { var n = t.get(e); if (n)
        return n; var r = { x: 0, y: 0 }; return t.set(e, r), r; }
    function ii(t, e) { t.setPositionType(e.POSITION_TYPE_ABSOLUTE), t.setPosition(e.EDGE_LEFT, 0), t.setPosition(e.EDGE_TOP, 0), t.setAlignSelf(e.ALIGN_FLEX_START), t.setFlexGrow(0), t.setFlexShrink(0), t.setPadding(e.EDGE_LEFT, 12), t.setPadding(e.EDGE_RIGHT, 12), t.setPadding(e.EDGE_TOP, 12), t.setPadding(e.EDGE_BOTTOM, 12), t.setWidth(540), t.setMinWidth(360), t.setMinHeight(148); }
    function oi(t) { var D, U; var e = t.node, n = t.container, r = t.w, i = t.h, o = t.theme, s = t.selectedBy, c = t.getCursorColor, a = t.dialogStates, h = t.dialogDrags, m = t.bringToFront, d = t.requestPaint, b = e.key; if (!b)
        return; var y = s.get(b), w = y == null ? o.boxBorder : c(y), u = Math.max(0, Math.round(r)), _ = Math.max(0, Math.round(i)), p = $t(n, "__dialogBorder"); vt(p), p.rect(0, 0, u, _), p.fill({ color: 16777215, alpha: .8 }); var S = y == null ? 1 : 2, O = S / 2; p.rect(O, O, Math.max(0, u - S), Math.max(0, _ - S)), p.stroke({ width: S, color: w, alignment: 0 }), p.eventMode = "static", p.cursor = "move", p.hitArea = new Rt(0, 0, u, _), p.on("pointerdown", function (T) {
        var e_15, _a;
        var P, X, j, f, v, H, q, rt;
        var A = function (ot) { try {
            typeof console != "undefined" && typeof console.log == "function" && console.log("[dialog pointerdown] ".concat(ot));
        }
        catch (K) { } };
        if (A("start"), (T == null ? void 0 : T.button) === 2)
            return;
        A("pointer-id");
        var G = t.getPointerId ? t.getPointerId(T) : Number((j = (X = T == null ? void 0 : T.pointerId) != null ? X : (P = T == null ? void 0 : T.data) == null ? void 0 : P.pointerId) != null ? j : 0);
        if (G <= 0 || G <= 0)
            return;
        A("clear-other-drags");
        try {
            for (var _b = __values(h.entries()), _c = _b.next(); !_c.done; _c = _b.next()) {
                var _d = __read(_c.value, 2), ot = _d[0], K = _d[1];
                K.key === b && ot !== G && h.delete(ot);
            }
        }
        catch (e_15_1) { e_15 = { error: e_15_1 }; }
        finally {
            try {
                if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
            }
            finally { if (e_15) throw e_15.error; }
        }
        A("select"), s.set(b, G), A("bring-to-front"), m == null || m(b), A("state");
        var k = ln(a, b);
        A("set-drag"), h.set(G, { key: b, startGX: (v = (f = T.global) == null ? void 0 : f.x) != null ? v : 0, startGY: (q = (H = T.global) == null ? void 0 : H.y) != null ? q : 0, originX: k.x, originY: k.y }), A("request-paint"), d == null || d(), A("stop-propagation"), (rt = T.stopPropagation) == null || rt.call(T), A("done");
    }); {
        var T = n.getChildByLabel, A = (U = (D = T == null ? void 0 : T.call(n, "__children")) != null ? D : n.children.find(function (G) { return G && G.label === "__children"; })) != null ? U : null;
        if (A && p.parent === n) {
            var G = n.getChildIndex(A), k = Math.max(0, n.children.length - 1), P = Math.max(0, Math.min(G - 1, k));
            n.getChildIndex(p) > P && n.setChildIndex(p, P);
        }
    } }
    function vn(t, e, n) { var c; var r = t.get(e); if (r)
        return r; var i = Number((c = n == null ? void 0 : n.value) != null ? c : "0"), s = { value: Number.isFinite(i) ? i : 0 }; return t.set(e, s), s; }
    function si(t, e) { t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0), t.setHeight(36), t.setMinHeight(36), t.setMinWidth(140), t.setFlexGrow(0), t.setFlexShrink(0); }
    function bo(t, e, n) { return Math.max(e, Math.min(n, t)); }
    function Dn(t, e, n) { var i; var r = Number((i = t == null ? void 0 : t[e]) != null ? i : ""); return Number.isFinite(r) ? r : n; }
    function _o(t, e, n, r, i, o) { var c = e + 3, a = e + r - 3, h = n + 3, m = n + i - 3; t.moveTo(c, m), t.lineTo((c + a) / 2, h), t.lineTo(a, m), t.stroke({ width: 2, color: o }); }
    function yo(t, e, n, r, i, o) { var c = e + 3, a = e + r - 3, h = n + 3, m = n + i - 3; t.moveTo(c, h), t.lineTo((c + a) / 2, m), t.lineTo(a, h), t.stroke({ width: 2, color: o }); }
    function ai(t) { var j; var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.theme, c = t.getValue, a = t.setValue, h = t.requestPaint, m = e.key, d = e.attrs, b = Dn(d, "min", 0), y = Dn(d, "max", 255), w = Math.max(1e-9, Dn(d, "step", 1)), u = c(), _ = 1, p = _ / 2; r.rect(p, p, Math.max(0, i - _), Math.max(0, o - _)), r.fill(s.control.background), r.stroke({ width: _, color: s.control.border }); var S = 22, O = Math.max(0, i - S); r.moveTo(O + .5, 0), r.lineTo(O + .5, o), r.stroke({ width: 1, color: s.control.border, alignment: 0 }); var D = $t(n, "__arrows"); vt(D), _o(D, O, 0, S, o / 2, s.text), yo(D, O, o / 2, S, o / 2, s.text); var U = ((j = d == null ? void 0 : d.channel) != null ? j : "").toLowerCase(), T = U === "r" ? "R" : U === "g" ? "G" : U === "b" ? "B" : U === "a" ? "A" : "", A = Ht(n, "__valueText", function (f) { f.style = { fontFamily: s.fontFamily, fontSize: s.fontSize, fill: s.text, fontWeight: "400", wordWrap: !1 }; }); if (A.text = T ? "".concat(T, ": ").concat(Math.round(u)) : String(Math.round(u)), A.position.set(8, 9 + Mt), !m)
        return; var G = new Rt(O, 0, S, o / 2), k = new Rt(O, o / 2, S, o / 2), P = function (f) { var v = c(), H = bo(v + f * w, b, y); a(H), h == null || h(); }, X = $t(n, "__hit"); vt(X), X.eventMode = "static", X.cursor = "default", X.hitArea = new Rt(0, 0, Math.max(0, i), Math.max(0, o)), X.on("pointerdown", function (f) {
        var e_16, _a;
        var et, C, V, Y, $, tt;
        if ((f == null ? void 0 : f.button) === 2)
            return;
        var v = t.getPointerId ? t.getPointerId(f) : Number((V = (C = f == null ? void 0 : f.pointerId) != null ? C : (et = f == null ? void 0 : f.data) == null ? void 0 : et.pointerId) != null ? V : 0);
        if (v <= 0)
            return;
        var H = n.toLocal(f.global), q = (Y = H == null ? void 0 : H.x) != null ? Y : 0, rt = ($ = H == null ? void 0 : H.y) != null ? $ : 0, ot = G.contains(q, rt) ? 1 : k.contains(q, rt) ? -1 : null;
        if (!ot)
            return;
        P(ot);
        var K = t.numberHolds;
        if (K && m) {
            try {
                for (var _b = __values(K.entries()), _c = _b.next(); !_c.done; _c = _b.next()) {
                    var _d = __read(_c.value, 2), ut = _d[0], dt = _d[1];
                    ut !== v && (dt.timeoutId != null && window.clearTimeout(dt.timeoutId), dt.intervalId != null && window.clearInterval(dt.intervalId), K.delete(ut));
                }
            }
            catch (e_16_1) { e_16 = { error: e_16_1 }; }
            finally {
                try {
                    if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
                }
                finally { if (e_16) throw e_16.error; }
            }
            var W = K.get(v);
            W && (W.timeoutId != null && window.clearTimeout(W.timeoutId), W.intervalId != null && window.clearInterval(W.intervalId));
            var F_1 = { key: m, timeoutId: null, intervalId: null };
            F_1.timeoutId = window.setTimeout(function () { F_1.timeoutId = null, F_1.intervalId = window.setInterval(function () { P(ot); }, 250); }, 500), K.set(v, F_1);
        }
        (tt = f.stopPropagation) == null || tt.call(f);
    }); }
    var cn = null;
    function li() { return cn || (cn = new on({ data: de, label: "attribute-color-picker-colors", shrinkToFit: !1, usage: An.VERTEX | An.COPY_DST }), cn); }
    function ci(t, e, n) { var h, m, d, b; t.setPadding(n.EDGE_LEFT, 0), t.setPadding(n.EDGE_RIGHT, 0), t.setPadding(n.EDGE_TOP, 0), t.setPadding(n.EDGE_BOTTOM, 0); var r = Number((m = (h = e.attrs) == null ? void 0 : h.width) != null ? m : "0"), i = Number((b = (d = e.attrs) == null ? void 0 : d.height) != null ? b : "0"), o = Number.isFinite(r) && r > 0, s = Number.isFinite(i) && i > 0, c = o ? r : 240, a = s ? i : 200; (o || s) && (t.setAlignSelf(n.ALIGN_FLEX_START), t.setFlexGrow(0), t.setFlexShrink(0)), t.setWidth(c), t.setHeight(a), t.setMinWidth(Math.min(240, c)), t.setMinHeight(Math.min(200, a)); }
    function fe(t) { return Number.isFinite(t) ? Math.max(0, Math.min(255, Math.round(t))) : 0; }
    function un(t) { return fe(t).toString(16).padStart(2, "0"); }
    function xo(t, e, n, r, i, o, s, c) { var a = s - n, h = c - r, m = i - n, d = o - r, b = t - n, y = e - r, w = a * a + h * h, u = a * m + h * d, _ = a * b + h * y, p = m * m + d * d, S = m * b + d * y, O = 1 / (w * p - u * u), D = (p * _ - u * S) * O, U = (w * S - u * _) * O; return D >= 0 && U >= 0 && D + U <= 1; }
    function wo(t, e, n, r, i, o, s, c) { var a = i - n, h = o - r, m = s - n, d = c - r, b = t - n, y = e - r, w = a * d - m * h; if (Math.abs(w) < 1e-9)
        return { w0: 1, w1: 0, w2: 0 }; var u = (b * d - m * y) / w, _ = (a * y - b * h) / w; return { w0: 1 - u - _, w1: u, w2: _ }; }
    var To = { name: "solid-out", fragment: { main: "\n      outColor = vec4(1.0);\n    " } }, dn = null;
    function Eo() { if (dn)
        return dn; var t = { name: "color-picker-vertex-color", bits: [cr, ur, lr, To] }; return dn = new Ue({ glProgram: t, resources: {} }), dn; }
    function ui(t, e, n) { var r = new Float32Array(12), i = [-90, -30, 30, 90, 150, 210]; for (var o = 0; o < 6; o++) {
        var s = i[o] * Math.PI / 180;
        r[o * 2 + 0] = t + Math.cos(s) * n, r[o * 2 + 1] = e + Math.sin(s) * n;
    } return r; }
    var de = new Uint8Array([255, 0, 0, 255, 128, 128, 0, 255, 0, 255, 0, 255, 0, 128, 128, 255, 0, 0, 255, 255, 128, 0, 128, 255]), Le = new Uint32Array([0, 1, 2, 0, 2, 3, 0, 3, 4, 0, 4, 5]);
    function Gn(t) { var e = t.lx, n = t.ly, r = t.w, i = t.h, o = 10, s = Math.max(0, r - o * 2), c = Math.max(0, i - o * 2), a = o + s / 2, h = o + c / 2, m = Math.max(0, Math.min(s, c) / 2 - 2), d = ui(a, h, m); for (var b = 0; b < Le.length; b += 3) {
        var y = Le[b + 0], w = Le[b + 1], u = Le[b + 2], _ = d[y * 2 + 0], p = d[y * 2 + 1], S = d[w * 2 + 0], O = d[w * 2 + 1], D = d[u * 2 + 0], U = d[u * 2 + 1];
        if (!xo(e, n, _, p, S, O, D, U))
            continue;
        var T = wo(e, n, _, p, S, O, D, U), A = y * 4, G = w * 4, k = u * 4, P = T.w0 * de[A + 0] + T.w1 * de[G + 0] + T.w2 * de[k + 0], X = T.w0 * de[A + 1] + T.w1 * de[G + 1] + T.w2 * de[k + 1], j = T.w0 * de[A + 2] + T.w1 * de[G + 2] + T.w2 * de[k + 2];
        return { r: fe(P), g: fe(X), b: fe(j) };
    } return null; }
    function di(t) { var K, et; var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.theme, c = t.rgb, a = t.setRgb, h = t.alpha, m = t.setAlpha, d = t.pick, b = t.setPick, y = t.requestPaint, w = t.getPointerId, u = t.setDraggingPointerId, _ = 1, p = _ / 2; r.rect(p, p, Math.max(0, i - _), Math.max(0, o - _)), r.fill(16777215), r.stroke({ width: _, color: s.control.border, alignment: 0 }); var S = 10, O = Math.max(0, i - S * 2), D = Math.max(0, o - S * 2), U = S + O / 2, T = S + D / 2, A = Math.max(0, Math.min(O, D) / 2 - 2), G = ui(U, T, A), k = "".concat(Math.round(i), "x").concat(Math.round(o)), P = n.getChildByLabel, X = P ? P.call(n, "__mesh") : n.children.find(function (C) { return (C == null ? void 0 : C.label) === "__mesh"; }); if (X) {
        if (X.__sizeKey !== k) {
            var C = new Float32Array(G.length), V = new Ce({ positions: G, uvs: C, indices: Le });
            V.addAttribute("aColor", { buffer: li(), format: "unorm8x4", stride: 4, offset: 0 });
            try {
                (et = (K = X.geometry) == null ? void 0 : K.destroy) == null || et.call(K);
            }
            catch (Y) { }
            X.geometry = V, X.__sizeKey = k;
        }
    }
    else {
        var C = new Float32Array(G.length), V = new Ce({ positions: G, uvs: C, indices: Le });
        V.addAttribute("aColor", { buffer: li(), format: "unorm8x4", stride: 4, offset: 0 }), X = new rn({ geometry: V, shader: Eo() }), X.label = "__mesh", n.addChild(X), X.__sizeKey = k;
    } X.removeAllListeners(), X.eventMode = "static", X.cursor = "crosshair", X.hitArea = new Rt(S, S, O, D), X.on("pointerdown", function (C) { var F, ut, dt; if ((C == null ? void 0 : C.button) === 2)
        return; var V = w(C); if (V <= 0)
        return; var Y = n.toLocal(C.global), $ = (F = Y == null ? void 0 : Y.x) != null ? F : 0, tt = (ut = Y == null ? void 0 : Y.y) != null ? ut : 0, W = Gn({ lx: $, ly: tt, w: i, h: o }); W && (b({ x: $, y: tt }), a(W), u(V), y == null || y(), (dt = C.stopPropagation) == null || dt.call(C)); }); {
        var C = $t(n, "__border");
        vt(C), C.moveTo(G[0], G[1]);
        for (var V = 1; V < 6; V++)
            C.lineTo(G[V * 2 + 0], G[V * 2 + 1]);
        C.closePath(), C.stroke({ width: 2, color: 0 });
    } var j = $t(n, "__overlay"); vt(j); var f = 44, v = 18, H = Math.max(S, i - S - f), q = S; j.rect(H, q, f, v), j.fill({ color: fe(c.r) << 16 | fe(c.g) << 8 | fe(c.b), alpha: Math.max(0, Math.min(1, fe(h) / 255)) }), j.rect(H + .5, q + .5, f - 1, v - 1), j.stroke({ width: 1, color: s.control.border, alignment: 0 }), d && (j.circle(d.x, d.y, 4), j.stroke({ width: 2, color: 16777215 }), j.circle(d.x, d.y, 4), j.stroke({ width: 1, color: 0 })); var rt = "#".concat(un(c.r)).concat(un(c.g)).concat(un(c.b)).concat(un(h)).toUpperCase(), ot = Ht(n, "__label", function (C) { C.style = { fontFamily: s.fontFamily, fontSize: Math.max(10, Math.floor(s.fontSize * .75)), fill: s.mutedText, fontWeight: "400", wordWrap: !1 }; }); ot.text = rt, ot.position.set(S, Math.max(S, o - S - ot.height)), m && m(fe(h)); }
    function Ee(t, e, n) { var r = t.get(e); if (r)
        return r; var i = { selectedIndex: Math.max(0, n | 0), open: !1 }; return t.set(e, i), i; }
    function hi(t, e) { t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0), t.setHeight(36), t.setMinHeight(36), t.setMinWidth(220); }
    function Io(t, e, n, r, i, o) { var c = e + 4, a = e + r - 4, h = n + 4, m = n + i - 4; t.moveTo(c, (h + m) / 2 - 2), t.lineTo((c + a) / 2, (h + m) / 2 + 2), t.lineTo(a, (h + m) / 2 - 2), t.stroke({ width: 2, color: o }); }
    function Hn(t) {
        var r;
        var n = String((r = t == null ? void 0 : t["data-options"]) != null ? r : "").split("\n").map(function (i) { return i.trim(); }).filter(function (i) { return i.length > 0; });
        return n.length > 0 ? n : ["(empty)"];
    }
    function Mo(t) { var n; var e = Number((n = t == null ? void 0 : t["data-selected-index"]) != null ? n : "0"); return Number.isFinite(e) ? Math.max(0, e | 0) : 0; }
    function hn(t) { var j; var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.absX, c = t.absY, a = t.theme, h = t.selectStates, m = t.uiState, d = t.getPointerId, b = t.getCursorColor, y = t.requestPaint, w = t.requestOverlayPaint, u = t.popupSink, _ = e.key; if (!_)
        return; var p = Hn(e.attrs), S = Mo(e.attrs), O = Ee(h, _, S); O.selectedIndex = Math.max(0, Math.min(p.length - 1, O.selectedIndex | 0)); var D = (function () {
        var e_17, _a;
        var f = m.keyboardOwnerPointerId;
        if (m.focusedKeyByPointer.get(f) === _)
            return f;
        try {
            for (var _b = __values(m.focusedKeyByPointer.entries()), _c = _b.next(); !_c.done; _c = _b.next()) {
                var _d = __read(_c.value, 2), v = _d[0], H = _d[1];
                if (H === _)
                    return v;
            }
        }
        catch (e_17_1) { e_17 = { error: e_17_1 }; }
        finally {
            try {
                if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
            }
            finally { if (e_17) throw e_17.error; }
        }
        return null;
    })(), U = D != null ? b(D) : null, T = U != null ? 2 : 1, A = T / 2; a.control.radius > 0 ? r.roundRect(A, A, Math.max(0, i - T), Math.max(0, o - T), a.control.radius) : r.rect(A, A, Math.max(0, i - T), Math.max(0, o - T)), r.fill(a.control.background), r.stroke({ width: T, color: U != null ? U : a.control.border }); var G = 22, k = Math.max(0, i - G); r.moveTo(k + .5, 0), r.lineTo(k + .5, o), r.stroke({ width: 1, color: a.control.border, alignment: 0 }), Io(r, k, 0, G, o, a.text); var P = (j = p[O.selectedIndex]) != null ? j : "", X = Ht(n, "__label", function (f) { f.style = { fontFamily: a.fontFamily, fontSize: a.fontSize, fill: a.text, fontWeight: "400", wordWrap: !1 }; }); X.text = P, X.position.set(8, 9 + Mt), Jt(n), n.eventMode = "static", n.cursor = "pointer", n.hitArea = new Rt(0, 0, Math.max(0, i), Math.max(0, o)), n.on("pointerdown", function (f) { var H, q; if ((f == null ? void 0 : f.button) === 2)
        return; var v = d(f); v <= 0 || (m.focusedKeyByPointer.set(v, _), m.keyboardOwnerPointerId = v, O.open = !O.open, (H = w != null ? w : y) == null || H(), (q = f.stopPropagation) == null || q.call(f)); }), O.open && u.push({ key: _, absX: s, absY: c, w: i, h: o, options: p, selectedIndex: O.selectedIndex }); }
    function Wn(t) { var S; var e = t.popup, n = t.stage, r = t.theme, i = t.selectStates, o = t.uiState, s = t.getPointerId, c = t.requestPaint, a = t.viewportW, h = t.viewportH, m = 30, b = Math.min(7, e.options.length), y = b * m, w = e.absX, u = e.absY + e.h; w = Math.max(0, Math.min(w, Math.max(0, a - e.w))), u + y > h - 4 && (u = e.absY - y), u = Math.max(0, Math.min(u, Math.max(0, h - y))); var _ = new Ot; _.position.set(w, u), n.addChild(_); var p = new Pt; p.rect(0, 0, e.w, y), p.fill(16777215), p.rect(.5, .5, Math.max(0, e.w - 1), Math.max(0, y - 1)), p.stroke({ width: 1, color: r.control.border, alignment: 0 }), _.addChild(p), _.eventMode = "static", _.cursor = "pointer", _.hitArea = new Rt(0, 0, e.w, y), _.on("pointerdown", function (O) { var P, X, j; if ((O == null ? void 0 : O.button) === 2)
        return; var D = s(O), U = _.toLocal(O.global), T = (P = U == null ? void 0 : U.x) != null ? P : -1, A = (X = U == null ? void 0 : U.y) != null ? X : -1; if (T < 0 || T > e.w || A < 0 || A > y)
        return; var G = Math.max(0, Math.min(e.options.length - 1, Math.floor(A / m))), k = i.get(e.key); k && (k.selectedIndex = G, k.open = !1), D > 0 && (o.focusedKeyByPointer.set(D, e.key), o.keyboardOwnerPointerId = D), c == null || c(), (j = O.stopPropagation) == null || j.call(O); }); for (var O = 0; O < b; O++) {
        var D = O * m;
        if (O === e.selectedIndex) {
            var T = new Pt;
            T.rect(1, D + 1, Math.max(0, e.w - 2), m - 2), T.fill({ color: 0, alpha: .06 }), _.addChild(T);
        }
        var U = Zt({ text: (S = e.options[O]) != null ? S : "", fontFamily: r.fontFamily, fontSize: r.fontSize, fill: r.text, wordWrap: !1 });
        U.position.set(8, D + 7 + Mt), _.addChild(U);
    } }
    function Wt(t, e, n) { var r = Number.isFinite(t) ? t | 0 : 0; return Math.max(e, Math.min(n, r)); }
    function qt(t) { var e = Wt(t, 0, 99); return e < 10 ? "0".concat(e) : String(e); }
    function se(t, e, n) { var r = Number(t); if (!Number.isFinite(r))
        return null; var i = Math.trunc(r); return i < e || i > n ? null : i; }
    function pn(t) { if (t.length !== 4)
        return null; for (var r = 0; r < t.length; r += 1) {
        var i = t.charCodeAt(r);
        if (i < 48 || i > 57)
            return null;
    } var e = Number(t); if (!Number.isFinite(e))
        return null; var n = e - 2e3; return n < 0 || n > 99 ? null : n; }
    function So(t) { var e = String(t != null ? t : "").trim().split(":"); if (e.length !== 2 && e.length !== 3)
        return null; var n = se(e[0], 0, 23), r = se(e[1], 0, 59), i = e.length === 3 ? se(e[2], 0, 59) : 0; return n == null || r == null || i == null ? null : { hour: n, minute: r, second: i }; }
    function Po(t) { var e = String(t != null ? t : "").trim().split("-"); if (e.length !== 2)
        return null; var n = pn(e[0]), r = se(e[1], 1, 12); return n == null || r == null ? null : { year2: n, month: r }; }
    function ko(t) { var e = String(t != null ? t : "").trim().split("-"); if (e.length !== 3)
        return null; var n = pn(e[0]), r = se(e[1], 1, 12), i = se(e[2], 1, 31); if (n == null || r == null || i == null)
        return null; var o = Wt(Math.floor((i - 1) / 7) + 1, 1, 4); return { year2: n, month: r, weekIndex: o }; }
    function Ro(t) { var e = String(t != null ? t : "").trim(), n = e.indexOf("-W"); if (n < 0)
        return null; var r = pn(e.slice(0, n)), i = se(e.slice(n + 2), 1, 48); if (r == null || i == null)
        return null; var o = Wt(Math.floor((i - 1) / 4) + 1, 1, 12), s = Wt((i - 1) % 4 + 1, 1, 4); return { year2: r, month: o, weekIndex: s }; }
    function Oo(t) { var e = String(t != null ? t : "").trim(), n = e.indexOf("T"); if (n < 0 && (n = e.indexOf(" ")), n < 0)
        return null; var r = e.slice(0, n).split("-"), i = e.slice(n + 1).split(":"); if (r.length !== 3 || i.length !== 2 && i.length !== 3)
        return null; var o = pn(r[0]), s = se(r[1], 1, 12), c = se(r[2], 1, 31), a = se(i[0], 0, 23), h = se(i[1], 0, 59), m = i.length === 3 ? se(i[2], 0, 59) : 0; if (o == null || s == null || c == null || a == null || h == null || m == null)
        return null; var d = Wt(Math.floor((c - 1) / 7) + 1, 1, 4); return { year2: o, month: s, weekIndex: d, hour: a, minute: h, second: m }; }
    function mn(t) { return "20".concat(qt(t.year2), "-").concat(qt(t.month)); }
    function Co(t) { return (Wt(t.month, 1, 12) - 1) * 4 + Wt(t.weekIndex, 1, 4); }
    function fn(t) { return "20".concat(qt(t.year2), "-W").concat(qt(Co(t))); }
    function De(t) { var e = (Wt(t.weekIndex, 1, 4) - 1) * 7 + 1; return "20".concat(qt(t.year2), "-").concat(qt(t.month), "-").concat(qt(e)); }
    function je(t) { return "".concat(qt(t.hour), ":").concat(qt(t.minute), ":").concat(qt(t.second)); }
    function Ke(t) { return "".concat(De(t), "T").concat(je(t)); }
    function Ao(t) { var m; var e = t.map, n = t.yearSliderOwners, r = t.inputKey, i = t.kind, o = t.attrs, s = e.get(r); if (s)
        return s.kind = i, s; var c = new Date, a = { kind: i, year2: Wt(c.getFullYear() - 2e3, 0, 99), month: Wt(c.getMonth() + 1, 1, 12), weekIndex: 1, hour: Wt(c.getHours(), 0, 23), minute: Wt(c.getMinutes(), 0, 59), second: Wt(c.getSeconds(), 0, 59), openPanel: null, openYear: !1, openMonthGrid: !1, yearSliderKey: "".concat(r, ":year-slider") }, h = String((m = o == null ? void 0 : o.value) != null ? m : ""); if (h.trim().length > 0) {
        if (i === "time") {
            var d = So(h);
            d && (a.hour = d.hour, a.minute = d.minute, a.second = d.second);
        }
        else if (i === "month") {
            var d = Po(h);
            d && (a.year2 = d.year2, a.month = d.month);
        }
        else if (i === "week") {
            var d = Ro(h);
            d && (a.year2 = d.year2, a.month = d.month, a.weekIndex = d.weekIndex);
        }
        else if (i === "date") {
            var d = ko(h);
            d && (a.year2 = d.year2, a.month = d.month, a.weekIndex = d.weekIndex);
        }
        else if (i === "datetime-local") {
            var d = Oo(h);
            d && (a.year2 = d.year2, a.month = d.month, a.weekIndex = d.weekIndex, a.hour = d.hour, a.minute = d.minute, a.second = d.second);
        }
    } return e.set(r, a), n.set(a.yearSliderKey, r), a; }
    function fi(t, e, n) { t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0), t.setHeight(36), t.setMinHeight(36), t.setMinWidth(n === "datetime-local" ? 340 : 220); }
    function No(t, e, n, r, i) { var o = i != null ? 2 : 1, s = o / 2; e.control.radius > 0 ? t.roundRect(s, s, Math.max(0, n - o), Math.max(0, r - o), e.control.radius) : t.rect(s, s, Math.max(0, n - o), Math.max(0, r - o)), t.fill(e.control.background), t.stroke({ width: o, color: i != null ? i : e.control.border }); }
    function mi(t, e, n, r, i) { var o = e + r / 2, s = n + r / 2; t.moveTo(e, s - 2), t.lineTo(o, s + 2), t.lineTo(e + r, s - 2), t.stroke({ width: 2, color: i }); }
    function pi(t) { var k, P; var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.absX, c = t.absY, a = t.theme, h = t.uiState, m = t.getPointerId, d = t.getCursorColor, b = t.temporalStates, y = t.yearSliderOwners, w = t.getOrInitInputValue, u = t.requestPaint, _ = t.requestOverlayPaint, p = t.popupSink, S = e.key; if (!S || !e.tagName)
        return; var O = e.tagName === "timeinput" ? "time" : e.tagName === "monthinput" ? "month" : e.tagName === "weekinput" ? "week" : e.tagName === "dateinput" ? "date" : "datetime-local", D = Ao({ map: b, yearSliderOwners: y, inputKey: S, kind: O, attrs: e.attrs }), U = w(S, Se(ae({}, (k = e.attrs) != null ? k : {}), { type: "text" })); O === "time" ? U.value = je(D) : O === "month" ? U.value = mn(D) : O === "week" ? U.value = fn(D) : O === "date" ? U.value = De(D) : U.value = Ke(D); var T = (function () {
        var e_18, _a;
        var X = h.keyboardOwnerPointerId;
        if (h.focusedKeyByPointer.get(X) === S)
            return X;
        try {
            for (var _b = __values(h.focusedKeyByPointer.entries()), _c = _b.next(); !_c.done; _c = _b.next()) {
                var _d = __read(_c.value, 2), j = _d[0], f = _d[1];
                if (f === S)
                    return j;
            }
        }
        catch (e_18_1) { e_18 = { error: e_18_1 }; }
        finally {
            try {
                if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
            }
            finally { if (e_18) throw e_18.error; }
        }
        return null;
    })(), A = T != null ? d(T) : null; No(r, a, i, o, A); var G = 8; if (O !== "datetime-local") {
        var X = (P = U.value) != null ? P : "", j = Ht(n, "__shown", function (H) { H.style = { fontFamily: a.fontFamily, fontSize: a.fontSize, fill: a.text, fontWeight: "400", wordWrap: !1 }; });
        j.text = X, j.visible = !0, j.position.set(G, 9 + Mt);
        var f = n.getChildByLabel ? n.getChildByLabel("__date") : n.children.find(function (H) { return (H == null ? void 0 : H.label) === "__date"; }), v = n.getChildByLabel ? n.getChildByLabel("__time") : n.children.find(function (H) { return (H == null ? void 0 : H.label) === "__time"; });
        f && (f.visible = !1), v && (v.visible = !1), mi(r, Math.max(0, i - 18), 11, 10, a.text);
    }
    else {
        var X = Math.max(0, Math.round(i * .52));
        r.moveTo(X + .5, 0), r.lineTo(X + .5, o), r.stroke({ width: 1, color: a.control.border, alignment: 0 });
        var j = De(D), f = je(D), v = Ht(n, "__date", function (rt) { rt.style = { fontFamily: a.fontFamily, fontSize: a.fontSize, fill: a.text, fontWeight: "400", wordWrap: !1 }; });
        v.text = j, v.visible = !0, v.position.set(G, 9 + Mt);
        var H = Ht(n, "__time", function (rt) { rt.style = { fontFamily: a.fontFamily, fontSize: a.fontSize, fill: a.text, fontWeight: "400", wordWrap: !1 }; });
        H.text = f, H.visible = !0, H.position.set(X + G, 9 + Mt);
        var q = n.getChildByLabel ? n.getChildByLabel("__shown") : n.children.find(function (rt) { return (rt == null ? void 0 : rt.label) === "__shown"; });
        q && (q.visible = !1), mi(r, Math.max(X + 0, X + (i - X) - 18), 11, 10, a.text);
    } Jt(n), n.eventMode = "static", n.cursor = "pointer", n.hitArea = new Rt(0, 0, Math.max(0, i), Math.max(0, o)), n.on("pointerdown", function (X) { var f, v, H, q; if ((X == null ? void 0 : X.button) === 2)
        return; var j = m(X); if (!(j <= 0)) {
        if (h.focusedKeyByPointer.set(j, S), h.keyboardOwnerPointerId = j, O !== "datetime-local")
            D.openPanel = D.openPanel ? null : O === "time" ? "time" : O === "month" ? "month" : "week", D.openYear = !1, D.openMonthGrid = !1;
        else {
            var K = ((v = (f = X.global) == null ? void 0 : f.x) != null ? v : 0) - s <= i * .52;
            D.openPanel = K ? D.openPanel === "week" ? null : "week" : D.openPanel === "time" ? null : "time", D.openYear = !1, D.openMonthGrid = !1;
        }
        b.set(S, D), (H = _ != null ? _ : u) == null || H(), (q = X.stopPropagation) == null || q.call(X);
    } }), D.openPanel === "month" ? p.push({ kind: "month-panel", inputKey: S, absX: s, absY: c, anchorW: i, anchorH: o }) : D.openPanel === "week" ? p.push({ kind: "week-panel", inputKey: S, absX: s, absY: c, anchorW: i, anchorH: o }) : D.openPanel === "time" && p.push({ kind: "time-panel", inputKey: S, absX: s, absY: c, anchorW: i, anchorH: o }); }
    function ze(t, e, n, r) { t.rect(0, 0, n, r), t.fill(e.control.background), t.rect(.5, .5, Math.max(0, n - 1), Math.max(0, r - 1)), t.stroke({ width: 1, color: e.control.border, alignment: 0 }); }
    function Lo(t) { var e = t.stage, n = t.theme, r = t.popup, i = t.st, o = t.viewportW, s = t.viewportH, c = t.getPointerId, a = t.requestPaint, h = t.onPick, m = 4, d = 3, b = 44, y = 34, w = 8, u = w * 2 + m * b, _ = w * 2 + d * y, p = r.absX, S = r.absY + r.anchorH; p = Math.max(0, Math.min(p, Math.max(0, o - u))), S + _ > s - 4 && (S = r.absY - _), S = Math.max(0, Math.min(S, Math.max(0, s - _))); var O = new Ot; O.position.set(p, S), e.addChild(O); var D = new Pt; ze(D, n, u, _), O.addChild(D); for (var U = 0; U < 12; U++) {
        var T = U + 1, A = w + U % m * b, G = w + Math.floor(U / m) * y;
        if (T === i.month) {
            var P = new Pt;
            P.rect(A + 1, G + 1, b - 2, y - 2), P.fill({ color: 0, alpha: .06 }), O.addChild(P);
        }
        var k = Zt({ text: String(T), fontFamily: n.fontFamily, fontSize: n.fontSize, fill: n.text, wordWrap: !1 });
        k.position.set(A + 14, G + 8 + Mt), O.addChild(k), D.rect(A, G, b, y), D.stroke({ width: 1, color: n.control.border, alignment: 0 });
    } O.eventMode = "static", O.cursor = "pointer", O.hitArea = new Rt(0, 0, u, _), O.on("pointerdown", function (U) { var q, rt, ot; if ((U == null ? void 0 : U.button) === 2 || c(U) <= 0)
        return; var A = O.toLocal(U.global), G = (q = A == null ? void 0 : A.x) != null ? q : -1, k = (rt = A == null ? void 0 : A.y) != null ? rt : -1, P = G - w, X = k - w; if (P < 0 || X < 0)
        return; var j = Math.floor(P / b), f = Math.floor(X / y); if (j < 0 || j >= m || f < 0 || f >= d)
        return; var H = f * m + j + 1; H < 1 || H > 12 || (h(H), a == null || a(), (ot = U.stopPropagation) == null || ot.call(U)); }); }
    function Do(t) {
        var e_19, _a;
        var e = t.stage, n = t.theme, r = t.popup, i = t.st, o = t.viewportW, s = t.viewportH, c = t.sliders, a = t.sliderBounds, h = t.sliderDrags, m = t.getPointerId, d = t.requestPaint, b = t.onChange, y = 10, w = 250, u = 78, _ = r.absX, p = r.absY;
        _ = r.absX + r.anchorW + 6, p = r.absY, _ = Math.max(0, Math.min(_, Math.max(0, o - w))), p = Math.max(0, Math.min(p, Math.max(0, s - u)));
        var S = new Ot;
        S.position.set(_, p), e.addChild(S);
        var O = new Pt;
        ze(O, n, w, u), S.addChild(O);
        var D = Zt({ text: "20".concat(qt(i.year2)), fontFamily: n.fontFamily, fontSize: n.fontSize, fill: n.text, wordWrap: !1 });
        D.position.set(y, 8 + Mt), S.addChild(D);
        var U = i.yearSliderKey, T = Math.max(0, Math.min(1, Wt(i.year2, 0, 99) / 99)), A = Pe(c, U, { value: String(T) }), G = !1;
        try {
            for (var _b = __values(h.values()), _c = _b.next(); !_c.done; _c = _b.next()) {
                var j = _c.value;
                if (j.key === U) {
                    G = !0;
                    break;
                }
            }
        }
        catch (e_19_1) { e_19 = { error: e_19_1 }; }
        finally {
            try {
                if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
            }
            finally { if (e_19) throw e_19.error; }
        }
        G || (A.value = T);
        var k = new Ot;
        k.position.set(y, 40), S.addChild(k);
        var P = new Pt;
        k.addChild(P), an({ node: { key: U, attrs: { value: String(A.value) } }, container: k, graphics: P, w: w - y * 2, h: 14, absX: _ + y, absY: p + 40, theme: { text: n.text, control: { progress: n.control.progress } }, sliderStates: c, sliderBounds: a, sliderDrags: h, requestPaint: d, getPointerId: m });
        var X = Wt(Math.round(A.value * 99), 0, 99);
        X !== i.year2 && b(X), S.eventMode = "static", S.hitArea = new Rt(0, 0, w, u), S.on("pointerdown", function (j) { var f; (f = j.stopPropagation) == null || f.call(j); });
    }
    function vo(t) { var e = t.panel, n = t.theme, r = t.x, i = t.y, o = t.w, s = t.st, c = t.onPick, a = 30, h = 6, m = []; for (var d = 0; d < 4; d++) {
        var b = d + 1, y = i + d * (a + h), w = new Pt;
        w.rect(r, y, o, a), w.fill({ color: 0, alpha: b === s.weekIndex ? .06 : .03 }), w.rect(r + .5, y + .5, Math.max(0, o - 1), Math.max(0, a - 1)), w.stroke({ width: 1, color: n.control.border, alignment: 0 }), e.addChild(w);
        var u = (Wt(s.month, 1, 12) - 1) * 4 + b, _ = Zt({ text: "".concat(b, " [").concat(qt(u), "]"), fontFamily: n.fontFamily, fontSize: n.fontSize, fill: n.text, wordWrap: !1 });
        _.position.set(r + 10, y + 7 + Mt), e.addChild(_), m.push({ x: r, y: y, w: o, h: a, weekIndex: b });
    } return { hitRects: m }; }
    function Fn(t) {
        var e_20, _a, e_21, _b;
        var O, D, U, T, A, G;
        var e = t.popups, n = t.stage, r = t.theme, i = t.viewportW, o = t.viewportH, s = t.temporalStates, c = t.getOrInitInputValue, a = t.sliders, h = t.sliderBounds, m = t.sliderDrags, d = t.selects, b = t.selectPopups, y = t.getCursorColor, w = t.uiFocus, u = t.getPointerId, _ = t.requestPaint, p = t.requestOverlayPaint, S = [];
        var _loop_1 = function (k) {
            var P = s.get(k.inputKey);
            if (P) {
                if (k.kind === "month-panel") {
                    var et = k.absX, C = k.absY + k.anchorH;
                    et = Math.max(0, Math.min(et, Math.max(0, i - 196))), C + 156 > o - 4 && (C = k.absY - 156), C = Math.max(0, Math.min(C, Math.max(0, o - 156)));
                    var V_1 = new Ot;
                    V_1.position.set(et, C), n.addChild(V_1);
                    var Y = new Pt;
                    ze(Y, r, 196, 156), V_1.addChild(Y);
                    var $_1 = { x: 10, y: 10, w: 132, h: 24 };
                    {
                        var F = new Pt;
                        F.rect($_1.x, $_1.y, $_1.w, $_1.h), F.fill({ color: 0, alpha: .03 }), F.rect($_1.x + .5, $_1.y + .5, Math.max(0, $_1.w - 1), Math.max(0, $_1.h - 1)), F.stroke({ width: 1, color: r.control.border, alignment: 0 }), V_1.addChild(F);
                        var ut = Zt({ text: "Year 20".concat(qt(P.year2)), fontFamily: r.fontFamily, fontSize: r.fontSize, fill: r.text, wordWrap: !1 });
                        ut.position.set($_1.x + 8, $_1.y + 4 + Mt), V_1.addChild(ut);
                    }
                    var tt_1 = 10, W_1 = 44;
                    for (var F = 0; F < 12; F++) {
                        var ut = F + 1, dt = tt_1 + F % 4 * 44, lt = W_1 + Math.floor(F / 4) * 34;
                        if (ut === P.month) {
                            var g = new Pt;
                            g.rect(dt + 1, lt + 1, 42, 32), g.fill({ color: 0, alpha: .06 }), V_1.addChild(g);
                        }
                        var gt = Zt({ text: String(ut), fontFamily: r.fontFamily, fontSize: r.fontSize, fill: r.text, wordWrap: !1 });
                        gt.position.set(dt + 14, lt + 8 + Mt), V_1.addChild(gt), Y.rect(dt, lt, 44, 34), Y.stroke({ width: 1, color: r.control.border, alignment: 0 });
                    }
                    V_1.eventMode = "static", V_1.cursor = "pointer", V_1.hitArea = new Rt(0, 0, 196, 156), V_1.on("pointerdown", function (F) { var J, Q, z, at, Z; if ((F == null ? void 0 : F.button) === 2)
                        return; var ut = u(F); if (ut <= 0)
                        return; w.focusedKeyByPointer.set(ut, k.inputKey), w.keyboardOwnerPointerId = ut; var dt = V_1.toLocal(F.global), lt = (J = dt == null ? void 0 : dt.x) != null ? J : -1, gt = (Q = dt == null ? void 0 : dt.y) != null ? Q : -1; if (lt >= $_1.x && lt <= $_1.x + $_1.w && gt >= $_1.y && gt <= $_1.y + $_1.h) {
                        P.openYear = !0, s.set(k.inputKey, P), (z = p != null ? p : _) == null || z(), (at = F.stopPropagation) == null || at.call(F);
                        return;
                    } var x = lt - tt_1, L = gt - W_1; if (x < 0 || L < 0)
                        return; var I = Math.floor(x / 44), E = Math.floor(L / 34); if (I < 0 || I >= 4 || E < 0 || E >= 3)
                        return; var M = E * 4 + I + 1; if (M < 1 || M > 12)
                        return; P.month = M, P.openPanel = null, P.openYear = !1, P.openMonthGrid = !1, s.set(k.inputKey, P); var N = c(k.inputKey, { type: "text" }); N.value = mn(P), _ == null || _(), (Z = F.stopPropagation) == null || Z.call(F); }), V_1.on("pointerdown", function (F) { var ut; (ut = F.stopPropagation) == null || ut.call(F); }), P.openYear && S.push({ kind: "year-panel", inputKey: k.inputKey, absX: et, absY: C, anchorW: 196, anchorH: 0 });
                }
                if (k.kind === "week-panel") {
                    var v = k.absX, H = k.absY + k.anchorH;
                    v = Math.max(0, Math.min(v, Math.max(0, i - 280))), H + 192 > o - 4 && (H = k.absY - 192), H = Math.max(0, Math.min(H, Math.max(0, o - 192)));
                    var q_1 = new Ot;
                    q_1.position.set(v, H), n.addChild(q_1);
                    var rt = new Pt;
                    ze(rt, r, 280, 192), q_1.addChild(rt);
                    var ot_1 = { x: 10, y: 10, w: 104, h: 24 }, K_1 = { x: 10 + ot_1.w + 10, y: 10, w: 120, h: 24 }, et = function (Y, $) { var tt = new Pt; tt.rect(Y.x, Y.y, Y.w, Y.h), tt.fill({ color: 0, alpha: .03 }), tt.rect(Y.x + .5, Y.y + .5, Math.max(0, Y.w - 1), Math.max(0, Y.h - 1)), tt.stroke({ width: 1, color: r.control.border, alignment: 0 }), q_1.addChild(tt); var W = Zt({ text: $, fontFamily: r.fontFamily, fontSize: r.fontSize, fill: r.text, wordWrap: !1 }); W.position.set(Y.x + 8, Y.y + 4 + Mt), q_1.addChild(W); };
                    et(ot_1, "Month ".concat(P.month)), et(K_1, "Year 20".concat(qt(P.year2)));
                    var C = 44, V_2 = vo({ panel: q_1, theme: r, x: 10, y: C, w: 280 - 10 * 2, st: P, onPick: function () { } }).hitRects;
                    q_1.eventMode = "static", q_1.cursor = "pointer", q_1.hitArea = new Rt(0, 0, 280, 192), q_1.on("pointerdown", function (Y) {
                        var e_23, _a;
                        var dt, lt, gt, g, x, L, I;
                        if ((Y == null ? void 0 : Y.button) === 2)
                            return;
                        var $ = u(Y);
                        if ($ <= 0)
                            return;
                        w.focusedKeyByPointer.set($, k.inputKey), w.keyboardOwnerPointerId = $;
                        var tt = q_1.toLocal(Y.global), W = (dt = tt == null ? void 0 : tt.x) != null ? dt : -1, F = (lt = tt == null ? void 0 : tt.y) != null ? lt : -1, ut = function (E) { return W >= E.x && W <= E.x + E.w && F >= E.y && F <= E.y + E.h; };
                        if (ut(ot_1)) {
                            P.openMonthGrid = !P.openMonthGrid, s.set(k.inputKey, P), (gt = p != null ? p : _) == null || gt(), (g = Y.stopPropagation) == null || g.call(Y);
                            return;
                        }
                        if (ut(K_1)) {
                            P.openYear = !0, s.set(k.inputKey, P), (x = p != null ? p : _) == null || x(), (L = Y.stopPropagation) == null || L.call(Y);
                            return;
                        }
                        try {
                            for (var V_3 = (e_23 = void 0, __values(V_2)), V_3_1 = V_3.next(); !V_3_1.done; V_3_1 = V_3.next()) {
                                var E = V_3_1.value;
                                if (ut(E)) {
                                    P.weekIndex = E.weekIndex;
                                    var R = c(k.inputKey, { type: "text" });
                                    P.kind === "week" ? R.value = fn(P) : P.kind === "date" ? R.value = De(P) : R.value = Ke(P), P.openPanel = null, P.openYear = !1, P.openMonthGrid = !1, s.set(k.inputKey, P), _ == null || _(), (I = Y.stopPropagation) == null || I.call(Y);
                                    return;
                                }
                            }
                        }
                        catch (e_23_1) { e_23 = { error: e_23_1 }; }
                        finally {
                            try {
                                if (V_3_1 && !V_3_1.done && (_a = V_3.return)) _a.call(V_3);
                            }
                            finally { if (e_23) throw e_23.error; }
                        }
                    }), P.openMonthGrid && S.push({ kind: "month-grid", inputKey: k.inputKey, absX: v, absY: H + ot_1.y + ot_1.h + 4, anchorW: 0, anchorH: 0 }), P.openYear && S.push({ kind: "year-panel", inputKey: k.inputKey, absX: v + K_1.x, absY: H + K_1.y, anchorW: K_1.w, anchorH: 0 });
                }
                if (k.kind === "time-panel") {
                    var v_1 = k.absX, H_1 = k.absY + k.anchorH;
                    v_1 = Math.max(0, Math.min(v_1, Math.max(0, i - 330))), H_1 + 80 > o - 4 && (H_1 = k.absY - 80), H_1 = Math.max(0, Math.min(H_1, Math.max(0, o - 80)));
                    var q_2 = new Ot;
                    q_2.position.set(v_1, H_1), n.addChild(q_2);
                    var rt = new Pt;
                    ze(rt, r, 330, 80), q_2.addChild(rt);
                    var ot = Zt({ text: "Time", fontFamily: r.fontFamily, fontSize: r.fontSize, fill: r.text, wordWrap: !1 });
                    ot.position.set(10, 8 + Mt), q_2.addChild(ot);
                    var K_2 = function (E) { return Array.from({ length: E }, function (R, M) { return qt(M); }).join("\n"); }, et = k.inputKey, C = "".concat(et, ":time-h"), V = "".concat(et, ":time-m"), Y = "".concat(et, ":time-s"), $ = Ee(d, C, Wt(P.hour, 0, 23)), tt = Ee(d, V, Wt(P.minute, 0, 59)), W = Ee(d, Y, Wt(P.second, 0, 59));
                    $.selectedIndex = Wt(P.hour, 0, 23), tt.selectedIndex = Wt(P.minute, 0, 59), W.selectedIndex = Wt(P.second, 0, 59);
                    var F_2 = 96, ut_1 = 36, dt_1 = 32, lt = 8, gt = function (E, R, M) { var N = new Ot; N.position.set(R, dt_1), q_2.addChild(N); var J = new Pt; N.addChild(J), hn({ node: { key: E, attrs: { "data-options": K_2(M), "data-selected-index": String(Ee(d, E, 0).selectedIndex) } }, container: N, graphics: J, w: F_2, h: ut_1, absX: v_1 + R, absY: H_1 + dt_1, theme: r, selectStates: d, uiState: w, getPointerId: u, getCursorColor: y, requestPaint: _, requestOverlayPaint: p, popupSink: b }); };
                    gt(C, 10, 24), gt(V, 10 + F_2 + lt, 60), gt(Y, 10 + (F_2 + lt) * 2, 60);
                    var g = Wt((D = (O = d.get(C)) == null ? void 0 : O.selectedIndex) != null ? D : P.hour, 0, 23), x = Wt((T = (U = d.get(V)) == null ? void 0 : U.selectedIndex) != null ? T : P.minute, 0, 59), L = Wt((G = (A = d.get(Y)) == null ? void 0 : A.selectedIndex) != null ? G : P.second, 0, 59);
                    P.hour = g, P.minute = x, P.second = L, s.set(k.inputKey, P);
                    var I = c(k.inputKey, { type: "text" });
                    P.kind === "time" ? I.value = je(P) : I.value = Ke(P), q_2.eventMode = "static", q_2.hitArea = new Rt(0, 0, 330, 80), q_2.on("pointerdown", function (E) { var R; (R = E.stopPropagation) == null || R.call(E); });
                }
            }
        };
        try {
            for (var e_22 = __values(e), e_22_1 = e_22.next(); !e_22_1.done; e_22_1 = e_22.next()) {
                var k = e_22_1.value;
                _loop_1(k);
            }
        }
        catch (e_20_1) { e_20 = { error: e_20_1 }; }
        finally {
            try {
                if (e_22_1 && !e_22_1.done && (_a = e_22.return)) _a.call(e_22);
            }
            finally { if (e_20) throw e_20.error; }
        }
        var _loop_2 = function (k) {
            var P = s.get(k.inputKey);
            P && (k.kind === "month-grid" && Lo({ stage: n, theme: r, popup: k, st: P, viewportW: i, viewportH: o, getPointerId: u, requestPaint: _, onPick: function (X) { P.month = X, P.openMonthGrid = !1, s.set(k.inputKey, P); var j = c(k.inputKey, { type: "text" }); P.kind === "month" ? j.value = mn(P) : P.kind === "week" ? j.value = fn(P) : P.kind === "date" ? j.value = De(P) : j.value = Ke(P); } }), k.kind === "year-panel" && Do({ stage: n, theme: r, popup: k, st: P, viewportW: i, viewportH: o, sliders: a, sliderBounds: h, sliderDrags: m, getPointerId: u, requestPaint: _, onChange: function (X) { P.year2 = X, s.set(k.inputKey, P); var j = c(k.inputKey, { type: "text" }); P.kind === "month" ? j.value = mn(P) : P.kind === "week" ? j.value = fn(P) : P.kind === "date" ? j.value = De(P) : P.kind === "time" ? j.value = je(P) : j.value = Ke(P); } }));
        };
        try {
            for (var S_1 = __values(S), S_1_1 = S_1.next(); !S_1_1.done; S_1_1 = S_1.next()) {
                var k = S_1_1.value;
                _loop_2(k);
            }
        }
        catch (e_21_1) { e_21 = { error: e_21_1 }; }
        finally {
            try {
                if (S_1_1 && !S_1_1.done && (_b = S_1.return)) _b.call(S_1);
            }
            finally { if (e_21) throw e_21.error; }
        }
    }
    function gi(t) {
        var e_24, _a;
        var e = !1;
        try {
            for (var _b = __values(t.values()), _c = _b.next(); !_c.done; _c = _b.next()) {
                var n = _c.value;
                (n.openPanel != null || n.openYear || n.openMonthGrid) && (n.openPanel = null, n.openYear = !1, n.openMonthGrid = !1, e = !0);
            }
        }
        catch (e_24_1) { e_24 = { error: e_24_1 }; }
        finally {
            try {
                if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
            }
            finally { if (e_24) throw e_24.error; }
        }
        return e;
    }
    var bi = 5e4, Ve = new WeakMap, yi = new Map, Go = 1, xi = 0, Ho = 0, _i = !1, ke = [], $n = null;
    function He(t) { return t instanceof Pt ? "Graphics" : t instanceof ie ? "Text" : t instanceof Ot ? "Container" : "Object"; }
    function Wo(t) { var e = t && typeof t == "object" ? t.label : void 0, n = t && typeof t == "object" ? He(t) : "Object"; return e ? "".concat(n, ":").concat(String(e).slice(0, 80)) : n; }
    function pe(t) { var e = Ve.get(t); return e || (e = Go++, Ve.set(t, e)), yi.set(e, t), e; }
    function gn(t) { var e, n, r, i, o, s; if (t == null || typeof t == "number" || typeof t == "string" || typeof t == "boolean")
        return t; if (Array.isArray(t))
        return t.slice(0, 16).map(gn); if (typeof t == "object") {
        var c = t;
        return "color" in c || "alpha" in c || "width" in c && !("x" in c) && !("y" in c) && !("height" in c) ? { color: c.color, alpha: c.alpha, width: c.width } : "x" in c || "y" in c || "width" in c || "height" in c ? { x: Number((e = c.x) != null ? e : 0), y: Number((n = c.y) != null ? n : 0), w: Number((i = (r = c.width) != null ? r : c.w) != null ? i : 0), h: Number((s = (o = c.height) != null ? o : c.h) != null ? s : 0) } : He(c);
    } return String(t); }
    function Ge(t, e, n) {
        var e_25, _a;
        if (e === void 0) { e = 0; }
        if (n === void 0) { n = new WeakSet; }
        if (t == null || typeof t == "number" || typeof t == "string" || typeof t == "boolean")
            return t;
        if (typeof t == "bigint")
            return Number.isSafeInteger(Number(t)) ? Number(t) : String(t);
        if (typeof t == "symbol")
            return String(t);
        if (typeof t == "function")
            return { type: "Function", name: t.name || void 0, arity: t.length };
        if (typeof t != "object")
            return String(t);
        if (n.has(t))
            return "[Circular]";
        if (e > 12)
            return He(t);
        if (n.add(t), Array.isArray(t))
            return t.slice(0, 256).map(function (i) { return Ge(i, e + 1, n); });
        var r = {};
        try {
            for (var _b = __values(Object.entries(t).slice(0, 128)), _c = _b.next(); !_c.done; _c = _b.next()) {
                var _d = __read(_c.value, 2), i = _d[0], o = _d[1];
                r[i] = Ge(o, e + 1, n);
            }
        }
        catch (e_25_1) { e_25 = { error: e_25_1 }; }
        finally {
            try {
                if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
            }
            finally { if (e_25) throw e_25.error; }
        }
        return r;
    }
    function Un(t) { if (t != null)
        return typeof t == "symbol" ? t.toString() : String(t); }
    function wi(t) { if (t != null)
        return typeof t == "function" ? { type: "function", name: t.name || void 0, arity: t.length } : typeof t == "object" ? { id: pe(t), type: He(t) } : { type: typeof t }; }
    function Fo(t) { if (t != null)
        return typeof t == "object" ? { id: pe(t), type: He(t) } : typeof t == "function" ? { type: "function" } : { type: typeof t }; }
    function $o(t) { var e = { event: Un(t[0]), listener: wi(t[1]) }; return t.length > 2 && (e.context = Fo(t[2])), [e]; }
    function Bo(t) { return String(t != null ? t : "").slice(0, 240); }
    function Uo(t) {
        var e_26, _a;
        var r, i;
        if (!t || typeof t != "object")
            return gn(t);
        var e = t, n = { type: (i = (r = t.constructor) == null ? void 0 : r.name) != null ? i : "object" };
        try {
            for (var _b = __values(["fontFamily", "fontSize", "fontStyle", "fontWeight", "fill", "align", "lineHeight", "letterSpacing", "wordWrap", "wordWrapWidth", "padding"]), _c = _b.next(); !_c.done; _c = _b.next()) {
                var o = _c.value;
                var s = e[o];
                s !== void 0 && (n[o] = gn(s));
            }
        }
        catch (e_26_1) { e_26 = { error: e_26_1 }; }
        finally {
            try {
                if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
            }
            finally { if (e_26) throw e_26.error; }
        }
        return n;
    }
    function Xo(t) { var s, c, a, h, m, d; if (!t || typeof t != "object")
        return; var e = t, n = Number((s = e.x) != null ? s : 0), r = Number((c = e.y) != null ? c : 0), i = Number((h = (a = e.width) != null ? a : e.w) != null ? h : 0), o = Number((d = (m = e.height) != null ? m : e.h) != null ? d : 0); if (!(!Number.isFinite(n) || !Number.isFinite(r) || !Number.isFinite(i) || !Number.isFinite(o)) && !(i <= 0 || o <= 0))
        return { x: n, y: r, w: i, h: o }; }
    function Yo(t, e) { if (e) {
        if (t === "addChild" || t === "removeChild")
            return e.map(function (n) { return n && typeof n == "object" ? pe(n) : 0; });
        if (t === "mask") {
            var n = e[0];
            return [n && typeof n == "object" ? pe(n) : 0];
        }
        if (t === "addChildAt" || t === "setChildIndex") {
            var n = e[0];
            return [n && typeof n == "object" ? pe(n) : 0, Number(e[1]) || 0];
        }
        return t === "on" ? $o(e) : t === "snapshot" ? e : t === "text.text.set" ? e.length ? [Bo(e[0])] : [] : t === "text.style.set" ? e.length ? [Uo(e[0])] : [] : e.map(gn);
    } }
    function bn(t, e, n) { var r, i; try {
        window.__TRUEOS_PIXI_CAPTURE_STEP__ = "record:".concat(e, ":begin");
        var o = window.__pixiCapture;
        if (!(o != null && o.enabled))
            return;
        o.counts[e] = ((r = o.counts[e]) != null ? r : 0) + 1;
        var s = { frame: xi, seq: ++Ho, op: e, id: t && typeof t == "object" ? pe(t) : void 0, target: Wo(t), event: e === "on" && (n != null && n.length) ? Un(n[0]) : void 0, listener: e === "on" && (n != null && n.length) ? wi(n[1]) : void 0, args: Yo(e, n) };
        window.__TRUEOS_PIXI_CAPTURE_STEP__ = "record:".concat(e, ":push"), o.commands.push(s), o.persist && Ko(s), o.commands.length > bi && o.commands.splice(0, o.commands.length - bi), window.__TRUEOS_PIXI_CAPTURE_STEP__ = "record:".concat(e, ":done");
    }
    catch (o) {
        try {
            window.__TRUEOS_PIXI_CAPTURE_ERROR__ = "record:".concat(e, ":").concat(String((i = o == null ? void 0 : o.message) != null ? i : o));
        }
        catch (s) { }
    } }
    function Ko(t) { if (ke.push(t), t.op === "snapshot") {
        Je();
        return;
    } if (ke.length >= 512) {
        Je();
        return;
    } $n == null && ($n = window.setTimeout(function () { $n = null, Je(); }, 50)); }
    function Je() {
        if (ke.length === 0)
            return;
        var t = ke;
        ke = [];
        var e = t.map(function (n) { return JSON.stringify(n); }).join("\n") + "\n";
        navigator.sendBeacon && navigator.sendBeacon("/__pixi_capture", new Blob([e], { type: "application/x-ndjson" })) || fetch("/__pixi_capture", { method: "POST", headers: { "Content-Type": "application/x-ndjson" }, body: e, keepalive: !0 }).catch(function () { ke = t.concat(ke); });
    }
    function zo(t, e, n) {
        var e_27, _a, e_28, _b, e_29, _c;
        var r, i;
        if (e === "on") {
            var o = Un(n[0]), s = n[1];
            return !o || typeof s != "function" || ((!t.listeners || typeof t.listeners != "object") && (t.listeners = {}), Array.isArray(t.listeners[o]) || (t.listeners[o] = []), t.listeners[o].push(s)), t;
        }
        if (e === "addChild") {
            var o = n;
            Array.isArray(t.children) || (t.children = []);
            try {
                for (var o_1 = __values(o), o_1_1 = o_1.next(); !o_1_1.done; o_1_1 = o_1.next()) {
                    var s = o_1_1.value;
                    if (s) {
                        if (s.parent && Array.isArray(s.parent.children)) {
                            var c = s.parent.children.indexOf(s);
                            c >= 0 && s.parent.children.splice(c, 1);
                        }
                        s.parent = t, t.children.push(s);
                    }
                }
            }
            catch (e_27_1) { e_27 = { error: e_27_1 }; }
            finally {
                try {
                    if (o_1_1 && !o_1_1.done && (_a = o_1.return)) _a.call(o_1);
                }
                finally { if (e_27) throw e_27.error; }
            }
            return o[0];
        }
        if (e === "addChildAt") {
            var o = n[0];
            if (!o)
                return o;
            if (Array.isArray(t.children) || (t.children = []), o.parent && Array.isArray(o.parent.children)) {
                var c = o.parent.children.indexOf(o);
                c >= 0 && o.parent.children.splice(c, 1);
            }
            o.parent = t;
            var s = Math.max(0, Math.min(Number(n[1]) | 0, t.children.length));
            return t.children.splice(s, 0, o), o;
        }
        if (e === "removeChild") {
            var o = n;
            Array.isArray(t.children) || (t.children = []);
            try {
                for (var o_2 = __values(o), o_2_1 = o_2.next(); !o_2_1.done; o_2_1 = o_2.next()) {
                    var s = o_2_1.value;
                    var c = t.children.indexOf(s);
                    c >= 0 && t.children.splice(c, 1), s && (s.parent = null);
                }
            }
            catch (e_28_1) { e_28 = { error: e_28_1 }; }
            finally {
                try {
                    if (o_2_1 && !o_2_1.done && (_b = o_2.return)) _b.call(o_2);
                }
                finally { if (e_28) throw e_28.error; }
            }
            return o[0];
        }
        if (e === "removeChildren") {
            Array.isArray(t.children) || (t.children = []);
            var o = Math.max(0, Number((r = n[0]) != null ? r : 0) | 0), s = Array.isArray(t.children) ? t.children.length : o, c = Math.max(o, Math.min(Number((i = n[1]) != null ? i : s) | 0, s)), a = t.children.splice(o, c - o);
            try {
                for (var a_1 = __values(a), a_1_1 = a_1.next(); !a_1_1.done; a_1_1 = a_1.next()) {
                    var h = a_1_1.value;
                    h.parent = null;
                }
            }
            catch (e_29_1) { e_29 = { error: e_29_1 }; }
            finally {
                try {
                    if (a_1_1 && !a_1_1.done && (_c = a_1.return)) _c.call(a_1);
                }
                finally { if (e_29) throw e_29.error; }
            }
            return a;
        }
        if (e === "setChildIndex") {
            var o = n[0];
            Array.isArray(t.children) || (t.children = []);
            var s = t.children.indexOf(o);
            if (s < 0)
                return;
            t.children.splice(s, 1);
            var c = Math.max(0, Math.min(Number(n[1]) | 0, t.children.length));
            t.children.splice(c, 0, o);
            return;
        }
        if (e === "removeAllListeners")
            return (!t.listeners || typeof t.listeners != "object") && (t.listeners = {}), n[0] == null ? t.listeners = {} : delete t.listeners[String(n[0])], t;
        if (e === "clear")
            return Array.isArray(t.commands) || (t.commands = []), t.commands.length = 0, t;
        if (e === "rect" || e === "roundRect" || e === "circle" || e === "ellipse" || e === "moveTo" || e === "lineTo" || e === "closePath" || e === "poly" || e === "fill" || e === "stroke" || e === "image" || e === "svg")
            return Array.isArray(t.commands) || (t.commands = []), t.commands.push(__spreadArray([e], __read(n), false)), t;
        if (e === "text.setSize")
            return t;
    }
    function jo() { var t = function () { return !!(window.__TRUEOS_PIXI_REPAINT_REQUIRED__ || window.__TRUEOS_PIXI_SCROLL_REPAINT_REQUIRED__ || window.__TRUEOS_PIXI_OVERLAY_REPAINT_REQUIRED__); }; window.__TRUEOS_DISPATCH_PIXI_POINTER__ = function (e, n, r, i, o, s, c) {
        var e_30, _a;
        if (c === void 0) { c = 0; }
        var D, U, T, A, G, k, P, X, j, f, v, H, q, rt, ot, K, et, C, V, Y, $;
        var a = function (tt) { try {
            window.__TRUEOS_PIXI_POINTER_DISPATCH_STEP__ = tt, typeof console != "undefined" && typeof console.log == "function" && console.log("[trueos pointer dispatch] ".concat(tt));
        }
        catch (W) { } };
        a("start node=".concat(Number(e) || 0, " event=").concat(String(n || "")));
        var h = window.__TRUEOS_PIXI_APP;
        if (String(n || "") === "wheel") {
            var tt = h == null ? void 0 : h.canvas;
            if (!tt || typeof tt.dispatchEvent != "function")
                return a("wheel-canvas-missing"), { handled: 0, listenerCount: 0, painted: 0, targetFound: 0 };
            var W = (T = (U = (D = window.__pixiCapture) == null ? void 0 : D.commands) == null ? void 0 : U.length) != null ? T : 0, F = { type: "wheel", deltaX: 0, deltaY: Number(c) || 0, deltaMode: 0, offsetX: Number(r) || 0, offsetY: Number(i) || 0, clientX: Number(r) || 0, clientY: Number(i) || 0, pointerId: Number(o) || 1, buttons: Number(s) || 0, defaultPrevented: !1, propagationStopped: !1, preventDefault: function () { this.defaultPrevented = !0; }, stopPropagation: function () { this.propagationStopped = !0; } };
            a("wheel-dispatch deltaY=".concat(F.deltaY)), tt.dispatchEvent(F);
            var ut = 0;
            if (window.__TRUEOS_CAPTURE_ONLY__) {
                var L = window.__TRUEOS_REPAINT_NOW__;
                t() && typeof L == "function" && (a("wheel-repaint-call"), L(), a("wheel-repaint-return"), ut = 1);
            }
            else
                (A = h == null ? void 0 : h.renderer) != null && A.render && (h != null && h.stage) && (h.renderer.render(h.stage), ut = 1);
            var dt = (P = (k = (G = window.__pixiCapture) == null ? void 0 : G.commands) == null ? void 0 : k.length) != null ? P : W, lt = (X = tt.listeners) == null ? void 0 : X.wheel, gt = Array.isArray(lt) ? lt.length : typeof lt == "function" ? 1 : 0, g = F.defaultPrevented || gt > 0 ? 1 : 0;
            a("wheel-done handled=".concat(g, " listeners=").concat(gt, " painted=").concat(ut));
            var x = window.__TRUEOS_PIXI_LAST_SCROLL_FAST_PATH__;
            return (x == null ? void 0 : x.owner) === "root" || (x == null ? void 0 : x.owner) === "iframe" ? { handled: g, listenerCount: gt, painted: 1, targetFound: 1, scrollFastPath: 1, rootNode: Number(x.rootNode) || 0, contentNode: Number(x.contentNode) || 0, contentY: Number(x.contentY) || 0, scrollbarNode: Number(x.scrollbarNode) || 0, scrollbarVisible: Number(x.scrollbarVisible) || 0, trackX: Number(x.trackX) || 0, trackY: Number(x.trackY) || 0, trackW: Number(x.trackW) || 0, trackH: Number(x.trackH) || 0, thumbX: Number(x.thumbX) || 0, thumbY: Number(x.thumbY) || 0, thumbW: Number(x.thumbW) || 0, thumbH: Number(x.thumbH) || 0 } : { handled: g, listenerCount: gt, painted: dt > W || ut ? 1 : 0, targetFound: 1 };
        }
        var m = yi.get(Number(e) || 0), d = 0, b = 0, y = 0;
        if (!m)
            return a("target-missing"), { handled: d, listenerCount: b, painted: y, targetFound: 0 };
        window.__TRUEOS_PIXI_LAST_GRAPHICS_FAST_PATH__ = null, window.__TRUEOS_PIXI_LAST_OVERLAY_FAST_PATH__ = null;
        var w = { type: String(n || ""), button: Number(s) & 2 ? 2 : 0, buttons: Number(s) || 0, pointerId: Number(o) || 1, pointerType: "mouse", global: { x: Number(r) || 0, y: Number(i) || 0 }, data: { pointerId: Number(o) || 1, pointerType: "mouse", global: { x: Number(r) || 0, y: Number(i) || 0 } }, target: m, currentTarget: m, defaultPrevented: !1, propagationStopped: !1, preventDefault: function () { this.defaultPrevented = !0; }, stopPropagation: function () { this.propagationStopped = !0; } }, u = (v = (f = (j = window.__pixiCapture) == null ? void 0 : j.commands) == null ? void 0 : f.length) != null ? v : 0;
        a("target-found label=".concat(String((H = m.label) != null ? H : "")));
        for (var tt = m; tt; tt = tt.parent) {
            w.currentTarget = tt;
            var W = (q = tt.listeners) == null ? void 0 : q[w.type];
            if (!(!Array.isArray(W) || W.length === 0)) {
                b += W.length, a("listeners node=".concat((rt = Ve.get(tt)) != null ? rt : 0, " count=").concat(W.length));
                try {
                    for (var _b = (e_30 = void 0, __values(W.slice())), _c = _b.next(); !_c.done; _c = _b.next()) {
                        var F = _c.value;
                        if (typeof F == "function" && (d = 1, a("listener-call node=".concat((ot = Ve.get(tt)) != null ? ot : 0)), F.call(tt, w), a("listener-return node=".concat((K = Ve.get(tt)) != null ? K : 0)), w.propagationStopped))
                            break;
                    }
                }
                catch (e_30_1) { e_30 = { error: e_30_1 }; }
                finally {
                    try {
                        if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
                    }
                    finally { if (e_30) throw e_30.error; }
                }
                if (w.propagationStopped)
                    break;
            }
        }
        if (window.__TRUEOS_CAPTURE_ONLY__) {
            var tt = window.__TRUEOS_REPAINT_NOW__;
            t() && typeof tt == "function" && (a("capture-repaint-call"), tt(), a("capture-repaint-return"), y = 1);
        }
        else
            (et = h == null ? void 0 : h.renderer) != null && et.render && (h != null && h.stage) && (a("paint-call"), h.renderer.render(h.stage), a("paint-return"), y = 1);
        var _ = (Y = (V = (C = window.__pixiCapture) == null ? void 0 : C.commands) == null ? void 0 : V.length) != null ? Y : u, p = window.__TRUEOS_PIXI_LAST_SCROLL_FAST_PATH__;
        if ((p == null ? void 0 : p.owner) === "root" || (p == null ? void 0 : p.owner) === "iframe")
            return a("scroll-fast owner=".concat(p.owner)), { handled: d, listenerCount: b, painted: 1, targetFound: 1, scrollFastPath: 1, rootNode: Number(p.rootNode) || 0, contentNode: Number(p.contentNode) || 0, contentY: Number(p.contentY) || 0, scrollbarNode: Number(p.scrollbarNode) || 0, scrollbarVisible: Number(p.scrollbarVisible) || 0, trackX: Number(p.trackX) || 0, trackY: Number(p.trackY) || 0, trackW: Number(p.trackW) || 0, trackH: Number(p.trackH) || 0, thumbX: Number(p.thumbX) || 0, thumbY: Number(p.thumbY) || 0, thumbW: Number(p.thumbW) || 0, thumbH: Number(p.thumbH) || 0 };
        var S = window.__TRUEOS_PIXI_LAST_GRAPHICS_FAST_PATH__;
        if ((S == null ? void 0 : S.owner) === "context-menu-hover" && (w.type === "pointerover" || w.type === "pointerout") && _ > u)
            return ($ = window.__pixiCapture) != null && $.commands && window.__pixiCapture.commands.splice(u, _ - u), a("graphics-fast owner=".concat(S.owner)), { handled: d, listenerCount: b, painted: 1, targetFound: 1, graphicsFastPath: 1, rootNode: Number(S.rootNode) || 0, graphicsNode: Number(S.graphicsNode) || 0, rectX: Number(S.x) || 0, rectY: Number(S.y) || 0, rectW: Number(S.w) || 0, rectH: Number(S.h) || 0, damageX: Number(S.worldX) + Number(S.x) || 0, damageY: Number(S.worldY) + Number(S.y) || 0, damageW: Number(S.w) || 0, damageH: Number(S.h) || 0, fillColor: Number(S.fillColor) || 0, fillAlpha: Number(S.fillAlpha) || 0 };
        var O = window.__TRUEOS_PIXI_LAST_OVERLAY_FAST_PATH__;
        return O && Number(O.rootNode) > 0 && Number(O.damageW) > 0 && Number(O.damageH) > 0 ? (a("overlay-fast"), { handled: d, listenerCount: b, painted: 1, targetFound: 1, overlayFastPath: 1, rootNode: Number(O.rootNode) || 0, damageX: Number(O.damageX) || 0, damageY: Number(O.damageY) || 0, damageW: Number(O.damageW) || 0, damageH: Number(O.damageH) || 0 }) : (y = _ > u || y ? 1 : 0, a("done handled=".concat(d, " listeners=").concat(b, " painted=").concat(y)), { handled: d, listenerCount: b, painted: y, targetFound: 1 });
    }; }
    function Bn(t, e, n) {
        if (n === void 0) { n = e; }
        var r = t == null ? void 0 : t[e];
        if (typeof r != "function" || r.__pixiCapturePatched)
            return;
        var i = function () {
            var s = [];
            for (var _a = 0; _a < arguments.length; _a++) {
                s[_a] = arguments[_a];
            }
            var c;
            if (bn(this, n, s), !window.__TRUEOS_CAPTURE_ONLY__)
                return r.apply(this, s);
            try {
                window.__TRUEOS_PIXI_CAPTURE_STEP__ = "invoke:".concat(n, ":begin");
                var a = zo(this, n, s);
                return window.__TRUEOS_PIXI_CAPTURE_STEP__ = "invoke:".concat(n, ":done"), a;
            }
            catch (a) {
                try {
                    window.__TRUEOS_PIXI_CAPTURE_ERROR__ = "invoke:".concat(n, ":").concat(String((c = a == null ? void 0 : a.message) != null ? c : a));
                }
                catch (h) { }
                return n === "addChild" || n === "addChildAt" || n === "removeChild" ? s[0] : this;
            }
        };
        i.__pixiCapturePatched = !0, t[e] = i;
    }
    function Vo(t, e) { var n = t; for (; n;) {
        var r = Object.getOwnPropertyDescriptor(n, e);
        if (r)
            return r;
        n = Object.getPrototypeOf(n);
    } }
    function ve(t, e, n) { var o, s; if (!(t != null && t.constructor) || t.constructor["__pixiCapturePatched_".concat(n)])
        return; var r = Vo(t, e); if ((r == null ? void 0 : r.configurable) === !1 || r && !r.set && !r.writable)
        return; var i = typeof Symbol == "function" ? Symbol("pixiCapture:".concat(n)) : "__pixiCaptureValue_".concat(n); Object.defineProperty(t, e, { configurable: (o = r == null ? void 0 : r.configurable) != null ? o : !0, enumerable: (s = r == null ? void 0 : r.enumerable) != null ? s : !0, get: r != null && r.get ? function () { var a; return (a = r.get) == null ? void 0 : a.call(this); } : function () { var a = this; return Object.prototype.hasOwnProperty.call(a, i) ? a[i] : r && "value" in r ? r.value : void 0; }, set: function (a) { if (bn(this, n, [a]), !window.__TRUEOS_CAPTURE_ONLY__) {
            r != null && r.set ? r.set.call(this, a) : Object.defineProperty(this, i, { configurable: !0, enumerable: !1, writable: !0, value: a });
            return;
        } var h = this; n === "text.text.set" ? h._text = String(a != null ? a : "") : n === "text.style.set" ? h._style = a != null ? a : {} : n === "text.resolution.set" ? h._resolution = Math.max(1, Number(a) || 1) : Object.defineProperty(h, i, { configurable: !0, enumerable: !1, writable: !0, value: a }); } }), t.constructor["__pixiCapturePatched_".concat(n)] = !0; }
    function Ti(t, e) {
        if (e === void 0) { e = 0; }
        var s, c, a, h, m, d, b, y, w;
        if (!t || e > 64)
            return null;
        var n, r;
        try {
            var u = typeof t.getGlobalPosition == "function" ? t.getGlobalPosition() : null;
            u && Number.isFinite(Number(u.x)) && Number.isFinite(Number(u.y)) && (n = Number(u.x), r = Number(u.y));
        }
        catch (u) { }
        var i = { id: pe(t), type: He(t), label: (s = t.label) != null ? s : void 0, x: (h = (a = (c = t.position) == null ? void 0 : c.x) != null ? a : t.x) != null ? h : 0, y: (b = (d = (m = t.position) == null ? void 0 : m.y) != null ? d : t.y) != null ? b : 0, globalX: n, globalY: r, scaleX: Number.isFinite(Number((y = t.scale) == null ? void 0 : y.x)) ? Number(t.scale.x) : 1, scaleY: Number.isFinite(Number((w = t.scale) == null ? void 0 : w.y)) ? Number(t.scale.y) : 1, visible: t.visible, alpha: Number.isFinite(Number(t.alpha)) ? Number(t.alpha) : 1, maskId: t.mask ? pe(t.mask) : 0, zIndex: Number(t.zIndex) || 0, sortableChildren: t.sortableChildren === !0 }, o = Xo(t.hitArea);
        if (o && (i.hitArea = o), t.listeners && typeof t.listeners == "object") {
            var u = Object.keys(t.listeners).filter(function (_) { var S; var p = (S = t.listeners) == null ? void 0 : S[_]; return Array.isArray(p) && p.length > 0; });
            u.length > 0 && (i.listeners = u.slice(0, 16));
        }
        if (t instanceof Pt && Array.isArray(t.commands) && t.commands.length > 0 && (i.commands = t.commands.slice(-256).map(function (u) { return Ge(u, 0); })), typeof t.text == "string" && (i.text = t.text.slice(0, 120), t instanceof ie && t.style && typeof t.style == "object")) {
            var u = {}, _ = t.style;
            typeof _.fontSize != "undefined" && (u.fontSize = Ge(_.fontSize, 0)), typeof _.fontWeight != "undefined" && (u.fontWeight = Ge(_.fontWeight, 0)), typeof _.fill != "undefined" && (u.fill = Ge(_.fill, 0)), Object.keys(u).length > 0 && (i.textStyle = u);
        }
        return Array.isArray(t.children) && t.children.length && (i.children = t.children.map(function (u) { return Ti(u, e + 1); })), i;
    }
    function Ei() {
        var e_31, _a, e_32, _b;
        if (window.__pixiCapture)
            return window.__pixiCapture;
        var t = { enabled: !0, persist: !window.__TRUEOS_CAPTURE_ONLY__, commands: [], counts: Object.create(null), objectId: function (e) { return pe(e); }, clear: function () { this.commands.length = 0, this.counts = Object.create(null); }, dump: function (e) {
                if (e === void 0) { e = 200; }
                return this.commands.slice(-e);
            }, flush: function () { Je(); }, summary: function () { return ae({}, this.counts); } };
        if (window.__pixiCapture = t, jo(), window.addEventListener("beforeunload", function () { return Je(); }), !_i) {
            _i = !0, typeof Pt.prototype.image != "function" && (Pt.prototype.image = function () { return this; });
            try {
                for (var _c = __values(["clear", "rect", "roundRect", "circle", "ellipse", "moveTo", "lineTo", "closePath", "poly", "fill", "stroke", "image", "svg"]), _d = _c.next(); !_d.done; _d = _c.next()) {
                    var e = _d.value;
                    Bn(Pt.prototype, e);
                }
            }
            catch (e_31_1) { e_31 = { error: e_31_1 }; }
            finally {
                try {
                    if (_d && !_d.done && (_a = _c.return)) _a.call(_c);
                }
                finally { if (e_31) throw e_31.error; }
            }
            try {
                for (var _f = __values(["addChild", "addChildAt", "removeChild", "removeChildren", "setChildIndex", "on", "removeAllListeners"]), _g = _f.next(); !_g.done; _g = _f.next()) {
                    var e = _g.value;
                    Bn(Ot.prototype, e);
                }
            }
            catch (e_32_1) { e_32 = { error: e_32_1 }; }
            finally {
                try {
                    if (_g && !_g.done && (_b = _f.return)) _b.call(_f);
                }
                finally { if (e_32) throw e_32.error; }
            }
            ve(ie.prototype, "text", "text.text.set"), ve(ie.prototype, "style", "text.style.set"), ve(ie.prototype, "resolution", "text.resolution.set"), Bn(ie.prototype, "setSize", "text.setSize"), ve(Ot.prototype, "visible", "visible"), ve(Ot.prototype, "alpha", "alpha"), ve(Ot.prototype, "mask", "mask");
        }
        return t;
    }
    function Ii(t) { var e = t.renderer, n = e == null ? void 0 : e.render; if (typeof n != "function" || n.__pixiCapturePatched)
        return; var r = function (o) { var c; var s = o && typeof o == "object" && "container" in o ? o.container : o || t.stage; return xi++, window.__TRUEOS_CAPTURE_ONLY__ && ((c = window.__pixiCapture) == null || c.clear()), bn(s, "render", []), bn(s, "snapshot", [Ti(s)]), window.__TRUEOS_CAPTURE_ONLY__ ? s : n.call(this, o); }; r.__pixiCapturePatched = !0, e.render = r; }
    Ei();
    var pt = null, xn = 6, Oe = 10, Kt = 1, Vt = 3, Qt = 4, We = 512, Di = new Map;
    var l = { focusedKeyByPointer: new Map, keyboardOwnerPointerId: 1, inputs: new Map, sliders: new Map, sliderDrags: new Map, sliderBounds: new Map, dialogs: new Map, dialogDrags: new Map, dialogSelectedBy: new Map, dialogZ: new Map, dialogZCounter: 1, numbers: new Map, numberHolds: new Map, selects: new Map, temporals: new Map, temporalYearOwners: new Map, color: { rgb: { r: 255, g: 0, b: 0 }, a: 255, pick: null, draggingPointerId: null, bounds: null }, cursorColors: new Map, primaryMousePointerId: 1, harness: { enabled: !0, activeUserPointerId: Kt, periodMs: 3e3 }, userCursorPos: new Map, lastMouse: { x: 0, y: 0, has: !1 }, scroll: { y: 0, contentHeight: 0, viewportHeight: 0, draggingPointerId: null, dragOffsetY: 0, track: { x: 0, y: 0, w: Oe, h: 0 }, thumb: { x: 0, y: 0, w: Oe, h: 0 } }, iframeScroll: new Map, iframeScrollRoots: new Map, iframeScrollbarGraphics: new Map, iframeRects: [], hoverRects: [], hoverHandlers: new Map, hoveredKeyByPointer: new Map, hoveredCursorByPointer: new Map, virtualCursor: { enabled: !1, x: 0, y: 0, t: 0, radius: 120, speed: .9 }, textDrags: new Map, fieldBounds: new Map, dialogDragBounds: new Map, detailsOpen: new Map, contextMenus: new Map, clipboards: new Map }, _n = null, zn = 0;
    function Qo(t) { if (!_n) {
        var n = document.createElement("canvas").getContext("2d");
        if (!n)
            throw new Error("2D canvas not available");
        _n = n;
    } return _n.font = "".concat(t.fontSize, "px ").concat(t.fontFamily), function (e) { return (zn += 1, _n.measureText(e).width); }; }
    function Yn(t, e) {
        if (e === void 0) { e = 16; }
        return Object.entries(t).sort(function (n, r) { return r[1] - n[1] || (n[0] < r[0] ? -1 : n[0] > r[0] ? 1 : 0); }).slice(0, e).map(function (_a) {
            var _b = __read(_a, 2), n = _b[0], r = _b[1];
            return "".concat(n, ":").concat(r);
        }).join(",");
    }
    function Xn(t) { var e = (2166136261 ^ t.length) >>> 0, n = function (o, s) { for (var c = o; c < s; c += 1) {
        var a = t.charCodeAt(c);
        e = e + (a & 65535) >>> 0, e = e + (e << 10) >>> 0, e ^= e >>> 6;
    } }, r = t.length, i = 4096; if (r <= i * 3)
        n(0, r);
    else {
        n(0, i);
        var o = Math.max(i, Math.floor((r - i) / 2));
        n(o, Math.min(r, o + i)), n(Math.max(0, r - i), r);
    } return e = e + (e << 3) >>> 0, e ^= e >>> 11, e = e + (e << 15) >>> 0, "0x".concat(e.toString(16).padStart(8, "0")); }
    function vi(t) {
        var e_33, _a;
        var n;
        if (!t)
            return;
        var e = {};
        try {
            for (var _b = __values(Object.keys(t).sort()), _c = _b.next(); !_c.done; _c = _b.next()) {
                var r = _c.value;
                e[r] = typeof t[r] == "string" ? t[r] : String((n = t[r]) != null ? n : "");
            }
        }
        catch (e_33_1) { e_33 = { error: e_33_1 }; }
        finally {
            try {
                if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
            }
            finally { if (e_33) throw e_33.error; }
        }
        return Object.keys(e).length > 0 ? e : void 0;
    }
    function Gi(t) { return t.kind === "text" ? { kind: "text", text: t.text } : { kind: "block", key: t.key, tagName: t.tagName, attrs: vi(t.attrs), children: t.children.map(Gi) }; }
    function Hi(t) { var e, n, r; return t.kind === "text" ? { kind: "text", text: (e = t.text) != null ? e : "", x: t.x, y: t.y, width: t.width, height: t.height, children: [] } : { kind: "block", key: (n = t.key) != null ? n : "", tagName: (r = t.tagName) != null ? r : "", attrs: vi(t.attrs), x: t.x, y: t.y, width: t.width, height: t.height, children: t.children.map(Hi) }; }
    function Zo(t, e, n, r, i) {
        Lt("[trueos pixi widgets] prepixi stage=canonical-render begin");
        var o = e.map(Gi);
        Lt("[trueos pixi widgets] prepixi stage=canonical-render done"), Lt("[trueos pixi widgets] prepixi stage=canonical-layout begin");
        var s = Hi(n);
        Lt("[trueos pixi widgets] prepixi stage=canonical-layout done"), Lt("[trueos pixi widgets] prepixi stage=stringify begin");
        var c = JSON.stringify(o), a = JSON.stringify(s);
        Lt("[trueos pixi widgets] prepixi stage=stringify done render_bytes=".concat(c.length, " layout_bytes=").concat(a.length)), Lt("[trueos pixi widgets] prepixi stage=hash begin");
        var h = Xn(c), m = Xn(a), d = Xn("".concat(c, "\n").concat(a));
        Lt("[trueos pixi widgets] prepixi stage=hash done"), Lt("[trueos pixi widgets] prepixi stage=trace-stringify begin");
        var b = JSON.stringify({ version: 1, source: t, viewport: { width: r, height: i }, renderHash: h, layoutHash: m, hash: d, renderNodes: o, layout: s });
        return Lt("[trueos pixi widgets] prepixi stage=trace-stringify done bytes=".concat(b.length)), window.__TRUEOS_PIXI_PREPIX_TRACE__ = b, window.__TRUEOS_PIXI_PREPIX_HASH__ = d, window.__TRUEOS_PIXI_PREPIX_RENDER_HASH__ = h, window.__TRUEOS_PIXI_PREPIX_LAYOUT_HASH__ = m, Dt() && console.log("[trueos pixi widgets] prepixi source=".concat(t, " hash=").concat(d, " render_hash=").concat(h, " layout_hash=").concat(m, " bytes=").concat(b.length)), { hash: d, renderHash: h, layoutHash: m, bytes: b.length };
    }
    function Fe(t) { var e = typeof t == "string" ? t : ""; return e.indexOf("<truesurfer-") >= 0 && (e = e.replace(/<truesurfer-[A-Za-z0-9._-]+>/g, "")), e; }
    function qo(t, e) { if (e >= t.length)
        return !0; var n = t.charCodeAt(e); return n === 95 || n === 40 || n === 91 || n === 123 || n === 34 || n === 39 || n >= 48 && n <= 57 || n >= 65 && n <= 90; }
    function Wi(t) { var e = t, n = !0; for (; n;) {
        n = !1;
        var r = 0;
        if (e.charCodeAt(0) === 78) {
            for (r = 1; r < e.length;) {
                var i = e.charCodeAt(r);
                if (i !== 117 && i !== 109)
                    break;
                r += 1;
            }
            r === 1 && (r = 0);
        }
        else {
            for (; r < e.length;) {
                var i = e.charCodeAt(r);
                if (i !== 117 && i !== 109)
                    break;
                r += 1;
            }
            r < 2 && (r = 0);
        }
        r >= 2 && qo(e, r) && (e = e.slice(r), n = !0);
    } return e; }
    function ts(t) { var e = Fe(t), n = e.indexOf("__trueos") >= 0 || e.indexOf("tsNu") >= 0 || e.indexOf("tsNum") >= 0; return e.indexOf("__TRUEOS_HOST_READY__") >= 0 && (e = e.replace(/__TRUEOS_HOST_READY__/g, "")), e.indexOf("__trueos") >= 0 && (e = es(e), e = e.replace(/__trueosNumberValue/g, "").replace(/__trueosHostNum/g, "").replace(/__trueosNum/g, "").replace(/__trueosNu/g, "").replace(/__trueos/g, "")), (e.indexOf("tsNu") >= 0 || e.indexOf("tsNum") >= 0) && (e = e.replace(/tsNum/g, "").replace(/tsNutsNutsNutsNu/g, "").replace(/tsNutsNutsNu/g, "").replace(/tsNutsNu/g, "").replace(/tsNu/g, "")), n && (e = Wi(e.trimStart())), e; }
    function es(t) { var e = "__trueosN", n = t, r = 0; for (; r < n.length;) {
        var i = n.indexOf(e, r);
        if (i < 0)
            break;
        var o = i + e.length;
        for (; o < n.length;) {
            var s = n.charCodeAt(o);
            if (s !== 117 && s !== 109)
                break;
            o += 1;
        }
        if (o === i + e.length) {
            r = o;
            continue;
        }
        n = n.slice(0, i) + n.slice(o), r = i;
    } return n; }
    function Fi(t) { return ts(t); }
    function jn(t) { return Wi(Fi(t).trimStart()); }
    function ns(t) { var e = ge(jn(t)); return !(e.length === 0 || e === "true" || e === "false" || e === "N" || e === "Nu" || e === "Num" || e.startsWith("<truesurfer-") || e.startsWith("__trueo")); }
    function $i(t, e) { var r; var n = Fe(e) || "block"; t[n] = ((r = t[n]) != null ? r : 0) + 1; }
    function rs(t) {
        var e_34, _a;
        var e = { nodes: 0, blocks: 0, text: 0, maxDepth: 0, tags: {} }, n = function (r, i) {
            var e_35, _a;
            if (e.nodes += 1, e.maxDepth = Math.max(e.maxDepth, i), r.kind === "text") {
                e.text += 1;
                return;
            }
            e.blocks += 1, $i(e.tags, r.tagName);
            try {
                for (var _b = __values(r.children), _c = _b.next(); !_c.done; _c = _b.next()) {
                    var o = _c.value;
                    n(o, i + 1);
                }
            }
            catch (e_35_1) { e_35 = { error: e_35_1 }; }
            finally {
                try {
                    if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
                }
                finally { if (e_35) throw e_35.error; }
            }
        };
        try {
            for (var t_1 = __values(t), t_1_1 = t_1.next(); !t_1_1.done; t_1_1 = t_1.next()) {
                var r = t_1_1.value;
                n(r, 1);
            }
        }
        catch (e_34_1) { e_34 = { error: e_34_1 }; }
        finally {
            try {
                if (t_1_1 && !t_1_1.done && (_a = t_1.return)) _a.call(t_1);
            }
            finally { if (e_34) throw e_34.error; }
        }
        return e;
    }
    function is(t) { var e = { nodes: 0, blocks: 0, text: 0, maxDepth: 0, tags: {} }, n = function (r, i) {
        var e_36, _a;
        var o;
        e.nodes += 1, e.maxDepth = Math.max(e.maxDepth, i), r.kind === "text" ? e.text += 1 : (e.blocks += 1, $i(e.tags, (o = r.tagName) != null ? o : "block"));
        try {
            for (var _b = __values(r.children), _c = _b.next(); !_c.done; _c = _b.next()) {
                var s = _c.value;
                n(s, i + 1);
            }
        }
        catch (e_36_1) { e_36 = { error: e_36_1 }; }
        finally {
            try {
                if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
            }
            finally { if (e_36) throw e_36.error; }
        }
    }; return n(t, 1), e; }
    function Mn(t, e) {
        if (e === void 0) { e = 64; }
        var n = ge(Fi(t)), r = "";
        for (var i = 0; i < n.length && r.length < e; i += 1) {
            var o = n.charAt(i);
            r += o === "|" || o === '"' || o === "\\" ? "_" : o;
        }
        return r;
    }
    function wn(t, e) {
        if (e === void 0) { e = 120; }
        var n = "";
        for (var r = 0; r < t.length && n.length < e; r += 1) {
            var i = t.charAt(r);
            n += i === "\r" || i === "\n" || i === "	" || i === "|" || i === '"' || i === "\\" ? "_" : i;
        }
        return n;
    }
    function os(t) { if (t.length <= 0 || t.length > 1e6 || t.indexOf("\0") >= 0)
        return !1; var e = t.slice(0, 256).trimStart().toLowerCase(); return e.startsWith("<!doctype") || e.startsWith("<html") || e.startsWith("<body") || e.startsWith("<"); }
    function Pi(t, e) {
        if (e === void 0) { e = 12; }
        var n = [], r = function (i, o, s) { if (n.length >= e)
            return; if (i.kind === "text") {
            n.push("#".concat(n.length, "@").concat(o, ":").concat(s, " chars=").concat(i.text.length, " sample=\"").concat(Mn(i.text), "\""));
            return;
        } var c = Fe(i.tagName || "block") || "block", a = i.key || ""; for (var h = 0; h < i.children.length; h += 1)
            r(i.children[h], c, a); };
        for (var i = 0; i < t.length; i += 1)
            r(t[i], "root", "");
        return n.join("|");
    }
    function ki(t, e) {
        if (e === void 0) { e = 12; }
        var n = [], r = function (i, o, s) { var h; if (n.length >= e)
            return; if (i.kind === "text") {
            var m = (h = i.text) != null ? h : "";
            n.push("#".concat(n.length, "@").concat(o, ":").concat(s, " chars=").concat(m.length, " box=").concat(Math.round(i.x), ",").concat(Math.round(i.y), ",").concat(Math.round(i.width), ",").concat(Math.round(i.height), " sample=\"").concat(Mn(m), "\""));
            return;
        } var c = Fe(i.tagName || "block") || "block", a = i.key || ""; for (var m = 0; m < i.children.length; m += 1)
            r(i.children[m], c, a); };
        return r(t, "root", ""), n.join("|");
    }
    function Ri(t, e) {
        if (e === void 0) { e = 24; }
        var n = [], r = new Set(["label", "input", "timeinput", "dateinput", "monthinput", "weekinput", "datetimelocalinput", "button", "select", "searchrow", "searchbutton"]), i = function (o, s, c, a) {
            var e_37, _a;
            var d;
            if (n.length >= e)
                return;
            var h = c + o.x, m = a + o.y;
            if (o.kind === "block") {
                var b = Fe(o.tagName || "block") || "block";
                if (r.has(b)) {
                    var y = Mn(Sn(o), 36);
                    n.push("#".concat(n.length, "@").concat(s, ">").concat(b, ":").concat((d = o.key) != null ? d : "", " box=").concat(Math.round(h), ",").concat(Math.round(m), ",").concat(Math.round(o.width), ",").concat(Math.round(o.height), " text=\"").concat(y, "\""));
                }
                try {
                    for (var _b = __values(o.children), _c = _b.next(); !_c.done; _c = _b.next()) {
                        var y = _c.value;
                        i(y, b, h, m);
                    }
                }
                catch (e_37_1) { e_37 = { error: e_37_1 }; }
                finally {
                    try {
                        if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
                    }
                    finally { if (e_37) throw e_37.error; }
                }
            }
        };
        return i(t, "root", 0, 0), n.join("|");
    }
    function Tn(t) { return (typeof t == "string" ? t : "").replace(/&quot;/g, '"').replace(/&#34;/g, '"').replace(/&#39;/g, "'").replace(/&apos;/g, "'").replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&amp;/g, "&"); }
    function Kn(t) { return ge(Tn((typeof t == "string" ? t : "").replace(/<[^>]*>/g, " "))); }
    function ss(t) { var e = 0, n = String(t != null ? t : ""); for (; e < n.length && n.charCodeAt(e) <= 32;)
        e += 1; for (n.charAt(e) === "/" && (e += 1); e < n.length && n.charCodeAt(e) <= 32;)
        e += 1; var r = e; for (; e < n.length;) {
        var i = n.charCodeAt(e);
        if (!(i >= 48 && i <= 57 || i >= 65 && i <= 90 || i >= 97 && i <= 122 || i === 45 || i === 58))
            break;
        e += 1;
    } return n.slice(r, e).toLowerCase(); }
    function as(t) { return t === "h1" || t === "h2" || t === "h3" || t === "h4" || t === "h5" || t === "h6" || t === "summary" || t === "p" || t === "button" || t === "label" || t === "legend" || t === "option"; }
    function Bi(t) { var e = typeof t == "string" ? t : "", n = [], r = function (m) { var d = Kn(m); d.length !== 0 && (d.startsWith("<truesurfer-") || d.startsWith("__trueo") || n.push(d)); }, i = [], o = e.toLowerCase(), s = o.indexOf("<body"); if (s >= 0) {
        var m = e.indexOf(">", s);
        s = m >= 0 ? m + 1 : s;
    }
    else
        s = 0; var c = o.indexOf("</body>", s), a = c >= 0 ? c : e.length, h = ""; for (; s < a && n.length < We;) {
        var m = e.charAt(s);
        if (m !== "<") {
            h += m, s += 1;
            continue;
        }
        var d = Tn(h);
        if (d.length > 0) {
            for (var O = i.length - 1; O >= 0; O -= 1)
                if (i[O].wanted) {
                    i[O].text += " ".concat(d);
                    break;
                }
        }
        h = "";
        var b = e.indexOf(">", s + 1);
        if (b < 0)
            break;
        var y = e.slice(s, b + 1), w = e.slice(s + 1, b), u = ss(w);
        if (w.trimStart().charAt(0) === "/") {
            for (var O = i.length - 1; O >= 0; O -= 1) {
                var D = i.pop();
                if (D != null && D.wanted && r(D.text), (D == null ? void 0 : D.tag) === u)
                    break;
            }
            s = b + 1;
            continue;
        }
        if (u === "script" || u === "style" || u === "template") {
            var O = "</".concat(u, ">"), D = o.indexOf(O, b + 1);
            s = D >= 0 ? D + O.length : b + 1;
            continue;
        }
        if (u === "input") {
            var O = Oi(y, "type").toLowerCase();
            (O === "button" || O === "submit" || O === "reset") && r(Oi(y, "value"));
        }
        var p = y.length - 1;
        for (; p >= 0 && y.charCodeAt(p) <= 32;)
            p -= 1;
        p >= 1 && y.charAt(p) === ">" && y.charAt(p - 1) === "/" || u === "input" || u === "br" || u === "hr" || u === "img" || i.push({ tag: u, wanted: as(u), text: "" }), s = b + 1;
    } if (h.length > 0) {
        var m = Tn(h);
        for (var d = i.length - 1; d >= 0; d -= 1)
            if (i[d].wanted) {
                i[d].text += " ".concat(m);
                break;
            }
    } for (; i.length && n.length < We;) {
        var m = i.pop();
        m != null && m.wanted && r(m.text);
    } if (n.length === 0) {
        var m = o.indexOf("<body");
        if (m >= 0) {
            var u = e.indexOf(">", m);
            m = u >= 0 ? u + 1 : m;
        }
        else
            m = 0;
        var d = o.indexOf("</body>", m), b = d >= 0 ? d : e.length, y = !1, w = "";
        for (var u = m; u < b && n.length < We; u += 1) {
            var _ = e.charAt(u);
            if (_ === "<") {
                r(w), w = "", y = !0;
                continue;
            }
            if (_ === ">") {
                y = !1;
                continue;
            }
            y || (w += _);
        }
        r(w);
    } return n; }
    function En(t) { var e = window == null ? void 0 : window[t]; return e !== void 0 ? e : globalThis == null ? void 0 : globalThis[t]; }
    function ls(t, e) {
        if (e === void 0) { e = 8; }
        var n = [];
        for (var r = 0; r < t.length && n.length < e; r += 1)
            n.push("#".concat(r, "=\"").concat(wn(t[r], 48), "\""));
        return n.join("|");
    }
    function Oi(t, e) { var i, o, s; var r = new RegExp("".concat(e, "[ \\t\\r\\n\\f]*=[ \\t\\r\\n\\f]*(\"([^\"]*)\"|'([^']*)'|([^ \\t\\r\\n\\f>]+))"), "i").exec(t); return Tn((s = (o = (i = r == null ? void 0 : r[2]) != null ? i : r == null ? void 0 : r[3]) != null ? o : r == null ? void 0 : r[4]) != null ? s : ""); }
    function qe(t) { var e = []; for (var n = 0; n < t.length; n += 1) {
        var r = t[n];
        typeof r == "string" && In(e, r);
    } return e; }
    function cs(t) { var e = "", n = !1; for (var r = 0; r < t.length; r += 1) {
        var i = t.charCodeAt(r);
        if (i === 32 || i === 9 || i === 10 || i === 13 || i === 12) {
            n = !0;
            continue;
        }
        n && e.length > 0 && (e += " "), e += t.charAt(r), n = !1;
    } return e; }
    function In(t, e) { var n = cs(e); if (n.length !== 0 && !(n.indexOf("<truesurfer-") === 0 || n.indexOf("__trueo") === 0)) {
        for (var r = 0; r < t.length; r += 1)
            if (t[r] === n)
                return;
        t.push(n);
    } }
    function us(t) {
        if (typeof t != "string" || t.length === 0)
            return [];
        var e = [], n = "";
        for (var r = 0; r < t.length; r += 1) {
            var i = t.charAt(r);
            if (i === "\r" || i === "\n") {
                In(e, n), n = "", i === "\r" && t.charAt(r + 1) === "\n" && (r += 1);
                continue;
            }
            n += i;
        }
        return In(e, n), e;
    }
    function ds(t) { var e = []; if (!Array.isArray(t))
        return e; for (var n = 0; n < t.length; n += 1) {
        var r = t[n];
        typeof r == "string" && In(e, r);
    } return e; }
    function hs(t) { var e = []; if (!Array.isArray(t))
        return e; for (var n = 0; n < t.length; n += 1) {
        var r = t[n];
        typeof r != "string" || r.length === 0 || r.indexOf("<truesurfer-") === 0 || r.indexOf("__trueo") === 0 || (e[e.length] = r);
    } return e; }
    function ms(t) {
        var e = [];
        if (typeof t != "string" || t.length === 0)
            return e;
        var n = "";
        for (var r = 0; r < t.length; r += 1) {
            var i = t.charAt(r);
            if (i === "\r" || i === "\n") {
                n.length > 0 && n.indexOf("<truesurfer-") !== 0 && n.indexOf("__trueo") !== 0 && (e[e.length] = n), n = "", i === "\r" && t.charAt(r + 1) === "\n" && (r += 1);
                continue;
            }
            n += i;
        }
        return n.length > 0 && n.indexOf("<truesurfer-") !== 0 && n.indexOf("__trueo") !== 0 && (e[e.length] = n), e;
    }
    function fs(t) { var e = En("__TRUEOS_WIDGET_TEXT_ROWS_TEXT__"), n = En("__TRUEOS_WIDGET_TEXT_ROWS__"), r = hs(n); if (r.length > 0)
        return { source: "array-trusted", rows: r }; var i = ms(e); if (i.length > 0)
        return { source: "text-trusted", rows: i }; var o = us(e); if (o.length > 0)
        return { source: "text", rows: o }; var s = ds(n); if (s.length > 0)
        return { source: "array", rows: s }; var c = Bi(t); if (Dt()) {
        var a = Array.isArray(n) && typeof n[0] == "string" ? wn(n[0], 72) : "", h = typeof e == "string" ? wn(e, 72) : "";
        console.log("[trueos pixi widgets] text-fallback-globals text_type=".concat(typeof e, " text_len=").concat(typeof e == "string" ? e.length : 0, " text_rows=").concat(o.length, " text_sample=\"").concat(h, "\" array=").concat(Array.isArray(n) ? n.length : -1, " array_rows=").concat(s.length, " array0=\"").concat(a, "\" html_len=").concat(t.length, " html_rows=").concat(c.length));
    } return { source: "html", rows: c }; }
    function ps() { var e; var t = En("__TRUEOS_WIDGET_RENDER_TREE_JSON__"); if (typeof t == "string" && t.length > 0)
        try {
            return { source: "json", tree: JSON.parse(t) };
        }
        catch (n) {
            Dt() && console.log("[trueos pixi widgets] render-tree-json parse failed err=".concat(String((e = n == null ? void 0 : n.message) != null ? e : n)));
        } return { source: "window", tree: En("__TRUEOS_WIDGET_RENDER_TREE__") }; }
    function gs(t) { var o, s, c, a; var e = [], n = String(t != null ? t : "").replace(/<script[^]*?<\/script>/gi, " ").replace(/<style[^]*?<\/style>/gi, " "), r = /<(h[1-6]|p|label|button)\b[^>]*>([^]*?)<\/\1>|<input\b[^>]*>/gi, i; for (; (i = r.exec(n)) && e.length < We;) {
        var h = (o = i[0]) != null ? o : "", m = String((s = i[1]) != null ? s : "").toLowerCase();
        if (h.toLowerCase().startsWith("<input"))
            continue;
        var d = Kn(m === "p" || m === "label" ? (c = i[2]) != null ? c : "" : (a = i[2]) != null ? a : "");
        d.length > 0 && e.push(d);
    } return e; }
    function bs(t) { var e = gs(t), n = qe(e); return qe(n); }
    function _s(t, e, n, r) {
        var e_38, _a;
        var a, h, m, d, b, y;
        var i = qe((h = Di.get(String((a = t.key) != null ? a : ""))) != null ? h : []), o = qe(String((d = (m = t.attrs) == null ? void 0 : m["data-trueos-srcdoc-text"]) != null ? d : "").split("\n").map(function (w) { return ge(w); })), s = i.length > 0 ? i : o.length > 0 ? o : bs(String((y = (b = t.attrs) == null ? void 0 : b.srcdoc) != null ? y : "")), c = n + 48;
        try {
            for (var s_2 = __values(s), s_2_1 = s_2.next(); !s_2_1.done; s_2_1 = s_2.next()) {
                var w = s_2_1.value;
                if (r.length >= We)
                    return;
                r.push({ x: e + 16, y: c, text: w }), c += 32;
            }
        }
        catch (e_38_1) { e_38 = { error: e_38_1 }; }
        finally {
            try {
                if (s_2_1 && !s_2_1.done && (_a = s_2.return)) _a.call(s_2);
            }
            finally { if (e_38) throw e_38.error; }
        }
    }
    function Sn(t) { var e; return t.kind === "text" ? (e = t.text) != null ? e : "" : t.children.map(Sn).join(" "); }
    function Ui(t) { var e; return t.kind === "text" ? (e = t.text) != null ? e : "" : t.children.map(Ui).join(" "); }
    function ys(t) { var e = [], n = function (r, i, o, s) {
        var e_39, _a;
        var u, _, p;
        if (e.length >= We)
            return;
        var c = i + r.x, a = o + r.y, h = r.kind === "block" && r.tagName === "iframe" && String((_ = (u = r.attrs) == null ? void 0 : u["data-root"]) != null ? _ : "") !== "1", m = s + (h ? 1 : 0), d = r.kind === "block" && r.tagName === "button", b = r.kind === "text" ? (p = r.text) != null ? p : "" : d ? Sn(r) : "", y = ge(jn(b)), w = e.length;
        if (ns(y)) {
            var S = d ? c + 8 : c, O = d ? a + Math.max(0, Math.floor((r.height - ne.fontSize * 1.25) / 2)) : a;
            e.push({ x: S, y: O, text: y });
        }
        if (!d) {
            try {
                for (var _b = __values(r.children), _c = _b.next(); !_c.done; _c = _b.next()) {
                    var S = _c.value;
                    n(S, c, a, m);
                }
            }
            catch (e_39_1) { e_39 = { error: e_39_1 }; }
            finally {
                try {
                    if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
                }
                finally { if (e_39) throw e_39.error; }
            }
            h && e.length === w && _s(r, c, a, e);
        }
    }; return n(t, 0, 0, 0), e; }
    function xs(t, e) {
        if (e === void 0) { e = 8; }
        var n = [];
        for (var r = 0; r < t.length && n.length < e; r += 1) {
            var i = t[r];
            n.push("#".concat(n.length, " x=").concat(Math.round(i.x), " y=").concat(Math.round(i.y), " text=\"").concat(Mn(i.text), "\""));
        }
        return n.join("|");
    }
    function ws() {
        var e_40, _a;
        var i, o, s, c;
        var t = (o = (i = window.__pixiCapture) == null ? void 0 : i.commands) != null ? o : [], e = {}, n = {}, r = new Set(["addChild", "addChildAt", "setChildIndex", "removeChild", "removeChildren", "removeAllListeners", "on", "clear", "rect", "roundRect", "circle", "ellipse", "moveTo", "lineTo", "closePath", "poly", "fill", "stroke", "image", "visible", "alpha", "scale", "mask", "text.text.set", "text.style.set", "text.resolution.set", "text.setSize", "render", "snapshot"]);
        try {
            for (var t_2 = __values(t), t_2_1 = t_2.next(); !t_2_1.done; t_2_1 = t_2.next()) {
                var a = t_2_1.value;
                var h = Fe(a == null ? void 0 : a.op);
                h && (e[h] = ((s = e[h]) != null ? s : 0) + 1, r.has(h) || (n[h] = ((c = n[h]) != null ? c : 0) + 1));
            }
        }
        catch (e_40_1) { e_40 = { error: e_40_1 }; }
        finally {
            try {
                if (t_2_1 && !t_2_1.done && (_a = t_2.return)) _a.call(t_2);
            }
            finally { if (e_40) throw e_40.error; }
        }
        return { total: t.length, ops: Yn(e, 24), unsupported: Yn(n, 24) };
    }
    function Ci(t, e, n, r, i, o) {
        if (i === void 0) { i = ""; }
        if (o === void 0) { o = { hash: "", renderHash: "", layoutHash: "", bytes: 0 }; }
        if (!Dt())
            return;
        var s = ws();
        window.__TRUEOS_PIXI_BRIDGE_STATS__ = { renderNodes: t.nodes, renderBlocks: t.blocks, renderText: t.text, renderTags: Yn(t.tags, 24), renderTextSamples: n, layoutBoxes: e.nodes, layoutBlocks: e.blocks, layoutText: e.text, layoutMaxDepth: e.maxDepth, layoutTextSamples: r, layoutWidgetSamples: i, prePixiHash: o.hash, prePixiRenderHash: o.renderHash, prePixiLayoutHash: o.layoutHash, prePixiTraceBytes: o.bytes, measureTextCalls: zn, scrollbarVisible: l.scroll.track.h > 0 ? 1 : 0, scrollbarTrack: "".concat(Math.round(l.scroll.track.x), ",").concat(Math.round(l.scroll.track.y), ",").concat(Math.round(l.scroll.track.w), ",").concat(Math.round(l.scroll.track.h)), scrollbarThumb: "".concat(Math.round(l.scroll.thumb.x), ",").concat(Math.round(l.scroll.thumb.y), ",").concat(Math.round(l.scroll.thumb.w), ",").concat(Math.round(l.scroll.thumb.h)), pixiCommands: s.total, pixiOps: s.ops, pixiUnsupported: s.unsupported };
    }
    var Ai = new WeakMap;
    function Vn(t, e) { var n = t; for (; n;) {
        if (n === e)
            return !0;
        n = n.parent;
    } return !1; }
    function Xi(t) { return Array.isArray(t.children) || (t.children = []), t.children; }
    function te(t, e, n) { var r = Number(e) || 0, i = Number(n) || 0; (!t.position || typeof t.position != "object") && (t.position = { x: 0, y: 0 }), t.position.x = r, t.position.y = i; }
    function Qe(t, e, n) { if (e === t || Vn(t, e))
        return; var r = Xi(t); if (e.parent !== t) {
        var c = Math.max(0, Math.min(n, r.length));
        t.addChildAt(e, c);
        return;
    } var i = Math.max(0, r.length - 1), o = Math.max(0, Math.min(n, i)); t.getChildIndex(e) !== o && t.setChildIndex(e, o); }
    function Ni(t, e, n) { if (e === t || Vn(t, e))
        return; var r = Xi(t); if (e.parent !== t) {
        var c = Math.max(0, Math.min(n, r.length));
        t.addChildAt(e, c);
        return;
    } var i = Math.max(0, r.length - 1), o = Math.max(0, Math.min(n, i)); t.getChildIndex(e) !== o && t.setChildIndex(e, o); }
    var yn = null, xt = null, Ut = null;
    function ee(t) { var e = l.cursorColors.get(t); if (e != null)
        return e; var n = [1118481, 2450411, 1483594, 14427686, 8141549, 959977, 16096779], r = Math.abs(Number(t) || 0) % n.length, i = n[r]; return l.cursorColors.set(t, i), i; }
    function Bt(t) { var i, o, s, c, a, h; var e = Number((s = (o = t == null ? void 0 : t.pointerId) != null ? o : (i = t == null ? void 0 : t.data) == null ? void 0 : i.pointerId) != null ? s : 0), r = String((h = (a = t == null ? void 0 : t.pointerType) != null ? a : (c = t == null ? void 0 : t.data) == null ? void 0 : c.pointerType) != null ? h : "").toLowerCase() === "mouse" || e === 1 || e === l.primaryMousePointerId; return l.harness.enabled && r ? l.harness.activeUserPointerId : e; }
    function Dt() { return !!globalThis.__TRUEOS_CAPTURE_ONLY__; }
    function Re(t, e) { if (!t)
        return e; if (!e)
        return t; var n = Math.min(t.x, e.x), r = Math.min(t.y, e.y), i = Math.max(t.x + t.w, e.x + e.w), o = Math.max(t.y + t.h, e.y + e.h); return { x: n, y: r, w: Math.max(0, i - n), h: Math.max(0, o - r) }; }
    function Ts(t, e, n) { return { x: t.x + e, y: t.y + n, w: t.w, h: t.h }; }
    function Es(t) {
        var e_41, _a;
        var o, s, c, a, h;
        var e = null, n = t == null ? void 0 : t.hitArea;
        n && Number.isFinite(Number(n.x)) && Number.isFinite(Number(n.y)) && Number((o = n.width) != null ? o : n.w) > 0 && Number((s = n.height) != null ? s : n.h) > 0 && (e = Re(e, { x: Number(n.x) || 0, y: Number(n.y) || 0, w: Number((c = n.width) != null ? c : n.w) || 0, h: Number((a = n.height) != null ? a : n.h) || 0 }));
        var r = Array.isArray(t == null ? void 0 : t.commands) ? t.commands : [];
        try {
            for (var r_1 = __values(r), r_1_1 = r_1.next(); !r_1_1.done; r_1_1 = r_1.next()) {
                var m = r_1_1.value;
                if (!Array.isArray(m))
                    continue;
                var d = String((h = m[0]) != null ? h : "");
                if (d === "rect" || d === "roundRect") {
                    var b = { x: Number(m[1]) || 0, y: Number(m[2]) || 0, w: Math.max(0, Number(m[3]) || 0), h: Math.max(0, Number(m[4]) || 0) };
                    e = Re(e, b);
                }
                else if (d === "circle") {
                    var b = Number(m[1]) || 0, y = Number(m[2]) || 0, w = Math.max(0, Number(m[3]) || 0);
                    e = Re(e, { x: b - w, y: y - w, w: w * 2, h: w * 2 });
                }
                else if (d === "ellipse") {
                    var b = Number(m[1]) || 0, y = Number(m[2]) || 0, w = Math.max(0, Number(m[3]) || 0), u = Math.max(0, Number(m[4]) || 0);
                    e = Re(e, { x: b - w, y: y - u, w: w * 2, h: u * 2 });
                }
            }
        }
        catch (e_41_1) { e_41 = { error: e_41_1 }; }
        finally {
            try {
                if (r_1_1 && !r_1_1.done && (_a = r_1.return)) _a.call(r_1);
            }
            finally { if (e_41) throw e_41.error; }
        }
        var i = typeof (t == null ? void 0 : t.text) == "string" ? t.text : typeof (t == null ? void 0 : t._text) == "string" ? t._text : "";
        if (i.length > 0) {
            var m = Math.max(1, Number(t == null ? void 0 : t.width) || i.length * ne.fontSize * .7), d = Math.max(1, Number(t == null ? void 0 : t.height) || ne.fontSize * 1.25);
            e = Re(e, { x: 0, y: 0, w: m, h: d });
        }
        return e;
    }
    function Li(t) { var e = function (i, o, s) {
        var e_42, _a;
        var d, b, y, w;
        var c = o + (Number((b = (d = i == null ? void 0 : i.position) == null ? void 0 : d.x) != null ? b : i == null ? void 0 : i.x) || 0), a = s + (Number((w = (y = i == null ? void 0 : i.position) == null ? void 0 : y.y) != null ? w : i == null ? void 0 : i.y) || 0), h = Es(i);
        h && (h = Ts(h, c, a));
        var m = Array.isArray(i == null ? void 0 : i.children) ? i.children : [];
        try {
            for (var m_1 = __values(m), m_1_1 = m_1.next(); !m_1_1.done; m_1_1 = m_1.next()) {
                var u = m_1_1.value;
                h = Re(h, e(u, c, a));
            }
        }
        catch (e_42_1) { e_42 = { error: e_42_1 }; }
        finally {
            try {
                if (m_1_1 && !m_1_1.done && (_a = m_1.return)) _a.call(m_1);
            }
            finally { if (e_42) throw e_42.error; }
        }
        return h;
    }, r = e(t, 0, 0); return r ? { x: r.x - 4, y: r.y - 4, w: r.w + 4 * 2, h: r.h + 4 * 2 } : null; }
    function At(t) { var i; if (!Dt() || (window.__TRUEOS_PIXI_APP_PHASE__ = t, !{ "main:start": !0, "main:yoga": !0, "main:create-app": !0, "main:attach-capture": !0, "main:append-canvas": !0, "main:capture-flags": !0, "main:canvas-listeners": !0, "main:stage:done": !0, "main:roots": !0, "main:text-measure": !0, "main:html": !0, "main:render-tree": !0, "main:first-rerender": !0, "main:layout-build": !0, "main:layout-commit": !0, "main:paint:clamp": !0, "main:paint:render-to-pixi": !0, "main:paint:scrollbar": !0, "main:paint:renderer-render": !0, "main:paint:done": !0, "main:cursor-setup": !0, "main:input-listeners": !0, "main:done": !0 }[t]))
        return; var n = window, r = (i = n.__TRUEOS_PIXI_PHASE_TRACE_SEEN__) != null ? i : n.__TRUEOS_PIXI_PHASE_TRACE_SEEN__ = {}; r[t] || (r[t] = 1, console.log("[Trace] [pixi] phase=".concat(t))); }
    function B(t) { Dt() && (window.__TRUEOS_PIXI_LAYOUT_STEP__ = t); }
    function Lt(t) { Dt() && console.log(t); }
    function he(t, e, n) { var o; if (!Dt())
        return; var r = "__TRUEOS_".concat(t, "_LOG_COUNT__"), i = Number((o = window[r]) != null ? o : 0) || 0; i >= e || (window[r] = i + 1, console.log(n)); }
    function Yi(t) { var c, a, h, m, d; var e = (c = window.__TRUEOS_PIXI_APP_PHASE__) != null ? c : "unknown", n = (a = window.__TRUEOS_PIXI_LAYOUT_STEP__) != null ? a : "", r = t, i = String((h = r == null ? void 0 : r.name) != null ? h : "Error"), o = String((m = r == null ? void 0 : r.message) != null ? m : t), s = String((d = r == null ? void 0 : r.stack) != null ? d : ""); return "phase=".concat(e, " layout=").concat(n, " name=").concat(i, " message=").concat(o, " stack=").concat(s); }
    function Is() { var t = Math.max(1, Number(window.innerWidth || 1920) | 0), e = Math.max(1, Number(window.innerHeight || 1080) | 0), n = new Rt(0, 0, t, e), r = document.createElement("canvas"), i = { width: t, height: e, screen: n, render: function (o) { return o; }, resize: function (o, s) { var c = Math.max(1, Number(o || t) | 0), a = Math.max(1, Number(s || e) | 0); this.width = c, this.height = a, n.width = c, n.height = a; } }; return { stage: new Ot, screen: n, canvas: r, renderer: i, ticker: { stop: function () { }, add: function () { }, remove: function () { } } }; }
    function Ms() { var p = 0, S = 0, O = 2e4; return { Node: { create: function () { return ({ children: [], measureFunc: null, paddingLeft: 0, paddingTop: 0, paddingRight: 0, paddingBottom: 0, marginLeft: 0, marginTop: 0, marginRight: 0, marginBottom: 0, width: 0, height: 0, minWidth: 0, minHeight: 0, flexDirection: 0, alignItems: 0, justifyContent: 1, flexWrap: 0, positionType: 0, positionLeft: null, positionTop: null, positionRight: null, positionBottom: null, computed: { left: 0, top: 0, width: 0, height: 0 }, debugLabel: "node", setMeasureFunc: function (T) { this.measureFunc = T; }, setMargin: function (T, A) { var G = Number(A) || 0; T === 0 ? this.marginLeft = G : T === 1 ? this.marginTop = G : T === 2 ? this.marginRight = G : T === 3 && (this.marginBottom = G); }, setPadding: function (T, A) { var G = Number(A) || 0; T === 0 ? this.paddingLeft = G : T === 1 ? this.paddingTop = G : T === 2 ? this.paddingRight = G : T === 3 && (this.paddingBottom = G); }, setFlexDirection: function (T) { this.flexDirection = T; }, setAlignItems: function (T) { this.alignItems = Number(T) || 0; }, setJustifyContent: function (T) { this.justifyContent = Number(T) || 0; }, setFlexWrap: function (T) { this.flexWrap = Number(T) === 1 ? 1 : 0; }, setFlexGrow: function (T) { }, setFlexShrink: function (T) { }, setAlignSelf: function (T) { }, setPositionType: function (T) { this.positionType = Number(T) === 1 ? 1 : 0; }, setPosition: function (T, A) { var G = Number(A) || 0; T === 0 ? this.positionLeft = G : T === 1 ? this.positionTop = G : T === 2 ? this.positionRight = G : T === 3 && (this.positionBottom = G); }, setWidth: function (T) { this.width = Math.max(0, Number(T) || 0); }, setHeight: function (T) { this.height = Math.max(0, Number(T) || 0); }, setMinWidth: function (T) { this.minWidth = Math.max(0, Number(T) || 0); }, setMinHeight: function (T) { this.minHeight = Math.max(0, Number(T) || 0); }, insertChild: function (T, A) { this.children.splice(Math.max(0, Math.min(A, this.children.length)), 0, T); }, getChildCount: function () { return this.children.length; }, getComputedLeft: function () { return this.computed.left; }, getComputedTop: function () { return this.computed.top; }, getComputedWidth: function () { return this.computed.width; }, getComputedHeight: function () { return this.computed.height; }, freeRecursive: function () { }, calculateLayout: function (T, A) {
                    if (T === void 0) { T = this.width; }
                    if (A === void 0) { A = this.height; }
                    this.layout(0, 0, Math.max(1, Number(T) || this.width || 1), Math.max(1, Number(A) || this.height || 1));
                }, layout: function (T, A, G, k) {
                    var e_43, _a, e_44, _b, e_45, _c;
                    var v, H, q, rt;
                    if (p += 1, (p <= 80 || p % 500 === 0) && (S += 1, S <= 140 && Lt("[trueos pixi widgets] yoga-layout-call #".concat(p, " label=\"").concat(this.debugLabel, "\" children=").concat(this.children.length, " flex=").concat(this.flexDirection, " pos=").concat(this.positionType, " xy=").concat(Math.round(T), ",").concat(Math.round(A), " avail=").concat(Math.round(G), "x").concat(Math.round(k), " own=").concat(Math.round(this.width), "x").concat(Math.round(this.height), " min=").concat(Math.round(this.minWidth), "x").concat(Math.round(this.minHeight)))), p > O)
                        throw new Error("capture yoga layout budget exceeded count=".concat(p, " label=\"").concat(this.debugLabel, "\" children=").concat(this.children.length, " flex=").concat(this.flexDirection, " pos=").concat(this.positionType, " avail=").concat(Math.round(G), "x").concat(Math.round(k)));
                    var P = this.paddingLeft + this.paddingRight, X = this.paddingTop + this.paddingBottom, j = Math.max(this.minWidth, this.width || G), f = Math.max(this.minHeight, this.height || 0);
                    if (this.computed.left = T, this.computed.top = A, this.computed.width = j, this.measureFunc) {
                        var ot = this.measureFunc(Math.max(0, j - P), 0);
                        this.width <= 0 && this.minWidth <= 0 && (this.computed.width = Math.ceil(Math.max(0, Number(ot.width) || 0)) + P), f = Math.max(f, Math.ceil(Number(ot.height) || 0) + X), this.computed.height = f;
                        return;
                    }
                    if (this.flexDirection === 1) {
                        var ot = this.paddingLeft, K = 0, et = Math.max(1, this.children.length);
                        try {
                            for (var _d = __values(this.children), _f = _d.next(); !_f.done; _f = _d.next()) {
                                var C = _f.value;
                                if (C.positionType === 1)
                                    continue;
                                var V = C.width || C.minWidth || Math.max(24, (j - P) / et);
                                C.layout(ot + C.marginLeft, this.paddingTop + C.marginTop, V, k), ot += C.computed.width + C.marginLeft + C.marginRight, K = Math.max(K, C.computed.height + C.marginTop + C.marginBottom);
                            }
                        }
                        catch (e_43_1) { e_43 = { error: e_43_1 }; }
                        finally {
                            try {
                                if (_f && !_f.done && (_a = _d.return)) _a.call(_d);
                            }
                            finally { if (e_43) throw e_43.error; }
                        }
                        try {
                            for (var _g = __values(this.children), _h = _g.next(); !_h.done; _h = _g.next()) {
                                var C = _h.value;
                                if (C.positionType === 1) {
                                    var V = C.width || C.minWidth || Math.max(0, j - P - C.marginLeft - C.marginRight), Y = C.height || C.minHeight || k, $ = C.positionLeft != null ? this.paddingLeft + C.positionLeft : Math.max(0, j - this.paddingRight - ((v = C.positionRight) != null ? v : 0) - V), tt = C.positionTop != null ? this.paddingTop + C.positionTop : Math.max(0, f - this.paddingBottom - ((H = C.positionBottom) != null ? H : 0) - Y);
                                    C.layout($ + C.marginLeft, tt + C.marginTop, V, Y);
                                }
                            }
                        }
                        catch (e_44_1) { e_44 = { error: e_44_1 }; }
                        finally {
                            try {
                                if (_h && !_h.done && (_b = _g.return)) _b.call(_g);
                            }
                            finally { if (e_44) throw e_44.error; }
                        }
                        f = Math.max(f, K + X);
                    }
                    else {
                        var ot = this.paddingTop;
                        try {
                            for (var _j = __values(this.children), _k = _j.next(); !_k.done; _k = _j.next()) {
                                var K = _k.value;
                                if (K.positionType === 1) {
                                    var C = K.width || K.minWidth || Math.max(0, j - P - K.marginLeft - K.marginRight), V = K.height || K.minHeight || k, Y = K.positionLeft != null ? this.paddingLeft + K.positionLeft : Math.max(0, j - this.paddingRight - ((q = K.positionRight) != null ? q : 0) - C), $ = K.positionTop != null ? this.paddingTop + K.positionTop : Math.max(0, f - this.paddingBottom - ((rt = K.positionBottom) != null ? rt : 0) - V);
                                    K.layout(Y + K.marginLeft, $ + K.marginTop, C, V);
                                    continue;
                                }
                                var et = Math.max(0, j - P - K.marginLeft - K.marginRight);
                                K.layout(this.paddingLeft + K.marginLeft, ot + K.marginTop, et, k), ot += K.computed.height + K.marginTop + K.marginBottom;
                            }
                        }
                        catch (e_45_1) { e_45 = { error: e_45_1 }; }
                        finally {
                            try {
                                if (_k && !_k.done && (_c = _j.return)) _c.call(_j);
                            }
                            finally { if (e_45) throw e_45.error; }
                        }
                        f = Math.max(f, ot + this.paddingBottom);
                    }
                    this.computed.height = Math.max(this.minHeight, f);
                } }); } }, EDGE_LEFT: 0, EDGE_TOP: 1, EDGE_RIGHT: 2, EDGE_BOTTOM: 3, FLEX_DIRECTION_COLUMN: 0, FLEX_DIRECTION_ROW: 1, FLEX_DIRECTION_ROW_REVERSE: 1, ALIGN_STRETCH: 0, ALIGN_CENTER: 1, ALIGN_FLEX_START: 2, JUSTIFY_CENTER: 0, JUSTIFY_FLEX_START: 1, JUSTIFY_SPACE_BETWEEN: 2, WRAP_WRAP: 1, WRAP_NO_WRAP: 0, POSITION_TYPE_RELATIVE: 0, POSITION_TYPE_ABSOLUTE: 1, DIRECTION_LTR: 0, MEASURE_MODE_UNDEFINED: 0 }; }
    function Ss(t) {
        var e_46, _a;
        var r;
        var e = 0, n = function (i, o, s) {
            var e_47, _a;
            var h;
            var c = o + i.x, a = s + i.y;
            if (!(i.kind === "block" && i.tagName === "dialog")) {
                e = Math.max(e, a + i.height);
                try {
                    for (var _b = __values((h = i.children) != null ? h : []), _c = _b.next(); !_c.done; _c = _b.next()) {
                        var m = _c.value;
                        n(m, c, a);
                    }
                }
                catch (e_47_1) { e_47 = { error: e_47_1 }; }
                finally {
                    try {
                        if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
                    }
                    finally { if (e_47) throw e_47.error; }
                }
            }
        };
        try {
            for (var _b = __values((r = t.children) != null ? r : []), _c = _b.next(); !_c.done; _c = _b.next()) {
                var i = _c.value;
                n(i, 0, 0);
            }
        }
        catch (e_46_1) { e_46 = { error: e_46_1 }; }
        finally {
            try {
                if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
            }
            finally { if (e_46) throw e_46.error; }
        }
        return e;
    }
    function Ze(t, e) { var o, s, c, a; var n = l.inputs.get(t); if (n)
        return n; var r = {}, i = ((o = e == null ? void 0 : e.type) != null ? o : "text").toLowerCase(); if (i === "checkbox" || i === "radio") {
        if (r.checked = e ? Object.prototype.hasOwnProperty.call(e, "checked") : !1, i === "checkbox") {
            var h = ((s = e == null ? void 0 : e["aria-checked"]) != null ? s : "").toLowerCase(), m = ((c = e == null ? void 0 : e["data-indeterminate"]) != null ? c : "").toLowerCase();
            r.indeterminate = (e ? Object.prototype.hasOwnProperty.call(e, "indeterminate") : !1) || h === "mixed" || m === "true" || m === "1" || m === "yes";
        }
    }
    else
        r.value = (a = e == null ? void 0 : e.value) != null ? a : ""; return l.inputs.set(t, r), r; }
    function Ps(t) { var e = new Map; function n(r) {
        var e_48, _a;
        var i, o, s, c, a;
        if (r.kind === "block" && r.tagName === "input" && ((o = (i = r.attrs) == null ? void 0 : i.type) != null ? o : "text").toLowerCase() === "radio") {
            var d = "radio:".concat((c = (s = r.attrs) == null ? void 0 : s.name) != null ? c : "__default__"), b = r.key;
            if (b) {
                var y = (a = e.get(d)) != null ? a : [];
                y.push(b), e.set(d, y);
            }
        }
        try {
            for (var _b = __values(r.children), _c = _b.next(); !_c.done; _c = _b.next()) {
                var h = _c.value;
                n(h);
            }
        }
        catch (e_48_1) { e_48 = { error: e_48_1 }; }
        finally {
            try {
                if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
            }
            finally { if (e_48) throw e_48.error; }
        }
    } return n(t), e; }
    function ge(t) { var e = "", n = !1, r = typeof t == "string" ? t : ""; for (var i = 0; i < r.length; i += 1) {
        var o = r.charCodeAt(i);
        if (o === 32 || o === 9 || o === 10 || o === 13 || o === 12) {
            n = !0;
            continue;
        }
        n && e.length > 0 && (e += " "), e += r.charAt(i), n = !1;
    } return e; }
    function ks(t) {
        var e_49, _a;
        if (!t || typeof t != "object")
            return;
        var e = {};
        try {
            for (var _b = __values(Object.entries(t)), _c = _b.next(); !_c.done; _c = _b.next()) {
                var _d = __read(_c.value, 2), n = _d[0], r = _d[1];
                typeof n != "string" || n.length === 0 || (e[n] = typeof r == "string" ? r : "");
            }
        }
        catch (e_49_1) { e_49 = { error: e_49_1 }; }
        finally {
            try {
                if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
            }
            finally { if (e_49) throw e_49.error; }
        }
        return Object.keys(e).length > 0 ? e : void 0;
    }
    function Ki(t, e, n) { var h, m; if (!t || typeof t != "object")
        return null; var r = t, i = typeof r.kind == "string" ? r.kind : ""; if (i === "text") {
        var d = typeof r.text == "string" ? r.text : "", b = "", y = (h = n == null ? void 0 : n.rows[n.index]) != null ? h : "", w = !1;
        if (n && n.index < n.rows.length ? (n.index += 1, b = y, w = !0) : b = ge(jn(d)), !w && (d.indexOf("<truesurfer-") >= 0 || d.indexOf("__trueo") >= 0) || b.startsWith("<truesurfer-") || b.startsWith("__trueo"))
            b = "";
        else if (b.length === 0) {
            var _ = (m = n == null ? void 0 : n.rows[n.index]) != null ? m : "";
            n && _ && (n.index += 1), _ && (b = _);
        }
        return b.length > 0 ? { kind: "text", text: b } : null;
    } if (i !== "block")
        return null; var o = typeof r.tagName == "string" ? r.tagName.toLowerCase() : ""; if (o.length === 0)
        return null; var s = typeof r.key == "string" ? r.key : "".concat(e, ":").concat(o), c = [], a = Array.isArray(r.children) ? r.children : []; for (var d = 0; d < a.length; d += 1) {
        var b = Ki(a[d], "".concat(e, ".").concat(d), n);
        b && c.push(b);
    } return { kind: "block", key: s, tagName: o, attrs: ks(r.attrs), children: c }; }
    function Rs(t, e) { var n = Array.isArray(t) ? t : t && typeof t == "object" && Array.isArray(t.widgetRenderTree) ? t.widgetRenderTree : [], i = { rows: Array.isArray(e) ? qe(e) : Bi(e), index: 0 }, o = []; for (var s = 0; s < n.length; s += 1) {
        var c = Ki(n[s], "0.".concat(s), i);
        c && o.push(c);
    } return o; }
    function Os(t, e) { if (!Array.isArray(e) || e.length === 0)
        return 0; var n = 0, r = 0, i = function (o) { if (o.kind === "text") {
        if (n < e.length) {
            var s = e[n];
            n += 1, typeof s == "string" && s.length > 0 && s.indexOf("<truesurfer-") !== 0 && s.indexOf("__trueo") !== 0 && (o.text = s, r += 1);
        }
        return;
    } for (var s = 0; s < o.children.length; s += 1)
        i(o.children[s]); }; for (var o = 0; o < t.length; o += 1)
        i(t[o]); return r; }
    function Cs(t) { var n = document.createElement("canvas").getContext("2d"); if (!n)
        throw new Error("2D canvas not available"); n.font = t; var r = t.indexOf("px"), i = r; for (; i > 0;) {
        var c = t.charCodeAt(i - 1);
        if (c < 48 || c > 57)
            break;
        i -= 1;
    } var o = r > i ? Number(t.slice(i, r)) : 16, s = Math.ceil(o * 1.25); return { measure: function (c, a) {
            var e_50, _a;
            zn += 1;
            var h = ge(c).split(" ").filter(Boolean);
            if (h.length === 0)
                return { width: 0, height: s, lines: [""] };
            var m = [], d = "";
            try {
                for (var h_1 = __values(h), h_1_1 = h_1.next(); !h_1_1.done; h_1_1 = h_1.next()) {
                    var w = h_1_1.value;
                    var u = d ? "".concat(d, " ").concat(w) : w, _ = n.measureText(u).width, p = a != null ? a : Number.POSITIVE_INFINITY;
                    _ <= p || !d ? d = u : (m.push(d), d = w);
                }
            }
            catch (e_50_1) { e_50 = { error: e_50_1 }; }
            finally {
                try {
                    if (h_1_1 && !h_1_1.done && (_a = h_1.return)) _a.call(h_1);
                }
                finally { if (e_50) throw e_50.error; }
            }
            d && m.push(d);
            var b = Math.min(Math.max.apply(Math, __spreadArray([], __read(m.map(function (w) { return n.measureText(w).width; })), false)), a != null ? a : Number.POSITIVE_INFINITY), y = m.length * s;
            return { width: Math.ceil(b), height: Math.ceil(y), lines: m };
        }, lineHeight: s, font: t }; }
    function As(t, e, n) { var w; B("build:start nodes=".concat(t.length, " viewport=").concat(e, "x").concat(n)), window.__TRUEOS_PIXI_LAYOUT_BUILD_COUNT__ = 0, Lt("[trueos pixi widgets] layout-build begin nodes=".concat(t.length, " viewport=").concat(e, "x").concat(n)); var r = 12, i = 8, o = ne; B("build:measurer"); var s = Cs("".concat(o.fontSize, "px ").concat(o.fontFamily)); function c(u) { return u.kind !== "block" || u.tagName === "hr" || u.tagName === "tr" || u.tagName === "td" || u.tagName === "th" ? 0 : i; } var a = 0; function h(u) { if (a += 1, (a <= 140 || a % 250 === 0) && Lt("[trueos pixi widgets] layout-box-build #".concat(a, " label=\"").concat(u, "\"")), a > 5e3)
        throw new Error("layout box build budget exceeded count=".concat(a, " label=\"").concat(u, "\"")); } function m(u) { var U; var _ = u.kind === "text" ? "text:".concat(u.text.slice(0, 24)) : "".concat(u.tagName, ":").concat(u.key); if (B("node:".concat(_, ":start")), u.kind === "text") {
        var T_1 = pt.Node.create();
        return T_1.debugLabel = _, B("node:".concat(_, ":measure-func")), T_1.setMeasureFunc(function (A, G) { B("node:".concat(_, ":measure-call")); var k = G === pt.MEASURE_MODE_UNDEFINED ? void 0 : Math.max(0, A), P = s.measure(u.text, k); return { width: P.width, height: P.height }; }), T_1.setMargin(pt.EDGE_RIGHT, 6), T_1.setMargin(pt.EDGE_BOTTOM, 0), { yogaNode: T_1, buildBox: function () { return (h(_), { kind: "text", text: u.text, x: T_1.getComputedLeft(), y: T_1.getComputedTop(), width: T_1.getComputedWidth(), height: T_1.getComputedHeight(), children: [] }); } };
    } if (u.tagName === "sliderlabel")
        return B("node:".concat(u.tagName, ":").concat(u.key, ":sliderlabel")), fr({ node: u, Yoga: pt, measurer: s }); B("node:".concat(u.tagName, ":").concat(u.key, ":create")); var p = pt.Node.create(); if (p.debugLabel = _, B("node:".concat(u.tagName, ":").concat(u.key, ":base-defaults")), p.setFlexDirection(pt.FLEX_DIRECTION_COLUMN), p.setAlignItems(pt.ALIGN_STRETCH), p.setPadding(pt.EDGE_LEFT, r), p.setPadding(pt.EDGE_RIGHT, r), p.setPadding(pt.EDGE_TOP, r), p.setPadding(pt.EDGE_BOTTOM, r), p.setMargin(pt.EDGE_BOTTOM, 0), Ln(u.tagName) && (B("node:".concat(u.tagName, ":").concat(u.key, ":heading-defaults")), Rr(p, pt)), u.tagName === "hr" && (B("node:".concat(u.tagName, ":").concat(u.key, ":hr-defaults")), wr(p, pt)), (u.tagName === "p" || u.tagName === "label") && (B("node:".concat(u.tagName, ":").concat(u.key, ":inline-scan")), u.children.some(function (A) { return A.kind === "block" && (A.tagName === "input" || A.tagName === "button" || A.tagName === "select" || A.tagName === "textarea" || A.tagName === "timeinput" || A.tagName === "dateinput" || A.tagName === "monthinput" || A.tagName === "weekinput" || A.tagName === "datetimelocalinput" || A.tagName === "progress" || A.tagName === "meter" || A.tagName === "slider" || A.tagName === "number" || A.tagName === "color"); }) && (p.setFlexDirection(pt.FLEX_DIRECTION_ROW), p.setFlexWrap(pt.WRAP_WRAP), p.setAlignItems(pt.ALIGN_CENTER)), p.setPadding(pt.EDGE_TOP, 4), p.setPadding(pt.EDGE_BOTTOM, 4), p.setPadding(pt.EDGE_LEFT, 4), p.setPadding(pt.EDGE_RIGHT, 4)), u.tagName === "table" && (B("node:".concat(u.tagName, ":").concat(u.key, ":table-defaults")), Sr(p, pt)), u.tagName === "tr" && (B("node:".concat(u.tagName, ":").concat(u.key, ":tr-defaults")), Pr(p, pt)), (u.tagName === "td" || u.tagName === "th") && (B("node:".concat(u.tagName, ":").concat(u.key, ":cell-defaults")), kr(p, pt)), u.tagName === "input" && (B("node:".concat(u.tagName, ":").concat(u.key, ":input-defaults")), Jr(p, u, pt)), u.tagName === "textarea" && (B("node:".concat(u.tagName, ":").concat(u.key, ":textarea-defaults")), Zr(p, pt)), u.tagName === "select" && (B("node:".concat(u.tagName, ":").concat(u.key, ":select-defaults")), hi(p, pt)), u.tagName === "timeinput" || u.tagName === "dateinput" || u.tagName === "monthinput" || u.tagName === "weekinput" || u.tagName === "datetimelocalinput") {
        var T = u.tagName === "timeinput" ? "time" : u.tagName === "monthinput" ? "month" : u.tagName === "weekinput" ? "week" : u.tagName === "dateinput" ? "date" : "datetime-local";
        B("node:".concat(u.tagName, ":").concat(u.key, ":temporal-defaults")), fi(p, pt, T);
    } if (u.tagName === "img" && (B("node:".concat(u.tagName, ":").concat(u.key, ":img-defaults")), Hr(p, u, pt)), u.tagName === "svg" && (B("node:".concat(u.tagName, ":").concat(u.key, ":svg-defaults")), Xr(p, u, pt)), u.tagName === "canvas" && (B("node:".concat(u.tagName, ":").concat(u.key, ":canvas-defaults")), Kr(p, u, pt)), u.tagName === "iframe" && (B("node:".concat(u.tagName, ":").concat(u.key, ":iframe-defaults")), jr(p, u, pt)), u.tagName === "button") {
        B("node:".concat(u.tagName, ":").concat(u.key, ":button-defaults")), Er(p, pt);
        var T = ge(Ui(u));
        if (T.length > 0) {
            var A = s.measure(T);
            p.setMinWidth(Math.max(100, Math.ceil(A.width) + 28));
        }
    } u.tagName === "dialog" && (B("node:".concat(u.tagName, ":").concat(u.key, ":dialog-defaults")), ii(p, pt)), u.tagName === "number" && (B("node:".concat(u.tagName, ":").concat(u.key, ":number-defaults")), si(p, pt)), u.tagName === "color" && (B("node:".concat(u.tagName, ":").concat(u.key, ":color-defaults")), ci(p, u, pt)), u.tagName === "searchrow" && (B("node:".concat(u.tagName, ":").concat(u.key, ":searchrow-defaults")), ei(p, pt)), u.tagName === "searchbutton" && (B("node:".concat(u.tagName, ":").concat(u.key, ":searchbutton-defaults")), ni(p, pt)), u.tagName === "summary" && (B("node:".concat(u.tagName, ":").concat(u.key, ":summary-defaults")), br(p, pt)), u.tagName === "details" && (B("node:".concat(u.tagName, ":").concat(u.key, ":details-defaults")), _r(p, pt)), u.tagName === "barrow" && (B("node:".concat(u.tagName, ":").concat(u.key, ":barrow-defaults")), ti(p, pt)), (u.tagName === "progress" || u.tagName === "meter") && (B("node:".concat(u.tagName, ":").concat(u.key, ":progress-defaults")), hr(p, pt)), u.tagName === "slider" && (B("node:".concat(u.tagName, ":").concat(u.key, ":slider-defaults")), mr(p, pt)), B("node:".concat(u.tagName, ":").concat(u.key, ":children-effective")); var S = yr(u, l.detailsOpen), O = Number((U = window.__TRUEOS_PIXI_LAYOUT_BUILD_COUNT__) != null ? U : 0) + 1; window.__TRUEOS_PIXI_LAYOUT_BUILD_COUNT__ = O, (O <= 120 || O % 50 === 0) && Lt("[trueos pixi widgets] layout-build-node #".concat(O, " label=\"").concat(_, "\" children=").concat(u.children.length, " effective=").concat(S.length)), B("node:".concat(u.tagName, ":").concat(u.key, ":children-map count=").concat(S.length)); var D = S.map(m); (O <= 120 || O % 50 === 0) && Lt("[trueos pixi widgets] layout-build-node-mapped #".concat(O, " label=\"").concat(_, "\" pairs=").concat(D.length)), B("node:".concat(u.tagName, ":").concat(u.key, ":children-insert")); for (var T = 0; T < D.length; T++) {
        var A = S[T], G = D[T];
        if (A && A.kind === "block") {
            var k = T === D.length - 1 ? 0 : c(A);
            G.yogaNode.setMargin(pt.EDGE_BOTTOM, k);
        }
        p.insertChild(G.yogaNode, p.getChildCount());
    } return { yogaNode: p, buildBox: function () { return (h(_), { kind: "block", key: u.key, tagName: u.tagName, attrs: u.attrs, x: p.getComputedLeft(), y: p.getComputedTop(), width: p.getComputedWidth(), height: p.getComputedHeight(), children: D.map(function (T) { return T.buildBox(); }) }); } }; } var d = pt.Node.create(); d.debugLabel = "root", B("root:flex-direction"), d.setFlexDirection(pt.FLEX_DIRECTION_COLUMN), B("root:align-items"), d.setAlignItems(pt.ALIGN_STRETCH), B("root:width"), d.setWidth(e), B("root:height"), d.setHeight(n), B("root:padding-left"), d.setPadding(pt.EDGE_LEFT, 16), B("root:padding-top"), d.setPadding(pt.EDGE_TOP, 16), B("root:padding-right"), d.setPadding(pt.EDGE_RIGHT, 16 + xn), B("root:padding-bottom"), d.setPadding(pt.EDGE_BOTTOM, 16), B("root:children-map count=".concat(t.length)), Lt("[trueos pixi widgets] layout-root children-map count=".concat(t.length)); var b = t.map(m); B("root:children-insert"), Lt("[trueos pixi widgets] layout-root children-insert pairs=".concat(b.length)); for (var u = 0; u < b.length; u++) {
        var _ = t[u], p = b[u];
        if (_ && _.kind === "block") {
            var S = u === b.length - 1 ? 0 : c(_);
            p.yogaNode.setMargin(pt.EDGE_BOTTOM, S);
        }
        d.insertChild(p.yogaNode, d.getChildCount());
    } B("root:calculate"), Lt("[trueos pixi widgets] layout-root calculate begin"), d.calculateLayout(e, n, pt.DIRECTION_LTR), Lt("[trueos pixi widgets] layout-root calculate done"), B("root:build-box"), Lt("[trueos pixi widgets] layout-root build-box begin"), h("root"); var y = { kind: "block", tagName: "root", x: 0, y: 0, width: d.getComputedWidth(), height: d.getComputedHeight(), children: b.map(function (u) { return u.buildBox(); }) }; return Lt("[trueos pixi widgets] layout-root build-box done boxes=".concat(a)), B("root:free"), (w = d.freeRecursive) == null || w.call(d), B("build:done"), y; }
    function Ns(t, e, n) {
        var e_51, _a, e_52, _b, e_53, _c, e_54, _d, e_55, _f;
        var X, j;
        B("render:start");
        var r = ne, i = n != null ? n : t.stage;
        B("render:get-background");
        var o = $t(i, "__background");
        B("render:get-content-root");
        var s = ue(i, "__contentRoot");
        B("render:get-dialog-root");
        var c = ue(i, "__dialogRoot");
        B("render:get-overlay-root");
        var a = ue(i, "__overlayRoot");
        B("render:ensure-background"), Ni(i, o, 0), B("render:ensure-content-root"), Qe(i, s, 1), B("render:ensure-dialog-root"), Qe(i, c, 2), B("render:ensure-overlay-root"), Qe(i, a, 3), B("render:overlay-remove-children"), a.removeChildren(), B("render:overlay-removed");
        var h = [], m = [], d = Ps(e);
        B("render:clear-ui-state"), l.fieldBounds.clear(), l.sliderBounds.clear(), l.dialogDragBounds.clear(), l.hoverRects.length = 0, l.hoverHandlers.clear(), l.iframeRects.length = 0, l.iframeScrollRoots.clear(), l.iframeScrollbarGraphics.clear(), B("render:node-cache");
        var b = (X = Ai.get(i)) != null ? X : new Map;
        Ai.set(i, b);
        var y = new Set, w = function (f) {
            var e_56, _a;
            var q;
            var v = 0, H = function (rt, ot, K) {
                var e_57, _a;
                var V;
                if (rt.kind === "block" && rt.tagName === "dialog")
                    return;
                var et = ot + rt.x, C = K + rt.y;
                v = Math.max(v, C + rt.height);
                try {
                    for (var _b = __values((V = rt.children) != null ? V : []), _c = _b.next(); !_c.done; _c = _b.next()) {
                        var Y = _c.value;
                        H(Y, et, C);
                    }
                }
                catch (e_57_1) { e_57 = { error: e_57_1 }; }
                finally {
                    try {
                        if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
                    }
                    finally { if (e_57) throw e_57.error; }
                }
            };
            try {
                for (var _b = __values((q = f.children) != null ? q : []), _c = _b.next(); !_c.done; _c = _b.next()) {
                    var rt = _c.value;
                    H(rt, 0, 0);
                }
            }
            catch (e_56_1) { e_56 = { error: e_56_1 }; }
            finally {
                try {
                    if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
                }
                finally { if (e_56) throw e_56.error; }
            }
            return v;
        }, u = new Set;
        try {
            for (var _g = __values(l.textDrags.values()), _h = _g.next(); !_h.done; _h = _g.next()) {
                var f = _h.value;
                u.add(f.key);
            }
        }
        catch (e_51_1) { e_51 = { error: e_51_1 }; }
        finally {
            try {
                if (_h && !_h.done && (_a = _g.return)) _a.call(_g);
            }
            finally { if (e_51) throw e_51.error; }
        }
        B("render:measure");
        var _ = Qo(r);
        function p(f, v, H) { return Math.max(v, Math.min(H, f)); }
        var S = function (f) {
            var e_58, _a;
            try {
                for (var _b = __values(l.textDrags.entries()), _c = _b.next(); !_c.done; _c = _b.next()) {
                    var _d = __read(_c.value, 2), v = _d[0], H = _d[1];
                    if (H.key === f)
                        return v;
                }
            }
            catch (e_58_1) { e_58 = { error: e_58_1 }; }
            finally {
                try {
                    if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
                }
                finally { if (e_58) throw e_58.error; }
            }
            return null;
        }, O = function (f) {
            var e_59, _a;
            var v = l.keyboardOwnerPointerId;
            if (l.focusedKeyByPointer.get(v) === f)
                return v;
            try {
                for (var _b = __values(l.focusedKeyByPointer.entries()), _c = _b.next(); !_c.done; _c = _b.next()) {
                    var _d = __read(_c.value, 2), H = _d[0], q = _d[1];
                    if (q === f)
                        return H;
                }
            }
            catch (e_59_1) { e_59 = { error: e_59_1 }; }
            finally {
                try {
                    if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
                }
                finally { if (e_59) throw e_59.error; }
            }
            return null;
        };
        B("render:background-clear"), vt(o), B("render:background-rect"), o.rect(0, 0, t.renderer.width, t.renderer.height), B("render:background-fill"), o.fill(r.background), B("render:content-position");
        {
            var f = l.scroll, v = f && Number(f.y || 0) || 0, H = s.position;
            H && (H.x = 0, H.y = -v);
        }
        B("render:content-position-done");
        function D(f, v, H, q, rt, ot, K, et, C) {
            var e_60, _a;
            if (q === void 0) { q = 0; }
            if (rt === void 0) { rt = 0; }
            var ut, dt, lt, gt, g, x, L, I, E, R, M, N, J, Q, z, at, Z, yt, kt, Tt;
            B("render:draw:".concat(et, ":").concat(f.kind, ":").concat(f.kind === "block" ? f.tagName : "text", ":start"));
            var V = f.kind === "block" ? f.key && f.key.length > 0 ? f.key : "".concat(et, ":").concat((ut = f.tagName) != null ? ut : "block") : "", Y = f.kind === "block" ? "b:".concat(V) : "t:".concat(et);
            B("render:draw:".concat(et, ":cache"));
            var $ = b.get(Y);
            (!$ || Vn(v, $)) && (B("render:draw:".concat(et, ":new-container")), $ = new Ot, $.label = Y, b.set(Y, $)), B("render:draw:".concat(et, ":ensure-child")), y.add(Y), Qe(v, $, C), B("render:draw:".concat(et, ":children-root"));
            var tt = ue($, "__children");
            if (B("render:draw:".concat(et, ":ensure-children-root")), Qe($, tt, 1), B("render:draw:".concat(et, ":position")), te($, f.x, f.y), f.kind === "block" && f.tagName === "hr" && te($, Math.round(f.x), Math.round(f.y)), f.kind === "block" && f.tagName === "dialog" && f.key) {
                var bt = ln(l.dialogs, f.key), ct = Math.max(0, f.width), it = Math.max(0, f.height), ft = K.x, Ft = K.y, ht = Math.max(ft, K.x + K.w - ct), St = Math.max(Ft, K.y + K.h - it);
                if (l.dialogDragBounds.set(f.key, { minX: ft, minY: Ft, maxX: ht, maxY: St }), Dt() && !bt.__trueosInitialPositionSeeded) {
                    var zt = K.w <= 760 && K.h <= 800, re = ft + Math.max(12, Math.floor((K.w - ct) / 2)), le = Ft + Math.max(zt ? 190 : 40, Math.floor((K.h - it) / 2));
                    bt.x = Math.max(ft, Math.min(ht, re)), bt.y = Math.max(Ft, Math.min(St, le)), bt.__trueosInitialPositionSeeded = !0;
                }
                bt.x = Math.max(ft, Math.min(ht, bt.x)), bt.y = Math.max(Ft, Math.min(St, bt.y)), te($, bt.x, bt.y);
            }
            var W = q + $.position.x, F = rt + $.position.y;
            if (f.kind === "block") {
                B("render:draw:".concat(et, ":block:").concat(f.tagName, ":begin"));
                var bt = H;
                (f.tagName === "h1" || f.tagName === "h2" || f.tagName === "h3" || f.tagName === "summary" || f.tagName === "th") && (bt = { bold: !0 }), B("render:draw:".concat(et, ":graphics"));
                var ct = $t($, "__g");
                B("render:draw:".concat(et, ":graphics-clear")), vt(ct), B("render:draw:".concat(et, ":graphics-ensure")), Ni($, ct, 0), ct.zIndex = -10;
                var it = Math.max(0, f.width), ft = Math.max(0, f.height), Ft = null;
                if ((f.tagName === "h1" || f.tagName === "h2" || f.tagName === "h3") && (te($, Math.round(f.x), Math.round(f.y)), it = Math.round(it), ft = Math.round(ft)), B("render:draw:".concat(et, ":widget:").concat(f.tagName)), f.tagName === "hr")
                    xr({ graphics: ct, w: it, theme: r });
                else if (f.tagName !== "barrow") {
                    if (f.tagName !== "searchrow") {
                        if (f.tagName === "searchbutton")
                            ri({ node: f, container: $, graphics: ct, w: it, h: ft, theme: r, uiState: l, getPointerId: Bt, focusInputKey: (dt = f.attrs) == null ? void 0 : dt["data-focus-key"], requestPaint: xt });
                        else if (f.tagName === "progress" || f.tagName === "meter")
                            dr({ node: f, graphics: ct, w: it, h: ft, theme: r });
                        else if (f.tagName === "sliderlabel")
                            pr({ node: f, container: $, theme: r, sliderStates: l.sliders });
                        else if (f.tagName === "slider")
                            an({ node: f, container: $, graphics: ct, w: it, h: ft, absX: W, absY: F, theme: r, sliderStates: l.sliders, sliderBounds: l.sliderBounds, sliderDrags: l.sliderDrags, requestPaint: xt, getPointerId: Bt });
                        else if (f.tagName === "timeinput" || f.tagName === "dateinput" || f.tagName === "monthinput" || f.tagName === "weekinput" || f.tagName === "datetimelocalinput")
                            pi({ node: f, container: $, graphics: ct, w: it, h: ft, absX: W, absY: F, theme: r, uiState: l, getPointerId: Bt, getCursorColor: ee, temporalStates: l.temporals, yearSliderOwners: l.temporalYearOwners, getOrInitInputValue: function (st, wt) { return Ze(st, wt); }, requestPaint: xt, requestOverlayPaint: Ut, popupSink: m });
                        else if (f.tagName === "input") {
                            var st = f.key, wt = st != null ? O(st) : null, jt = st != null && l.focusedKeyByPointer.get(l.keyboardOwnerPointerId) === st, Ct = st == null ? null : jt ? l.keyboardOwnerPointerId : u.has(st) ? S(st) : null, _t = Ct != null, nt = wt != null ? ee(wt) : null;
                            Qr({ node: f, container: $, graphics: ct, w: it, h: ft, absX: W, absY: F, theme: r, textMeasure: _, uiState: l, getOrInitInputState: Ze, clamp: p, radioGroups: d, textDrags: l.textDrags, requestPaint: xt, showCaret: _t, caretPointerId: Ct, focusColor: nt != null ? nt : void 0, getCursorColor: ee, getPointerId: Bt });
                        }
                        else if (f.tagName === "textarea") {
                            var st = f.key, wt = st != null ? O(st) : null, jt = st != null && l.focusedKeyByPointer.get(l.keyboardOwnerPointerId) === st, Ct = st == null ? null : jt ? l.keyboardOwnerPointerId : u.has(st) ? S(st) : null, _t = Ct != null, nt = wt != null ? ee(wt) : null;
                            qr({ node: f, container: $, graphics: ct, w: it, h: ft, absX: W, absY: F, theme: r, textMeasure: _, uiState: l, getOrInitInputState: Ze, clamp: p, textDrags: l.textDrags, requestPaint: xt, showCaret: _t, caretPointerId: Ct, focusColor: nt != null ? nt : void 0, getCursorColor: ee, getPointerId: Bt });
                        }
                        else if (f.tagName === "select") {
                            if (f.key) {
                                var st = Number((gt = (lt = f.attrs) == null ? void 0 : lt["data-selected-index"]) != null ? gt : "0");
                                Ee(l.selects, f.key, Number.isFinite(st) ? st : 0);
                            }
                            hn({ node: f, container: $, graphics: ct, w: it, h: ft, absX: W, absY: F, theme: r, selectStates: l.selects, uiState: l, getPointerId: Bt, getCursorColor: ee, requestPaint: xt, requestOverlayPaint: Ut, popupSink: h });
                        }
                        else if (f.tagName === "summary")
                            f.key && l.hoverRects.push({ key: f.key, kind: "summary", cursor: "pointer", x: W, y: F, w: it, h: ft }), gr({ node: f, container: $, w: it, h: ft, theme: r, detailsOpen: l.detailsOpen, requestRerender: yn });
                        else if (f.tagName === "dialog")
                            oi({ node: f, container: $, w: it, h: ft, theme: r, selectedBy: l.dialogSelectedBy, getCursorColor: ee, dialogStates: l.dialogs, dialogDrags: l.dialogDrags, bringToFront: function (st) { l.dialogZ.set(st, l.dialogZCounter++); }, requestPaint: xt, getPointerId: Bt });
                        else if (f.tagName === "img")
                            Gr({ node: f, container: $, graphics: ct, w: it, h: ft, theme: r, requestRerender: yn });
                        else if (f.tagName === "svg") {
                            var st = (x = (g = f.attrs) == null ? void 0 : g["data-svg"]) != null ? x : "";
                            Yr({ svgMarkup: st, container: $, w: it, h: ft, requestRerender: yn });
                        }
                        else if (f.tagName === "canvas")
                            zr({ node: f, container: $, graphics: ct, w: it, h: ft, theme: r });
                        else if (f.tagName === "iframe")
                            Vr({ node: f, container: $, graphics: ct, w: it, h: ft, theme: r });
                        else if (f.tagName === "color")
                            l.color.bounds = { x: W, y: F, w: Math.max(0, it), h: Math.max(0, ft) }, di({ node: f, container: $, graphics: ct, w: it, h: ft, theme: r, rgb: l.color.rgb, setRgb: function (st) { l.color.rgb = st; }, alpha: l.color.a, setAlpha: function (st) { l.color.a = Math.max(0, Math.min(255, Math.round(st))); }, pick: l.color.pick, setPick: function (st) { l.color.pick = st; }, requestPaint: xt, getPointerId: Bt, setDraggingPointerId: function (st) { l.color.draggingPointerId = st; } });
                        else if (f.tagName === "number") {
                            var st_1 = f.key, wt_1 = String((I = (L = f.attrs) == null ? void 0 : L.channel) != null ? I : "").toLowerCase(), jt_1 = wt_1 === "r" || wt_1 === "g" || wt_1 === "b" || wt_1 === "a";
                            st_1 && ai({ node: f, container: $, graphics: ct, w: it, h: ft, theme: r, getValue: function () { var Ct, _t; return jt_1 ? wt_1 === "a" ? (Ct = l.color.a) != null ? Ct : 255 : (_t = l.color.rgb[wt_1]) != null ? _t : 0 : vn(l.numbers, st_1, f.attrs).value; }, setValue: function (Ct) { jt_1 ? wt_1 === "a" ? l.color.a = Math.max(0, Math.min(255, Math.round(Ct))) : l.color.rgb[wt_1] = Math.max(0, Math.min(255, Math.round(Ct))) : vn(l.numbers, st_1, f.attrs).value = Ct; }, requestPaint: xt, numberHolds: l.numberHolds, getPointerId: Bt });
                        }
                        else if (f.tagName === "button") {
                            var st = ge(Sn(f));
                            f.key && l.hoverRects.push({ key: f.key, kind: "button", cursor: "pointer", x: W, y: F, w: it, h: ft }), Tr({ container: $, graphics: ct, w: it, h: ft, label: st, theme: r, registerHoverHandlers: f.key ? function (wt) { l.hoverHandlers.set(f.key, wt); } : void 0 });
                        }
                        else if (!Ln(f.tagName))
                            if (f.tagName === "table")
                                Ir({ graphics: ct, w: it, h: ft, boxBorder: r.boxBorder });
                            else if (f.tagName === "td" || f.tagName === "th")
                                Mr({ nodeTag: f.tagName, graphics: ct, w: it, h: ft, theme: r });
                            else {
                                var st = Math.max(0, Math.round(it)), wt = Math.max(0, Math.round(ft));
                                ct.rect(0, 0, st, wt), ct.stroke({ width: 1, color: r.boxBorder, alignment: 0 });
                            }
                    }
                }
                B("render:draw:".concat(et, ":overlay-label")), Ft && $.addChild(Ft);
                var ht = null, St = null, zt = f.tagName === "iframe" && String((R = (E = f.attrs) == null ? void 0 : E["data-root"]) != null ? R : "") === "1";
                if (f.tagName === "iframe" && !zt) {
                    f.key && l.iframeRects.push({ key: f.key, x: W, y: F, w: Math.max(0, it), h: Math.max(0, ft) }), ht = ue($, "__iframeContentRoot"), te(ht, 0, 0);
                    var Ct = $t($, "__iframeContentMask");
                    vt(Ct);
                    var _t = 0, nt = 34, Et = Math.max(0, it), Gt = Math.max(0, ft - 34);
                    Ct.rect(_t, nt, Et, Gt), Ct.fill(16777215), Ct.alpha = 0, ht.mask = Ct;
                    var Nt_1 = (M = f.key) != null ? M : "", mt_1 = (N = l.iframeScroll.get(Nt_1)) != null ? N : { y: 0, contentHeight: 0, viewportHeight: 0, draggingPointerId: null, dragOffsetY: 0, track: { x: 0, y: 0, w: Oe, h: 0 }, thumb: { x: 0, y: 0, w: Oe, h: 0 }, rect: { x: W, y: F, w: Math.max(0, it), h: Math.max(0, ft) } };
                    mt_1.rect = { x: W, y: F, w: Math.max(0, it), h: Math.max(0, ft) }, mt_1.contentHeight = w(f), mt_1.viewportHeight = Math.max(0, ft - 34 - 8);
                    var Xt_1 = Math.max(0, mt_1.contentHeight - mt_1.viewportHeight);
                    mt_1.y = Math.max(0, Math.min(mt_1.y, Xt_1)), St = ue(ht, "__iframeScrollRoot"), te(St, 0, -mt_1.y), Nt_1 && l.iframeScrollRoots.set(Nt_1, St);
                    var Yt = $t($, "__iframeScrollbar");
                    Nt_1 && l.iframeScrollbarGraphics.set(Nt_1, Yt), vt(Yt), Yt.eventMode = "static";
                    var xe = xn, Ie = Oe, tn = Math.max(0, it - Ie - xe), Pn = 34 + xe, $e = Math.max(0, ft - 34 - xe * 2), Jn = Xt_1 > .5 && $e > 1;
                    if (Yt.visible = Jn, Jn) {
                        var kn = Math.max(24, (mt_1.viewportHeight || 1) / Math.max(1, mt_1.contentHeight) * $e), zi = Math.max(1, $e - kn), ji = Xt_1 <= 0 ? 0 : mt_1.y / Xt_1, Qn = Pn + zi * ji;
                        mt_1.track = { x: W + tn, y: F + Pn, w: Ie, h: $e }, mt_1.thumb = { x: W + tn, y: F + Qn, w: Ie, h: kn }, Yt.rect(tn, Pn, Ie, $e), Yt.fill({ color: 0, alpha: .06 }), Yt.rect(tn, Qn, Ie, kn), Yt.fill({ color: 0, alpha: .25 }), Yt.on("pointerdown", function (oe) { var tr, er, nr, rr, ir, or; if ((oe == null ? void 0 : oe.button) === 2)
                            return; var Rn = Bt(oe); if (Rn <= 0)
                            return; var en = (er = (tr = oe.global) == null ? void 0 : tr.x) != null ? er : 0, Me = (rr = (nr = oe.global) == null ? void 0 : nr.y) != null ? rr : 0; if (!(en >= mt_1.track.x && en <= mt_1.track.x + mt_1.track.w && Me >= mt_1.track.y && Me <= mt_1.track.y + mt_1.track.h))
                            return; if (en >= mt_1.thumb.x && en <= mt_1.thumb.x + mt_1.thumb.w && Me >= mt_1.thumb.y && Me <= mt_1.thumb.y + mt_1.thumb.h) {
                            mt_1.draggingPointerId = Rn, mt_1.dragOffsetY = Me - mt_1.thumb.y, l.iframeScroll.set(Nt_1, mt_1), (ir = oe.stopPropagation) == null || ir.call(oe);
                            return;
                        } var Zn = Math.max(1, mt_1.track.h - mt_1.thumb.h), qn = Math.max(mt_1.track.y, Math.min(mt_1.track.y + Zn, Me - mt_1.thumb.h / 2)), Vi = (qn - mt_1.track.y) / Zn; mt_1.y = Math.max(0, Math.min(Xt_1, Vi * Xt_1)), mt_1.draggingPointerId = Rn, mt_1.dragOffsetY = Me - qn, l.iframeScroll.set(Nt_1, mt_1), Dt() && (window.__TRUEOS_PIXI_SCROLL_REPAINT_REQUIRED__ = !0, window.__TRUEOS_PIXI_SCROLL_REPAINT_OWNER__ = "iframe", window.__TRUEOS_PIXI_SCROLL_REPAINT_IFRAME_KEY__ = Nt_1), xt == null || xt(), (or = oe.stopPropagation) == null || or.call(oe); });
                    }
                    else
                        mt_1.track = { x: 0, y: 0, w: Ie, h: 0 }, mt_1.thumb = { x: 0, y: 0, w: Ie, h: 0 };
                    l.iframeScroll.set(Nt_1, mt_1);
                }
                var re = [], le = f.tagName === "dialog" || f.tagName === "iframe" && !zt ? re : ot, ce = K;
                if (f.tagName === "dialog")
                    ce = { x: 0, y: 0, w: Math.max(0, it), h: Math.max(0, ft) };
                else if (f.tagName === "iframe" && !zt) {
                    var st = (J = f.key) != null ? J : "", wt = l.iframeScroll.get(st), jt = wt ? wt.y : 0, Ct = 34;
                    ce = { x: 0, y: Ct + jt, w: Math.max(0, it), h: Math.max(0, ft - Ct) };
                }
                var be = (Q = St != null ? St : ht) != null ? Q : tt, _e = W + ((z = ht == null ? void 0 : ht.position.x) != null ? z : 0), ye = F + ((at = ht == null ? void 0 : ht.position.y) != null ? at : 0) + ((Z = St == null ? void 0 : St.position.y) != null ? Z : 0);
                B("render:draw:".concat(et, ":children"));
                var me = 0;
                for (var st = 0; st < ((yt = f.children) != null ? yt : []).length; st++) {
                    var wt = ((kt = f.children) != null ? kt : [])[st];
                    if (wt.kind === "block" && wt.tagName === "dialog")
                        le.push(wt);
                    else {
                        if (f.tagName === "button" && wt.kind === "text")
                            continue;
                        D(wt, be, bt, _e, ye, le, ce, "".concat(et, ".").concat(st), me++);
                    }
                }
                if ((f.tagName === "dialog" || f.tagName === "iframe" && !zt) && re.length > 0) {
                    re.sort(function (st, wt) { var _t, nt; var jt = st.key && (_t = l.dialogZ.get(st.key)) != null ? _t : 0, Ct = wt.key && (nt = l.dialogZ.get(wt.key)) != null ? nt : 0; return jt - Ct; });
                    try {
                        for (var re_1 = __values(re), re_1_1 = re_1.next(); !re_1_1.done; re_1_1 = re_1.next()) {
                            var st = re_1_1.value;
                            var wt = st.key && st.key.length > 0 ? st.key : "".concat(et, ".dlg.").concat(me);
                            D(st, be, bt, _e, ye, re, ce, "".concat(et, ".dlg.").concat(wt), me++);
                        }
                    }
                    catch (e_60_1) { e_60 = { error: e_60_1 }; }
                    finally {
                        try {
                            if (re_1_1 && !re_1_1.done && (_a = re_1.return)) _a.call(re_1);
                        }
                        finally { if (e_60) throw e_60.error; }
                    }
                }
            }
            else {
                B("render:draw:".concat(et, ":text:begin"));
                var bt = Ht($, "__text", function (ct) { ct.style = { fontFamily: r.fontFamily, fontSize: r.fontSize, fill: r.text, fontWeight: H.bold ? "700" : "400", wordWrap: !0, wordWrapWidth: 0 }; });
                bt.text = (Tt = f.text) != null ? Tt : "", bt.style.fontFamily = r.fontFamily, bt.style.fontSize = r.fontSize, bt.style.fill = r.text, bt.style.fontWeight = H.bold ? "700" : "400", bt.style.wordWrap = !0, bt.style.wordWrapWidth = Math.max(0, Math.ceil(f.width) + Ae), te(bt, 0, Mt), B("render:draw:".concat(et, ":text:done"));
            }
        }
        B("render:root-loop");
        var U = { bold: !1 }, T = { x: 0, y: 0, w: t.renderer.width, h: t.renderer.height }, A = [], G = s.position, k = G && Number(G.y || 0) || 0, P = 0;
        for (var f = 0; f < e.children.length; f++) {
            B("render:root-loop:".concat(f));
            var v = e.children[f];
            v && (v.kind === "block" && v.tagName === "dialog" ? A.push(v) : (B("render:root-loop:".concat(f, ":dispatch")), D(v, s, U, 0, k, A, T, "root.".concat(f), P++)));
        }
        if (B("render:root-dialogs"), A.length > 0) {
            A.sort(function (v, H) { var ot, K; var q = v.key && (ot = l.dialogZ.get(v.key)) != null ? ot : 0, rt = H.key && (K = l.dialogZ.get(H.key)) != null ? K : 0; return q - rt; });
            var f = 0;
            try {
                for (var A_1 = __values(A), A_1_1 = A_1.next(); !A_1_1.done; A_1_1 = A_1.next()) {
                    var v = A_1_1.value;
                    var H = v.key && v.key.length > 0 ? v.key : "rootdlg.".concat(f);
                    D(v, c, U, 0, 0, A, T, "dlg.".concat(H), f++);
                }
            }
            catch (e_52_1) { e_52 = { error: e_52_1 }; }
            finally {
                try {
                    if (A_1_1 && !A_1_1.done && (_b = A_1.return)) _b.call(A_1);
                }
                finally { if (e_52) throw e_52.error; }
            }
        }
        if (B("render:temporal-popups"), m.length > 0 && Fn({ popups: m, stage: a, theme: r, viewportW: t.renderer.width, viewportH: t.renderer.height, temporalStates: l.temporals, getOrInitInputValue: function (f, v) { return Ze(f, v); }, sliders: l.sliders, sliderBounds: l.sliderBounds, sliderDrags: l.sliderDrags, selects: l.selects, selectPopups: h, uiFocus: l, getPointerId: Bt, getCursorColor: ee, requestPaint: xt, requestOverlayPaint: Ut }), B("render:select-popups"), h.length > 0)
            try {
                for (var h_2 = __values(h), h_2_1 = h_2.next(); !h_2_1.done; h_2_1 = h_2.next()) {
                    var f = h_2_1.value;
                    Wn({ popup: f, stage: a, theme: r, selectStates: l.selects, uiState: l, getPointerId: Bt, requestPaint: xt, viewportW: t.renderer.width, viewportH: t.renderer.height });
                }
            }
            catch (e_53_1) { e_53 = { error: e_53_1 }; }
            finally {
                try {
                    if (h_2_1 && !h_2_1.done && (_c = h_2.return)) _c.call(h_2);
                }
                finally { if (e_53) throw e_53.error; }
            }
        B("render:context-menus");
        var _loop_3 = function (f, v) {
            if (!(v != null && v.open))
                return "continue";
            var H = new Ot;
            H.eventMode = "static", H.cursor = "default", te(H, v.x, v.y);
            var q = 140, rt = 28, ot = 6, K = ["Copy", "Paste", "Close"], et = new Pt;
            et.rect(0, 0, q + ot * 2, K.length * rt + ot * 2), et.fill(16777215);
            var C = 1;
            et.rect(C, C, q + ot * 2 - C * 2, K.length * rt + ot * 2 - C * 2), et.stroke({ width: 2, color: ee(f), alignment: 0 }), H.addChild(et), K.forEach(function (V, Y) { var $ = ot + Y * rt, tt = new Ot; tt.eventMode = "static", tt.cursor = "pointer", te(tt, ot, $); var W = new Pt; W.rect(0, 0, q, rt), W.fill(16777215), tt.addChild(W); var F = Zt({ text: V, fontFamily: r.fontFamily, fontSize: r.fontSize, fill: r.text }); te(F, 8, Math.max(0, (rt - F.height) / 2) + Mt), tt.addChild(F); var ut = function (lt) { return Bt(lt) === f; }, dt = function (lt) { if (!Dt())
                return; var gt = window.__pixiCapture, g = gt && typeof gt.objectId == "function" ? gt.objectId.bind(gt) : null; if (!g)
                return; var x = typeof W.getGlobalPosition == "function" ? W.getGlobalPosition() : { x: 0, y: 0 }; window.__TRUEOS_PIXI_LAST_GRAPHICS_FAST_PATH__ = { owner: "context-menu-hover", rootNode: g(t.stage), graphicsNode: g(W), x: 0, y: 0, w: q, h: rt, worldX: Number(x == null ? void 0 : x.x) || 0, worldY: Number(x == null ? void 0 : x.y) || 0, fillColor: lt, fillAlpha: 1 }; }; tt.on("pointerover", function (lt) { ut(lt) && (W.clear(), W.rect(0, 0, q, rt), W.fill(15921906), dt(15921906)); }), tt.on("pointerout", function (lt) { ut(lt) && (W.clear(), W.rect(0, 0, q, rt), W.fill(16777215), dt(16777215)); }), tt.on("pointerdown", function (lt) { var I, E, R, M, N, J, Q, z, at, Z, yt; if (!ut(lt))
                return; (I = lt.stopPropagation) == null || I.call(lt); var gt = (E = l.focusedKeyByPointer.get(f)) != null ? E : null, g = gt ? l.inputs.get(gt) : null, x = gt != null && l.fieldBounds.has(gt) && g != null && typeof g.value == "string"; if (V === "Copy" && x) {
                var kt = g, Tt = (R = kt.value) != null ? R : "", bt = (N = (M = kt.selections) == null ? void 0 : M.get(f)) != null ? N : null, ct = bt ? Math.max(0, Math.min(Tt.length, (J = bt.start) != null ? J : 0)) : 0, it = bt ? Math.max(0, Math.min(Tt.length, (Q = bt.end) != null ? Q : ct)) : ct, ft = Math.min(ct, it), Ft = Math.max(ct, it), ht = ft !== Ft ? Tt.slice(ft, Ft) : Tt;
                l.clipboards.set(f, ht);
            }
            else if (V === "Paste" && x) {
                var kt = (z = l.clipboards.get(f)) != null ? z : "";
                if (kt.length > 0) {
                    var Tt = g, bt = (at = Tt.value) != null ? at : "";
                    if (Tt.selections || (Tt.selections = new Map), !Tt.selections.has(f)) {
                        var zt = bt.length;
                        Tt.selections.set(f, { start: zt, end: zt });
                    }
                    var ct = Tt.selections.get(f), it = Math.max(0, Math.min(bt.length, (Z = ct.start) != null ? Z : bt.length)), ft = Math.max(0, Math.min(bt.length, (yt = ct.end) != null ? yt : it)), Ft = Math.min(it, ft), ht = Math.max(it, ft);
                    Tt.value = bt.slice(0, Ft) + kt + bt.slice(ht);
                    var St = Ft + kt.length;
                    ct.start = St, ct.end = St;
                }
            } var L = l.contextMenus.get(f); L && (L.open = !1, l.contextMenus.set(f, L)), xt == null || xt(); }), H.addChild(tt); }), a.addChild(H);
        };
        try {
            for (var _j = __values(l.contextMenus.entries()), _k = _j.next(); !_k.done; _k = _j.next()) {
                var _l = __read(_k.value, 2), f = _l[0], v = _l[1];
                _loop_3(f, v);
            }
        }
        catch (e_54_1) { e_54 = { error: e_54_1 }; }
        finally {
            try {
                if (_k && !_k.done && (_d = _j.return)) _d.call(_j);
            }
            finally { if (e_54) throw e_54.error; }
        }
        B("render:prune-cache");
        try {
            for (var _m = __values(b.entries()), _p = _m.next(); !_p.done; _p = _m.next()) {
                var _q = __read(_p.value, 2), f = _q[0], v = _q[1];
                if (!y.has(f)) {
                    try {
                        v.removeFromParent(), (j = v.destroy) == null || j.call(v, { children: !0 });
                    }
                    catch (H) { }
                    b.delete(f);
                }
            }
        }
        catch (e_55_1) { e_55 = { error: e_55_1 }; }
        finally {
            try {
                if (_p && !_p.done && (_f = _m.return)) _f.call(_m);
            }
            finally { if (e_55) throw e_55.error; }
        }
        B("render:done");
    }
    function Ls() {
        return nn(this, null, function () {
            var t, e, n, r, _a, i_1, _b, o_3, s, c, a_2, h, m_2, d_1, b_1, y_1, u_1, _1, p_1, S, O, _c, D, U_1, T_2, A, G_1, k_1, P_1, X_1, j_1, f_1, v_2, H_2, q_3, rt_1, ot_2, K_3, et_1, C_1, V_4, Y_1, $_2, tt_2, W_2, F, ut, dt_2, lt_1, g, x, L, gt_1, n_3, r;
            return __generator(this, function (_d) {
                switch (_d.label) {
                    case 0:
                        _d.trys.push([0, 9, , 10]);
                        At("main:start");
                        n = (t = document.getElementById("app")) != null ? t : document.body, r = !0;
                        At("main:yoga");
                        if (!r) return [3 /*break*/, 1];
                        _a = Ms();
                        return [3 /*break*/, 3];
                    case 1: return [4 /*yield*/, Promise.resolve().then(function () { return (Si(), Mi); })];
                    case 2:
                        _a = (_d.sent()).default;
                        _d.label = 3;
                    case 3:
                        pt = _a, At("main:create-app");
                        i_1 = r ? Is() : new sn;
                        _b = r;
                        if (_b) return [3 /*break*/, 5];
                        return [4 /*yield*/, i_1.init({ background: "#ffffff", resizeTo: window, antialias: !1, preference: "webgl" })];
                    case 4:
                        _b = (_d.sent());
                        _d.label = 5;
                    case 5:
                        _b, At("main:attach-capture"), Ii(i_1), window.__TRUEOS_PIXI_APP = i_1, At("main:append-canvas"), n.appendChild(i_1.canvas), i_1.ticker.stop(), At("main:capture-flags"), r && (l.harness.enabled = !1, l.virtualCursor.enabled = !1, window.__pixiCapture && (window.__pixiCapture.persist = !1)), At("main:canvas-listeners"), i_1.canvas.addEventListener("contextmenu", function (g) { return g.preventDefault(); }), i_1.canvas.addEventListener("wheel", function (g) { var J, Q; var x = (J = g.offsetX) != null ? J : 0, L = (Q = g.offsetY) != null ? Q : 0, I = function (z) { var yt; if (!Dt())
                            return; var at = window, Z = Number((yt = at.__TRUEOS_WHEEL_ROUTE_LOG_COUNT__) != null ? yt : 0) || 0; Z >= 32 || (at.__TRUEOS_WHEEL_ROUTE_LOG_COUNT__ = Z + 1, console.log("[trueos pixi widgets] wheel-route ".concat(z))); }, E = null; for (var z = l.iframeRects.length - 1; z >= 0; z--) {
                            var at = l.iframeRects[z];
                            if (x >= at.x && x <= at.x + at.w && L >= at.y && L <= at.y + at.h) {
                                E = at.key;
                                break;
                            }
                        } var R = !1; if (E) {
                            var z = l.iframeScroll.get(E);
                            if (z) {
                                var at = Math.max(0, z.contentHeight - z.viewportHeight);
                                if (I("hit=iframe x=".concat(Math.round(x), " y=").concat(Math.round(L), " delta=").concat(Math.round(g.deltaY), " y0=").concat(Math.round(z.y), " max=").concat(Math.round(at))), at > 0) {
                                    var Z = Math.max(0, Math.min(at, z.y + g.deltaY));
                                    Z !== z.y && (z.y = Z, l.iframeScroll.set(E, z), Dt() && (window.__TRUEOS_PIXI_SCROLL_REPAINT_REQUIRED__ = !0, window.__TRUEOS_PIXI_SCROLL_REPAINT_OWNER__ = "iframe", window.__TRUEOS_PIXI_SCROLL_REPAINT_IFRAME_KEY__ = E), xt == null || xt(), g.preventDefault(), R = !0, I("owner=iframe y1=".concat(Math.round(Z), " repaint=1")));
                                }
                            }
                        } if (R)
                            return; var M = Math.max(0, l.scroll.contentHeight - l.scroll.viewportHeight); if (M <= 0) {
                            I("owner=none x=".concat(Math.round(x), " y=").concat(Math.round(L), " delta=").concat(Math.round(g.deltaY), " root_y=").concat(Math.round(l.scroll.y), " root_max=0"));
                            return;
                        } var N = Math.max(0, Math.min(M, l.scroll.y + g.deltaY)); if (N !== l.scroll.y) {
                            var z = l.scroll.y;
                            l.scroll.y = N, Dt() && (window.__TRUEOS_PIXI_SCROLL_REPAINT_REQUIRED__ = !0, window.__TRUEOS_PIXI_SCROLL_REPAINT_OWNER__ = "root", window.__TRUEOS_PIXI_SCROLL_REPAINT_IFRAME_KEY__ = ""), xt == null || xt(), g.preventDefault(), I("owner=root x=".concat(Math.round(x), " y=").concat(Math.round(L), " delta=").concat(Math.round(g.deltaY), " y0=").concat(Math.round(z), " y1=").concat(Math.round(N), " max=").concat(Math.round(M), " repaint=1"));
                        }
                        else
                            I("owner=root-boundary x=".concat(Math.round(x), " y=").concat(Math.round(L), " delta=").concat(Math.round(g.deltaY), " y0=").concat(Math.round(l.scroll.y), " max=").concat(Math.round(M))); }, { passive: !1 }), At("main:stage:eventMode"), i_1.stage.eventMode = "static", At("main:stage:hitArea"), i_1.stage.hitArea = i_1.screen, At("main:stage:on:pointerdown"), i_1.stage.on("pointerdown", function (g) {
                            var e_61, _a;
                            var x, L, I, E, R, M;
                            if ((g == null ? void 0 : g.button) === 2) {
                                var N = Bt(g);
                                if (N > 0) {
                                    var J = (x = l.contextMenus.get(N)) != null ? x : { open: !1, x: 0, y: 0 };
                                    J.open = !0, J.x = (I = (L = g.global) == null ? void 0 : L.x) != null ? I : 0, J.y = (R = (E = g.global) == null ? void 0 : E.y) != null ? R : 0, l.contextMenus.set(N, J);
                                }
                                Ut == null || Ut(), (M = g.preventDefault) == null || M.call(g);
                                return;
                            }
                            if ((g == null ? void 0 : g.button) !== 2) {
                                var N = Bt(g), J = N > 0 ? l.contextMenus.get(N) : null;
                                J && J.open && (J.open = !1, l.contextMenus.set(N, J), Ut == null || Ut());
                            }
                            if ((g == null ? void 0 : g.button) !== 2) {
                                var N = !1;
                                try {
                                    for (var _b = __values(l.selects.values()), _c = _b.next(); !_c.done; _c = _b.next()) {
                                        var J = _c.value;
                                        J.open && (J.open = !1, N = !0);
                                    }
                                }
                                catch (e_61_1) { e_61 = { error: e_61_1 }; }
                                finally {
                                    try {
                                        if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
                                    }
                                    finally { if (e_61) throw e_61.error; }
                                }
                                N && (Ut == null || Ut());
                            }
                            (g == null ? void 0 : g.button) !== 2 && gi(l.temporals) && (Ut == null || Ut()), W_2();
                        }), At("main:stage:done"), At("main:roots");
                        o_3 = new Ot, s = new Ot;
                        s.eventMode = "static";
                        c = new Ot;
                        c.eventMode = "none", i_1.stage.addChild(o_3), i_1.stage.addChild(s), i_1.stage.addChild(c);
                        a_2 = new Pt;
                        a_2.label = "__trueosGlobalScrollbar", a_2.eventMode = "static", s.addChild(a_2);
                        h = function (g, x) { g.clear(); var L = x.half, I = x.strokeWidth, E = x.color; g.moveTo(-L, 0), g.lineTo(L, 0), g.stroke({ width: I, color: E }), g.moveTo(0, -L), g.lineTo(0, L), g.stroke({ width: I, color: E }); }, m_2 = new Pt;
                        m_2.eventMode = "none", m_2.visible = !1, c.addChild(m_2);
                        d_1 = new Pt;
                        d_1.eventMode = "none", d_1.visible = !1, c.addChild(d_1);
                        b_1 = new Pt;
                        b_1.eventMode = "none", b_1.visible = !1, c.addChild(b_1);
                        y_1 = new Pt;
                        y_1.eventMode = "none", c.addChild(y_1), At("main:text-measure");
                        u_1 = document.createElement("canvas").getContext("2d");
                        if (!u_1)
                            throw new Error("2D canvas not available");
                        u_1.font = "".concat(ne.fontSize, "px ").concat(ne.fontFamily);
                        _1 = function (g) { return u_1.measureText(g).width; }, p_1 = ne.fontSize * 1.25;
                        At("main:html");
                        if (!(typeof window.__TRUEOS_INPUT_HTML__ == "string")) return [3 /*break*/, 6];
                        _c = window.__TRUEOS_INPUT_HTML__;
                        return [3 /*break*/, 8];
                    case 6: return [4 /*yield*/, fetch("/input.html").then(function (g) { return g.text(); })];
                    case 7:
                        _c = _d.sent();
                        _d.label = 8;
                    case 8:
                        S = _c, O = os(S) ? S : "";
                        Dt() && console.log("[trueos pixi widgets] input-html chars=".concat(S.length, " usable=").concat(O ? 1 : 0, " sample=\"").concat(wn(S), "\"")), At("main:render-tree"), Di.clear();
                        D = fs(O), U_1 = ps(), T_2 = Rs(U_1.tree, D.rows), A = Os(T_2, D.rows);
                        if (Dt() && (console.log("[trueos pixi widgets] text-fallback source=".concat(D.source, " rows=").concat(D.rows.length, " samples=").concat(ls(D.rows))), console.log("[trueos pixi widgets] render-tree source=".concat(U_1.source, " nodes=").concat(T_2.length, " trusted_text_applied=").concat(A))), T_2.length === 0)
                            throw new Error("TrueSurfer widget render tree is missing");
                        G_1 = rs(T_2), k_1 = null, P_1 = { nodes: 0, blocks: 0, text: 0, maxDepth: 0, tags: {} }, X_1 = { hash: "", renderHash: "", layoutHash: "", bytes: 0 }, j_1 = 0, f_1 = null, v_2 = function () { var g = Math.max(0, l.scroll.contentHeight - l.scroll.viewportHeight); l.scroll.y = Math.max(0, Math.min(l.scroll.y, g)); }, H_2 = function () { var g = i_1.renderer.width, x = i_1.renderer.height; l.scroll.viewportHeight = x; var L = l.scroll.contentHeight, I = Math.max(0, L - x), E = I > .5; if (a_2.clear(), a_2.visible = E, !E) {
                            l.scroll.track = { x: 0, y: 0, w: l.scroll.track.w, h: 0 }, l.scroll.thumb = { x: 0, y: 0, w: l.scroll.thumb.w, h: 0 };
                            return;
                        } var R = xn, M = Oe, N = Math.max(0, g - M - R), J = R, Q = Math.max(0, x - R * 2), at = Math.max(24, x / Math.max(x, L) * Q), Z = Math.max(1, Q - at), yt = I <= 0 ? 0 : l.scroll.y / I, kt = J + Z * yt; l.scroll.track = { x: N, y: J, w: M, h: Q }, l.scroll.thumb = { x: N, y: kt, w: M, h: at }, a_2.rect(N, J, M, Q), a_2.fill({ color: 0, alpha: .06 }), a_2.rect(N, kt, M, at), a_2.fill({ color: 0, alpha: .25 }); }, q_3 = function () {
                            var e_62, _a;
                            var x = xn, L = Oe;
                            try {
                                for (var _b = __values(l.iframeScrollRoots.entries()), _c = _b.next(); !_c.done; _c = _b.next()) {
                                    var _d = __read(_c.value, 2), I = _d[0], E = _d[1];
                                    var R = l.iframeScroll.get(I);
                                    if (!R)
                                        continue;
                                    var M = Math.max(0, R.contentHeight - R.viewportHeight);
                                    R.y = Math.max(0, Math.min(R.y, M)), te(E, 0, -R.y);
                                    var N = l.iframeScrollbarGraphics.get(I);
                                    if (!N) {
                                        l.iframeScroll.set(I, R);
                                        continue;
                                    }
                                    var J = Math.max(0, R.rect.w), Q = Math.max(0, R.rect.h), z = Math.max(0, J - L - x), at = 34 + x, Z = Math.max(0, Q - 34 - x * 2), yt = M > .5 && Z > 1;
                                    if (N.clear(), N.visible = yt, !yt) {
                                        R.track = { x: 0, y: 0, w: L, h: 0 }, R.thumb = { x: 0, y: 0, w: L, h: 0 }, l.iframeScroll.set(I, R);
                                        continue;
                                    }
                                    var Tt = Math.max(24, (R.viewportHeight || 1) / Math.max(1, R.contentHeight) * Z), bt = Math.max(1, Z - Tt), ct = M <= 0 ? 0 : R.y / M, it = at + bt * ct;
                                    R.track = { x: R.rect.x + z, y: R.rect.y + at, w: L, h: Z }, R.thumb = { x: R.rect.x + z, y: R.rect.y + it, w: L, h: Tt }, N.rect(z, at, L, Z), N.fill({ color: 0, alpha: .06 }), N.rect(z, it, L, Tt), N.fill({ color: 0, alpha: .25 }), l.iframeScroll.set(I, R);
                                }
                            }
                            catch (e_62_1) { e_62 = { error: e_62_1 }; }
                            finally {
                                try {
                                    if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
                                }
                                finally { if (e_62) throw e_62.error; }
                            }
                        }, rt_1 = function (g) {
                            var e_63, _a;
                            var R;
                            var x = [], L = [], I = -l.scroll.y, E = function (M, N, J) {
                                var e_64, _a;
                                var at;
                                var Q = N + M.x, z = J + M.y;
                                if (M.kind === "block" && M.key) {
                                    if (M.tagName === "select") {
                                        var Z = l.selects.get(M.key);
                                        if (Z != null && Z.open) {
                                            var yt = Hn(M.attrs);
                                            Z.selectedIndex = Math.max(0, Math.min(yt.length - 1, Z.selectedIndex | 0)), L.push({ key: M.key, absX: Q, absY: z, w: M.width, h: M.height, options: yt, selectedIndex: Z.selectedIndex });
                                        }
                                    }
                                    else if (M.tagName === "timeinput" || M.tagName === "dateinput" || M.tagName === "monthinput" || M.tagName === "weekinput" || M.tagName === "datetimelocalinput") {
                                        var Z = l.temporals.get(M.key);
                                        (Z == null ? void 0 : Z.openPanel) === "month" ? x.push({ kind: "month-panel", inputKey: M.key, absX: Q, absY: z, anchorW: M.width, anchorH: M.height }) : (Z == null ? void 0 : Z.openPanel) === "week" ? x.push({ kind: "week-panel", inputKey: M.key, absX: Q, absY: z, anchorW: M.width, anchorH: M.height }) : (Z == null ? void 0 : Z.openPanel) === "time" && x.push({ kind: "time-panel", inputKey: M.key, absX: Q, absY: z, anchorW: M.width, anchorH: M.height });
                                    }
                                }
                                try {
                                    for (var _b = __values((at = M.children) != null ? at : []), _c = _b.next(); !_c.done; _c = _b.next()) {
                                        var Z = _c.value;
                                        Z.kind === "block" && Z.tagName === "dialog" || E(Z, Q, z);
                                    }
                                }
                                catch (e_64_1) { e_64 = { error: e_64_1 }; }
                                finally {
                                    try {
                                        if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
                                    }
                                    finally { if (e_64) throw e_64.error; }
                                }
                            };
                            try {
                                for (var _b = __values((R = g.children) != null ? R : []), _c = _b.next(); !_c.done; _c = _b.next()) {
                                    var M = _c.value;
                                    E(M, 0, I);
                                }
                            }
                            catch (e_63_1) { e_63 = { error: e_63_1 }; }
                            finally {
                                try {
                                    if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
                                }
                                finally { if (e_63) throw e_63.error; }
                            }
                            return { temporalPopups: x, selectPopups: L };
                        }, ot_2 = function (g) {
                            var e_65, _a;
                            var _loop_4 = function (x, L) {
                                if (!(L != null && L.open))
                                    return "continue";
                                var I = new Ot;
                                I.eventMode = "static", I.cursor = "default", te(I, L.x, L.y);
                                var E = 140, R = 28, M = 6, N = ["Copy", "Paste", "Close"], J = new Pt;
                                J.rect(0, 0, E + M * 2, N.length * R + M * 2), J.fill(16777215);
                                var Q = 1;
                                J.rect(Q, Q, E + M * 2 - Q * 2, N.length * R + M * 2 - Q * 2), J.stroke({ width: 2, color: ee(x), alignment: 0 }), I.addChild(J), N.forEach(function (z, at) { var Z = M + at * R, yt = new Ot; yt.eventMode = "static", yt.cursor = "pointer", te(yt, M, Z); var kt = new Pt; kt.rect(0, 0, E, R), kt.fill(16777215), yt.addChild(kt); var Tt = Zt({ text: z, fontFamily: ne.fontFamily, fontSize: ne.fontSize, fill: ne.text }); te(Tt, 8, Math.max(0, (R - Tt.height) / 2) + Mt), yt.addChild(Tt); var bt = function (ct) { return Bt(ct) === x; }; yt.on("pointerdown", function (ct) { var zt, re, le, ce, be, _e, ye, me, st, wt, jt; if (!bt(ct))
                                    return; (zt = ct.stopPropagation) == null || zt.call(ct); var it = (re = l.focusedKeyByPointer.get(x)) != null ? re : null, ft = it ? l.inputs.get(it) : null, Ft = it != null && l.fieldBounds.has(it) && ft != null && typeof ft.value == "string", ht = !1; if (z === "Copy" && Ft) {
                                    var Ct = ft, _t = (le = Ct.value) != null ? le : "", nt = (be = (ce = Ct.selections) == null ? void 0 : ce.get(x)) != null ? be : null, Et = nt ? Math.max(0, Math.min(_t.length, (_e = nt.start) != null ? _e : 0)) : 0, Gt = nt ? Math.max(0, Math.min(_t.length, (ye = nt.end) != null ? ye : Et)) : Et;
                                    l.clipboards.set(x, _t.slice(Math.min(Et, Gt), Math.max(Et, Gt)) || _t);
                                }
                                else if (z === "Paste" && Ft) {
                                    var Ct = (me = l.clipboards.get(x)) != null ? me : "";
                                    if (Ct.length > 0) {
                                        var _t = ft, nt = (st = _t.value) != null ? st : "";
                                        if (_t.selections || (_t.selections = new Map), !_t.selections.has(x)) {
                                            var xe = nt.length;
                                            _t.selections.set(x, { start: xe, end: xe });
                                        }
                                        var Et = _t.selections.get(x), Gt = Math.max(0, Math.min(nt.length, (wt = Et.start) != null ? wt : nt.length)), Nt = Math.max(0, Math.min(nt.length, (jt = Et.end) != null ? jt : Gt)), mt = Math.min(Gt, Nt), Xt = Math.max(Gt, Nt);
                                        _t.value = nt.slice(0, mt) + Ct + nt.slice(Xt);
                                        var Yt = mt + Ct.length;
                                        Et.start = Yt, Et.end = Yt, ht = !0;
                                    }
                                } var St = l.contextMenus.get(x); St && (St.open = !1, l.contextMenus.set(x, St)), ht ? xt == null || xt() : Ut == null || Ut(); }), I.addChild(yt); }), g.addChild(I);
                            };
                            try {
                                for (var _b = __values(l.contextMenus.entries()), _c = _b.next(); !_c.done; _c = _b.next()) {
                                    var _d = __read(_c.value, 2), x = _d[0], L = _d[1];
                                    _loop_4(x, L);
                                }
                            }
                            catch (e_65_1) { e_65 = { error: e_65_1 }; }
                            finally {
                                try {
                                    if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
                                }
                                finally { if (e_65) throw e_65.error; }
                            }
                        }, K_3 = function (g, x, L) {
                            var e_66, _a;
                            if (g.removeChildren(), x.length > 0 && Fn({ popups: x, stage: g, theme: ne, viewportW: i_1.renderer.width, viewportH: i_1.renderer.height, temporalStates: l.temporals, getOrInitInputValue: function (I, E) { return Ze(I, E); }, sliders: l.sliders, sliderBounds: l.sliderBounds, sliderDrags: l.sliderDrags, selects: l.selects, selectPopups: L, uiFocus: l, getPointerId: Bt, getCursorColor: ee, requestPaint: xt, requestOverlayPaint: Ut }), L.length > 0)
                                try {
                                    for (var L_1 = __values(L), L_1_1 = L_1.next(); !L_1_1.done; L_1_1 = L_1.next()) {
                                        var I = L_1_1.value;
                                        Wn({ popup: I, stage: g, theme: ne, selectStates: l.selects, uiState: l, getPointerId: Bt, requestPaint: xt, viewportW: i_1.renderer.width, viewportH: i_1.renderer.height });
                                    }
                                }
                                catch (e_66_1) { e_66 = { error: e_66_1 }; }
                                finally {
                                    try {
                                        if (L_1_1 && !L_1_1.done && (_a = L_1.return)) _a.call(L_1);
                                    }
                                    finally { if (e_66) throw e_66.error; }
                                }
                            ot_2(g);
                        }, et_1 = function () { if (!k_1)
                            return; var g = ue(o_3, "__overlayRoot"), x = f_1, _a = rt_1(k_1), L = _a.temporalPopups, I = _a.selectPopups, E = window.__pixiCapture, R = E && Array.isArray(E.commands) ? E.commands : null, M = R ? R.length : 0; K_3(g, L, I); var N = Li(g); f_1 = N; var J = Re(x, N); if (Dt()) {
                            if (window.__TRUEOS_PIXI_LAST_OVERLAY_FAST_PATH__ = null, window.__TRUEOS_PIXI_INCREMENTAL_COMMANDS__ = [], window.__TRUEOS_PIXI_INCREMENTAL_ROOT__ = 0, window.__TRUEOS_PIXI_INCREMENTAL_DAMAGE__ = null, R && J && J.w > 0 && J.h > 0) {
                                var Q = R.slice(M);
                                R.splice(M, Q.length);
                                var z = window.__pixiCapture, at = z && typeof z.objectId == "function" ? z.objectId.bind(z) : null, Z = at ? at(i_1.stage) : 0;
                                window.__TRUEOS_PIXI_INCREMENTAL_COMMANDS__ = Q, window.__TRUEOS_PIXI_INCREMENTAL_ROOT__ = Z, window.__TRUEOS_PIXI_INCREMENTAL_DAMAGE__ = J, window.__TRUEOS_PIXI_LAST_OVERLAY_FAST_PATH__ = { rootNode: Z, damageX: J.x, damageY: J.y, damageW: J.w, damageH: J.h };
                            }
                            return;
                        } i_1.renderer.render(i_1.stage); }, C_1 = function () { if (k_1) {
                            if (he("PIXI_PAINT_STEP", 96, "[trueos pixi widgets] paint-step clamp begin"), At("main:paint:clamp"), v_2(), he("PIXI_PAINT_STEP", 96, "[trueos pixi widgets] paint-step render-to-pixi begin"), At("main:paint:render-to-pixi"), Ns(i_1, k_1, o_3), f_1 = Li(ue(o_3, "__overlayRoot")), he("PIXI_PAINT_STEP", 96, "[trueos pixi widgets] paint-step render-to-pixi done"), At("main:paint:scrollbar"), he("PIXI_PAINT_STEP", 96, "[trueos pixi widgets] paint-step scrollbar begin"), H_2(), At("main:paint:renderer-render"), he("PIXI_PAINT_STEP", 96, "[trueos pixi widgets] paint-step renderer-render begin"), i_1.renderer.render(i_1.stage), he("PIXI_PAINT_STEP", 96, "[trueos pixi widgets] paint-step publish-stats begin"), Ci(G_1, P_1, Pi(T_2), ki(k_1), Ri(k_1), X_1), he("PIXI_PAINT_STEP", 96, "[trueos pixi widgets] paint-step publish-stats done"), Dt()) {
                                he("PIXI_PAINT_STEP", 96, "[trueos pixi widgets] paint-step overlays begin");
                                var g = ys(k_1);
                                window.__TRUEOS_PIXI_LAYOUT_TEXT_OVERLAYS__ = g, j_1 < 4 && (j_1 += 1, console.log("[trueos pixi widgets] layout-text-overlays count=".concat(g.length, " samples=").concat(xs(g)))), he("PIXI_PAINT_STEP", 96, "[trueos pixi widgets] paint-step overlays done");
                            }
                            At("main:paint:done"), he("PIXI_PAINT_STEP", 96, "[trueos pixi widgets] paint-step done");
                        } }, V_4 = function () { var g = window.__TRUEOS_PIXI_SCROLL_REPAINT_OWNER__ || "", x = window.__TRUEOS_PIXI_SCROLL_REPAINT_IFRAME_KEY__ || ""; At("main:scroll-paint:clamp"), v_2(), At("main:scroll-paint:content-position"); var L = ue(o_3, "__contentRoot"); if (te(L, 0, -l.scroll.y), At("main:scroll-paint:scrollbar"), H_2(), window.__TRUEOS_PIXI_LAST_SCROLL_FAST_PATH__ = null, g === "root") {
                            var I = window.__pixiCapture, E = I && typeof I.objectId == "function" ? I.objectId.bind(I) : null;
                            E && (window.__TRUEOS_PIXI_LAST_SCROLL_FAST_PATH__ = { owner: "root", rootNode: E(i_1.stage), contentNode: E(L), contentY: -l.scroll.y, scrollbarNode: E(a_2), scrollbarVisible: l.scroll.track.h > 0 ? 1 : 0, trackX: l.scroll.track.x, trackY: l.scroll.track.y, trackW: l.scroll.track.w, trackH: l.scroll.track.h, thumbX: l.scroll.thumb.x, thumbY: l.scroll.thumb.y, thumbW: l.scroll.thumb.w, thumbH: l.scroll.thumb.h });
                        } if (At("main:scroll-paint:iframe-scrollbars"), q_3(), g === "iframe" && x) {
                            var I = window.__pixiCapture, E = I && typeof I.objectId == "function" ? I.objectId.bind(I) : null, R = l.iframeScrollRoots.get(x), M = l.iframeScrollbarGraphics.get(x), N = l.iframeScroll.get(x);
                            E && R && M && N && (window.__TRUEOS_PIXI_LAST_SCROLL_FAST_PATH__ = { owner: "iframe", rootNode: E(i_1.stage), contentNode: E(R), contentY: -N.y, scrollbarNode: E(M), scrollbarVisible: N.track.h > 0 ? 1 : 0, trackX: N.track.h > 0 ? N.track.x - N.rect.x : 0, trackY: N.track.h > 0 ? N.track.y - N.rect.y : 0, trackW: N.track.w, trackH: N.track.h, thumbX: N.thumb.h > 0 ? N.thumb.x - N.rect.x : 0, thumbY: N.thumb.h > 0 ? N.thumb.y - N.rect.y : 0, thumbW: N.thumb.w, thumbH: N.thumb.h });
                        } At("main:scroll-paint:renderer-render"), i_1.renderer.render(i_1.stage), Ci(G_1, P_1, Pi(T_2), k_1 ? ki(k_1) : "", k_1 ? Ri(k_1) : ""), At("main:scroll-paint:done"); };
                        Dt() && (window.__TRUEOS_REPAINT_NOW__ = function () { var I; var g = window.__TRUEOS_PIXI_SCROLL_REPAINT_REQUIRED__ === !0, x = !g && window.__TRUEOS_PIXI_OVERLAY_REPAINT_REQUIRED__ === !0; window.__TRUEOS_PIXI_DIRTY__ = !1, window.__TRUEOS_PIXI_REPAINT_REQUIRED__ = !1, window.__TRUEOS_PIXI_SCROLL_REPAINT_REQUIRED__ = !1, window.__TRUEOS_PIXI_OVERLAY_REPAINT_REQUIRED__ = !1, g || (window.__TRUEOS_PIXI_LAST_SCROLL_FAST_PATH__ = null), !g && !x && (window.__TRUEOS_PIXI_LAST_OVERLAY_FAST_PATH__ = null), g || (window.__TRUEOS_PIXI_LAST_GRAPHICS_FAST_PATH__ = null); var L = Number((I = window.__TRUEOS_REPAINT_NOW_LOG_COUNT__) != null ? I : 0) || 0; L < 24 && (window.__TRUEOS_REPAINT_NOW_LOG_COUNT__ = L + 1, console.log("[trueos pixi widgets] repaint-now scrollOnly=".concat(g ? 1 : 0, " overlayOnly=").concat(x ? 1 : 0, " begin"))), g ? V_4() : x ? et_1() : C_1(), window.__TRUEOS_PIXI_SCROLL_REPAINT_OWNER__ = "", window.__TRUEOS_PIXI_SCROLL_REPAINT_IFRAME_KEY__ = "", L < 24 && console.log("[trueos pixi widgets] repaint-now scrollOnly=".concat(g ? 1 : 0, " overlayOnly=").concat(x ? 1 : 0, " done")); });
                        Y_1 = function () { At("main:layout-build"), Lt("[trueos pixi widgets] rerender layout-build begin"); var g = As(T_2, window.innerWidth, window.innerHeight); Lt("[trueos pixi widgets] rerender layout-build done"), Lt("[trueos pixi widgets] rerender prepixi begin"), X_1 = Zo(U_1.source, T_2, g, window.innerWidth, window.innerHeight), Lt("[trueos pixi widgets] rerender prepixi done"), At("main:layout-commit"), k_1 = g, Dt() && (window.__TRUEOS_PIXI_LAST_LAYOUT__ = g, window.__TRUEOS_PIXI_LAYOUT_TEXT_OVERLAYS__ = []), Lt("[trueos pixi widgets] rerender stats begin"), P_1 = is(g), Lt("[trueos pixi widgets] rerender stats done"), Lt("[trueos pixi widgets] rerender scroll-height begin"), l.scroll.contentHeight = Ss(g), l.scroll.viewportHeight = window.innerHeight, Lt("[trueos pixi widgets] rerender paint begin"), C_1(), Lt("[trueos pixi widgets] rerender paint done"); };
                        yn = function () { Y_1(); };
                        $_2 = !1, tt_2 = !1, W_2 = function () { if (Dt()) {
                            window.__TRUEOS_PIXI_DIRTY__ = !0;
                            return;
                        } tt_2 || $_2 || (tt_2 = !0, requestAnimationFrame(function () { tt_2 = !1, i_1.renderer.render(i_1.stage); })); };
                        xt = function () { if (!$_2) {
                            if (Dt()) {
                                window.__TRUEOS_PIXI_DIRTY__ = !0, window.__TRUEOS_PIXI_REPAINT_REQUIRED__ = !0;
                                return;
                            }
                            $_2 = !0, requestAnimationFrame(function () { $_2 = !1, C_1(); });
                        } }, Ut = function () { if (!$_2) {
                            if (Dt()) {
                                window.__TRUEOS_PIXI_DIRTY__ = !0, window.__TRUEOS_PIXI_OVERLAY_REPAINT_REQUIRED__ = !0;
                                return;
                            }
                            $_2 = !0, requestAnimationFrame(function () { $_2 = !1, et_1(); });
                        } }, At("main:first-rerender"), Y_1(), At("main:cursor-setup");
                        F = 2, ut = 10, dt_2 = Dt();
                        h(m_2, { half: ut, strokeWidth: F, color: ee(Kt) }), h(d_1, { half: ut, strokeWidth: F, color: ee(Vt) }), h(b_1, { half: ut, strokeWidth: F, color: ee(Qt) });
                        lt_1 = 2;
                        if (h(y_1, { half: ut, strokeWidth: F, color: ee(lt_1) }), l.userCursorPos.set(Kt, { x: i_1.renderer.width * .25, y: i_1.renderer.height * .5 }), l.userCursorPos.set(Vt, { x: i_1.renderer.width * .25 + 40, y: i_1.renderer.height * .5 + 20 }), l.userCursorPos.set(Qt, { x: i_1.renderer.width * .25 + 80, y: i_1.renderer.height * .5 + 40 }), m_2.visible = !dt_2, d_1.visible = !dt_2, b_1.visible = !dt_2, !dt_2) {
                            g = l.userCursorPos.get(Kt), x = l.userCursorPos.get(Vt), L = l.userCursorPos.get(Qt);
                            m_2.position.set(g.x, g.y), d_1.position.set(x.x, x.y), b_1.position.set(L.x, L.y);
                        }
                        y_1.visible = !dt_2 && l.virtualCursor.enabled;
                        gt_1 = function () { if (dt_2) {
                            m_2.visible = !1, d_1.visible = !1, b_1.visible = !1, y_1.visible = !1;
                            return;
                        } var g = l.userCursorPos.get(Kt), x = l.userCursorPos.get(Vt), L = l.userCursorPos.get(Qt); g && (m_2.visible = !0, m_2.position.set(g.x, g.y)), x && (d_1.visible = !0, d_1.position.set(x.x, x.y)), L && (b_1.visible = !0, b_1.position.set(L.x, L.y)); var I = function (E, R) { var M = null, N = null; for (var J = l.hoverRects.length - 1; J >= 0; J--) {
                            var Q = l.hoverRects[J];
                            if (E >= Q.x && E <= Q.x + Q.w && R >= Q.y && R <= Q.y + Q.h) {
                                M = Q.key, N = Q.cursor;
                                break;
                            }
                        } return { hitKey: M, hitCursor: N }; }; if (g) {
                            var _a = I(g.x, g.y), E = _a.hitKey, R = _a.hitCursor;
                            l.hoveredKeyByPointer.set(Kt, E), l.hoveredCursorByPointer.set(Kt, R);
                            var M = l.textDrags.has(Kt) || l.sliderDrags.has(Kt) || l.dialogDrags.has(Kt);
                            m_2.rotation = R != null || M ? Math.PI / 4 : 0;
                        } if (x) {
                            var _b = I(x.x, x.y), E = _b.hitKey, R = _b.hitCursor;
                            l.hoveredKeyByPointer.set(Vt, E), l.hoveredCursorByPointer.set(Vt, R);
                            var M = l.textDrags.has(Vt) || l.sliderDrags.has(Vt) || l.dialogDrags.has(Vt);
                            d_1.rotation = R != null || M ? Math.PI / 4 : 0;
                        } if (L) {
                            var _c = I(L.x, L.y), E = _c.hitKey, R = _c.hitCursor;
                            l.hoveredKeyByPointer.set(Qt, E), l.hoveredCursorByPointer.set(Qt, R);
                            var M = l.textDrags.has(Qt) || l.sliderDrags.has(Qt) || l.dialogDrags.has(Qt);
                            b_1.rotation = R != null || M ? Math.PI / 4 : 0;
                        } W_2(); };
                        l.harness.enabled && setInterval(function () {
                            var e_67, _a, e_68, _b;
                            var g = l.harness.activeUserPointerId, x = g === Kt ? Vt : g === Vt ? Qt : Kt;
                            if (l.harness.activeUserPointerId = x, l.lastMouse.has) {
                                var Q = l.userCursorPos.get(g), z = l.userCursorPos.get(x);
                                l.userCursorPos.set(x, { x: l.lastMouse.x, y: l.lastMouse.y }), z ? l.userCursorPos.set(g, { x: z.x, y: z.y }) : Q && l.userCursorPos.set(g, { x: Q.x, y: Q.y });
                            }
                            var L = l.textDrags.size > 0, I = l.sliderDrags.size > 0, E = l.dialogDrags.size > 0, R = l.scroll.draggingPointerId != null, M = l.color.draggingPointerId != null, N = !1;
                            try {
                                for (var _c = __values(l.iframeScroll.values()), _d = _c.next(); !_d.done; _d = _c.next()) {
                                    var Q = _d.value;
                                    if (Q.draggingPointerId != null) {
                                        N = !0;
                                        break;
                                    }
                                }
                            }
                            catch (e_67_1) { e_67 = { error: e_67_1 }; }
                            finally {
                                try {
                                    if (_d && !_d.done && (_a = _c.return)) _a.call(_c);
                                }
                                finally { if (e_67) throw e_67.error; }
                            }
                            var J = L || I || E || R || M || N;
                            l.textDrags.delete(Kt), l.textDrags.delete(Vt), l.textDrags.delete(Qt), l.sliderDrags.delete(Kt), l.sliderDrags.delete(Vt), l.sliderDrags.delete(Qt), l.dialogDrags.delete(Kt), l.dialogDrags.delete(Vt), l.dialogDrags.delete(Qt);
                            try {
                                for (var _f = __values([Kt, Vt, Qt]), _g = _f.next(); !_g.done; _g = _f.next()) {
                                    var Q = _g.value;
                                    var z = l.numberHolds.get(Q);
                                    z && (z.timeoutId != null && window.clearTimeout(z.timeoutId), z.intervalId != null && window.clearInterval(z.intervalId), l.numberHolds.delete(Q));
                                }
                            }
                            catch (e_68_1) { e_68 = { error: e_68_1 }; }
                            finally {
                                try {
                                    if (_g && !_g.done && (_b = _f.return)) _b.call(_f);
                                }
                                finally { if (e_68) throw e_68.error; }
                            }
                            (l.scroll.draggingPointerId === Kt || l.scroll.draggingPointerId === Vt || l.scroll.draggingPointerId === Qt) && (l.scroll.draggingPointerId = null), (l.color.draggingPointerId === Kt || l.color.draggingPointerId === Vt || l.color.draggingPointerId === Qt) && (l.color.draggingPointerId = null), gt_1(), J && (xt == null || xt());
                        }, l.harness.periodMs), !dt_2 && l.virtualCursor.enabled && i_1.ticker.add(function () { var R, M, N, J, Q; var g = Math.max(0, i_1.ticker.deltaMS) / 1e3; y_1.visible = !0, l.virtualCursor.t += g; var x = i_1.renderer.width * .75, L = i_1.renderer.height * .25, I = l.virtualCursor.t * l.virtualCursor.speed, E = l.virtualCursor.radius; l.virtualCursor.x = x + Math.cos(I) * E, l.virtualCursor.y = L + Math.sin(I) * E, y_1.position.set(l.virtualCursor.x, l.virtualCursor.y); {
                            var z = lt_1, at = l.virtualCursor.x, Z = l.virtualCursor.y, yt = null, kt = null;
                            for (var ct = l.hoverRects.length - 1; ct >= 0; ct--) {
                                var it = l.hoverRects[ct];
                                if (at >= it.x && at <= it.x + it.w && Z >= it.y && Z <= it.y + it.h) {
                                    yt = it.key, kt = it.cursor;
                                    break;
                                }
                            }
                            var Tt = (R = l.hoveredKeyByPointer.get(z)) != null ? R : null;
                            Tt !== yt && (Tt && ((N = (M = l.hoverHandlers.get(Tt)) == null ? void 0 : M.out) == null || N.call(M)), yt && ((Q = (J = l.hoverHandlers.get(yt)) == null ? void 0 : J.over) == null || Q.call(J)), l.hoveredKeyByPointer.set(z, yt)), l.hoveredCursorByPointer.set(z, kt);
                            var bt = l.textDrags.has(z) || l.sliderDrags.has(z) || l.dialogDrags.has(z);
                            y_1.rotation = kt != null || bt ? Math.PI / 4 : 0;
                        } }), l.virtualCursor.x = i_1.renderer.width * .75 + l.virtualCursor.radius, l.virtualCursor.y = i_1.renderer.height * .25, y_1.position.set(l.virtualCursor.x, l.virtualCursor.y), Dt() && C_1(), i_1.stage.on("pointerup", function (g) {
                            var e_69, _a;
                            var I, E, R;
                            var x = Bt(g), L = (E = (I = l.sliderDrags.get(x)) == null ? void 0 : I.key) != null ? E : null;
                            l.textDrags.delete(x), l.sliderDrags.delete(x), l.dialogDrags.delete(x), l.scroll.draggingPointerId === x && (l.scroll.draggingPointerId = null), l.color.draggingPointerId === x && (l.color.draggingPointerId = null);
                            try {
                                for (var _b = __values(l.iframeScroll.values()), _c = _b.next(); !_c.done; _c = _b.next()) {
                                    var M = _c.value;
                                    M.draggingPointerId === x && (M.draggingPointerId = null);
                                }
                            }
                            catch (e_69_1) { e_69 = { error: e_69_1 }; }
                            finally {
                                try {
                                    if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
                                }
                                finally { if (e_69) throw e_69.error; }
                            }
                            {
                                var M = l.numberHolds.get(x);
                                M && (M.timeoutId != null && window.clearTimeout(M.timeoutId), M.intervalId != null && window.clearInterval(M.intervalId), l.numberHolds.delete(x));
                            }
                            if (L) {
                                var M = (R = l.temporalYearOwners.get(L)) != null ? R : null;
                                if (M) {
                                    var N = l.temporals.get(M);
                                    N && N.openYear && (N.openYear = !1, l.temporals.set(M, N), xt == null || xt());
                                }
                            }
                            W_2();
                        }), i_1.stage.on("pointerupoutside", function (g) {
                            var e_70, _a;
                            var I, E, R;
                            var x = Bt(g), L = (E = (I = l.sliderDrags.get(x)) == null ? void 0 : I.key) != null ? E : null;
                            l.textDrags.delete(x), l.sliderDrags.delete(x), l.dialogDrags.delete(x), l.scroll.draggingPointerId === x && (l.scroll.draggingPointerId = null), l.color.draggingPointerId === x && (l.color.draggingPointerId = null);
                            try {
                                for (var _b = __values(l.iframeScroll.values()), _c = _b.next(); !_c.done; _c = _b.next()) {
                                    var M = _c.value;
                                    M.draggingPointerId === x && (M.draggingPointerId = null);
                                }
                            }
                            catch (e_70_1) { e_70 = { error: e_70_1 }; }
                            finally {
                                try {
                                    if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
                                }
                                finally { if (e_70) throw e_70.error; }
                            }
                            {
                                var M = l.numberHolds.get(x);
                                M && (M.timeoutId != null && window.clearTimeout(M.timeoutId), M.intervalId != null && window.clearInterval(M.intervalId), l.numberHolds.delete(x));
                            }
                            if (L) {
                                var M = (R = l.temporalYearOwners.get(L)) != null ? R : null;
                                if (M) {
                                    var N = l.temporals.get(M);
                                    N && N.openYear && (N.openYear = !1, l.temporals.set(M, N), xt == null || xt());
                                }
                            }
                            W_2();
                        }), a_2.on("pointerdown", function (g) { var Z, yt, kt, Tt, bt, ct; if ((g == null ? void 0 : g.button) === 2)
                            return; var x = Bt(g); if (x <= 0)
                            return; var L = (yt = (Z = g.global) == null ? void 0 : Z.x) != null ? yt : 0, I = (Tt = (kt = g.global) == null ? void 0 : kt.y) != null ? Tt : 0, E = l.scroll.track, R = l.scroll.thumb; if (!(L >= E.x && L <= E.x + E.w && I >= E.y && I <= E.y + E.h))
                            return; var N = Math.max(0, l.scroll.contentHeight - l.scroll.viewportHeight); if (N <= .5)
                            return; if (L >= R.x && L <= R.x + R.w && I >= R.y && I <= R.y + R.h) {
                            l.scroll.draggingPointerId = x, l.scroll.dragOffsetY = I - R.y, (bt = g.stopPropagation) == null || bt.call(g);
                            return;
                        } var Q = Math.max(1, E.h - R.h), z = Math.max(E.y, Math.min(E.y + Q, I - R.h / 2)), at = (z - E.y) / Q; l.scroll.y = Math.max(0, Math.min(N, at * N)), l.scroll.draggingPointerId = x, l.scroll.dragOffsetY = I - z, Dt() && (window.__TRUEOS_PIXI_SCROLL_REPAINT_REQUIRED__ = !0, window.__TRUEOS_PIXI_SCROLL_REPAINT_OWNER__ = "root", window.__TRUEOS_PIXI_SCROLL_REPAINT_IFRAME_KEY__ = ""), xt == null || xt(), (ct = g.stopPropagation) == null || ct.call(g); }), i_1.stage.on("pointermove", function (g) {
                            var e_71, _a;
                            var N, J, Q, z, at, Z, yt, kt, Tt, bt, ct, it, ft, Ft, ht, St, zt, re, le, ce, be, _e, ye, me, st, wt, jt, Ct;
                            var x = Number((Q = (J = g == null ? void 0 : g.pointerId) != null ? J : (N = g == null ? void 0 : g.data) == null ? void 0 : N.pointerId) != null ? Q : 1);
                            if (String((Z = (at = g == null ? void 0 : g.pointerType) != null ? at : (z = g == null ? void 0 : g.data) == null ? void 0 : z.pointerType) != null ? Z : "").toLowerCase() === "mouse" || x === 1) {
                                var _t = (kt = (yt = g.global) == null ? void 0 : yt.x) != null ? kt : 0, nt = (bt = (Tt = g.global) == null ? void 0 : Tt.y) != null ? bt : 0;
                                l.lastMouse.x = _t, l.lastMouse.y = nt, l.lastMouse.has = !0, l.primaryMousePointerId = x;
                                var Et = l.harness.enabled ? l.harness.activeUserPointerId : x;
                                l.userCursorPos.set(Et, { x: _t, y: nt }), gt_1();
                            }
                            var E = Bt(g);
                            if (E <= 0)
                                return;
                            var R = !1, M = !1;
                            {
                                var _t = l.textDrags.get(E);
                                if (_t) {
                                    var nt = _t.key, Et = l.fieldBounds.get(nt), Gt = l.inputs.get(nt);
                                    if (Et && Gt && typeof Gt.value == "string") {
                                        var Nt = Et.isPassword ? "\u2022".repeat(Gt.value.length) : Gt.value, mt = Te(we(Nt, Math.max(0, Et.innerWidth), _1), Et.maxLines), Xt = ((it = (ct = g.global) == null ? void 0 : ct.x) != null ? it : 0) - Et.x - Et.innerLeft, Yt = ((Ft = (ft = g.global) == null ? void 0 : ft.y) != null ? Ft : 0) - Et.y - Et.innerTop, xe = Ne({ fullText: Nt, lines: mt, localX: Xt, localY: Yt, lineHeight: p_1, measure: _1 });
                                        Gt.selections || (Gt.selections = new Map), Gt.selections.set(E, { start: _t.anchor, end: xe }), R = !0;
                                    }
                                }
                            }
                            {
                                var _t = l.sliderDrags.get(E);
                                if (_t) {
                                    var nt = _t.key, Et = l.sliderBounds.get(nt);
                                    if (Et) {
                                        var Nt = ((St = (ht = g.global) == null ? void 0 : ht.x) != null ? St : 0) - Et.x, mt = Math.max(1, Et.w - Et.innerPad * 2), Xt = (Nt - Et.innerPad) / mt, Yt = Pe(l.sliders, nt, void 0);
                                        Yt.value = Math.max(0, Math.min(1, Xt)), R = !0;
                                    }
                                }
                            }
                            {
                                var _t = l.color.draggingPointerId;
                                if (_t != null && _t === E) {
                                    var nt = l.color.bounds;
                                    if (nt) {
                                        var Et = (re = (zt = g.global) == null ? void 0 : zt.x) != null ? re : 0, Gt = (ce = (le = g.global) == null ? void 0 : le.y) != null ? ce : 0, Nt = Et - nt.x, mt = Gt - nt.y, Xt = Gn({ lx: Nt, ly: mt, w: nt.w, h: nt.h });
                                        Xt && (l.color.rgb = Xt, l.color.pick = { x: Nt, y: mt }, R = !0);
                                    }
                                }
                            }
                            {
                                var _t = l.scroll.draggingPointerId;
                                if (_t != null && _t === E) {
                                    var nt = l.scroll.track, Et = l.scroll.thumb, Gt = Math.max(0, l.scroll.contentHeight - l.scroll.viewportHeight);
                                    if (Gt > .5 && nt.h > 0 && Et.h > 0) {
                                        var Nt = (_e = (be = g.global) == null ? void 0 : be.y) != null ? _e : 0, mt = Math.max(1, nt.h - Et.h), Yt = (Math.max(nt.y, Math.min(nt.y + mt, Nt - l.scroll.dragOffsetY)) - nt.y) / mt;
                                        l.scroll.y = Math.max(0, Math.min(Gt, Yt * Gt)), R = !0, M = !0, Dt() && (window.__TRUEOS_PIXI_SCROLL_REPAINT_OWNER__ = "root", window.__TRUEOS_PIXI_SCROLL_REPAINT_IFRAME_KEY__ = "");
                                    }
                                }
                            }
                            try {
                                for (var _b = __values(l.iframeScroll.entries()), _c = _b.next(); !_c.done; _c = _b.next()) {
                                    var _d = __read(_c.value, 2), _t = _d[0], nt = _d[1];
                                    if (nt.draggingPointerId == null || nt.draggingPointerId !== E)
                                        continue;
                                    var Et = Math.max(0, nt.contentHeight - nt.viewportHeight);
                                    if (Et <= .5 || nt.track.h <= 0 || nt.thumb.h <= 0)
                                        continue;
                                    var Gt = (me = (ye = g.global) == null ? void 0 : ye.y) != null ? me : 0, Nt = Math.max(1, nt.track.h - nt.thumb.h), Xt = (Math.max(nt.track.y, Math.min(nt.track.y + Nt, Gt - nt.dragOffsetY)) - nt.track.y) / Nt;
                                    nt.y = Math.max(0, Math.min(Et, Xt * Et)), R = !0, M = !0, Dt() && (window.__TRUEOS_PIXI_SCROLL_REPAINT_OWNER__ = "iframe", window.__TRUEOS_PIXI_SCROLL_REPAINT_IFRAME_KEY__ = _t);
                                }
                            }
                            catch (e_71_1) { e_71 = { error: e_71_1 }; }
                            finally {
                                try {
                                    if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
                                }
                                finally { if (e_71) throw e_71.error; }
                            }
                            {
                                var _t = l.dialogDrags.get(E);
                                if (_t) {
                                    var nt = ln(l.dialogs, _t.key), Et = (wt = (st = g.global) == null ? void 0 : st.x) != null ? wt : 0, Gt = (Ct = (jt = g.global) == null ? void 0 : jt.y) != null ? Ct : 0;
                                    nt.x = _t.originX + (Et - _t.startGX), nt.y = _t.originY + (Gt - _t.startGY);
                                    var Nt = l.dialogDragBounds.get(_t.key);
                                    Nt && (nt.x = Math.max(Nt.minX, Math.min(Nt.maxX, nt.x)), nt.y = Math.max(Nt.minY, Math.min(Nt.maxY, nt.y))), R = !0;
                                }
                            }
                            R && (M && Dt() && (window.__TRUEOS_PIXI_SCROLL_REPAINT_REQUIRED__ = !0), xt == null || xt());
                        }), At("main:input-listeners"), window.addEventListener("keydown", function (g) {
                            var kt, Tt, bt, ct, it, ft, Ft;
                            var x = l.keyboardOwnerPointerId, L = (kt = l.focusedKeyByPointer.get(x)) != null ? kt : null;
                            if (!L)
                                return;
                            var I = l.inputs.get(L);
                            if (!I || typeof I.value != "string")
                                return;
                            if (I.selections || (I.selections = new Map), !I.selections.has(x)) {
                                var ht = I.value.length;
                                I.selections.set(x, { start: ht, end: ht });
                            }
                            var E = I.selections.get(x), R = I.value.length, M = function (ht) { return Math.max(0, Math.min(R, ht)); }, N = M((Tt = E.start) != null ? Tt : R), J = M((bt = E.end) != null ? bt : N);
                            E.start = N, E.end = J;
                            var Q = Math.min(N, J), z = Math.max(N, J), at = Q !== z, Z = function (ht) { var St = Math.max(0, Math.min(I.value.length, ht)); E.start = St, E.end = St; }, yt = function (ht, St) { E.start = Math.max(0, Math.min(I.value.length, ht)), E.end = Math.max(0, Math.min(I.value.length, St)); };
                            if (g.key.toLowerCase() === "a" && (g.ctrlKey || g.metaKey)) {
                                yt(0, I.value.length), g.preventDefault(), C_1();
                                return;
                            }
                            if (g.key === "ArrowLeft" || g.key === "ArrowRight") {
                                var ht = g.key === "ArrowLeft" ? -1 : 1;
                                if (g.shiftKey) {
                                    var St = (ct = E.start) != null ? ct : R, zt = ((it = E.end) != null ? it : St) + ht;
                                    yt(St, zt);
                                }
                                else
                                    Z((at ? Q : z) + ht);
                                g.preventDefault(), Y_1();
                                return;
                            }
                            if (g.key === "Home") {
                                g.shiftKey ? yt((ft = E.start) != null ? ft : R, 0) : Z(0), g.preventDefault(), Y_1();
                                return;
                            }
                            if (g.key === "End") {
                                g.shiftKey ? yt((Ft = E.start) != null ? Ft : 0, I.value.length) : Z(I.value.length), g.preventDefault(), Y_1();
                                return;
                            }
                            if (g.key === "Backspace") {
                                if (at)
                                    I.value = I.value.slice(0, Q) + I.value.slice(z), Z(Q);
                                else {
                                    var ht = z;
                                    ht > 0 && (I.value = I.value.slice(0, ht - 1) + I.value.slice(ht), Z(ht - 1));
                                }
                                g.preventDefault(), Y_1();
                                return;
                            }
                            if (g.key === "Enter") {
                                var ht = "\n";
                                if (at)
                                    I.value = I.value.slice(0, Q) + ht + I.value.slice(z), Z(Q + ht.length);
                                else {
                                    var St = z;
                                    I.value = I.value.slice(0, St) + ht + I.value.slice(St), Z(St + ht.length);
                                }
                                g.preventDefault(), Y_1();
                                return;
                            }
                            if (g.key === "Delete") {
                                if (at)
                                    I.value = I.value.slice(0, Q) + I.value.slice(z), Z(Q);
                                else {
                                    var ht = z;
                                    ht < I.value.length && (I.value = I.value.slice(0, ht) + I.value.slice(ht + 1), Z(ht));
                                }
                                g.preventDefault(), Y_1();
                                return;
                            }
                            if (g.key === "Escape") {
                                l.focusedKeyByPointer.set(x, null), Y_1();
                                return;
                            }
                            if (g.key.length === 1 && !g.ctrlKey && !g.metaKey && !g.altKey) {
                                if (at)
                                    I.value = I.value.slice(0, Q) + g.key + I.value.slice(z), Z(Q + 1);
                                else {
                                    var ht = z;
                                    I.value = I.value.slice(0, ht) + g.key + I.value.slice(ht), Z(ht + 1);
                                }
                                g.preventDefault(), Y_1();
                            }
                        }), window.addEventListener("resize", function () { Y_1(), y_1.visible = l.virtualCursor.enabled; }), At("main:done"), r && (window.__TRUEOS_PIXI_APP_READY__ = !0, window.__TRUEOS_PIXI_APP_ERROR__ = "", window.__TRUEOS_PIXI_APP_PHASE__ = "ready");
                        return [3 /*break*/, 10];
                    case 9:
                        n_3 = _d.sent();
                        window.__TRUEOS_PIXI_APP_READY__ = !1, window.__TRUEOS_PIXI_APP_ERROR__ = Yi(n_3);
                        try {
                            console.error(n_3);
                        }
                        catch (r) { }
                        try {
                            r = document.createElement("pre");
                            r.textContent = String((e = n_3 == null ? void 0 : n_3.stack) != null ? e : n_3), document.body.appendChild(r);
                        }
                        catch (r) { }
                        return [3 /*break*/, 10];
                    case 10: return [2 /*return*/];
                }
            });
        });
    }
    Ls().then(function () { window.__TRUEOS_PIXI_APP_ERROR__ || (window.__TRUEOS_PIXI_APP_READY__ = !0, window.__TRUEOS_PIXI_APP_ERROR__ = "", window.__TRUEOS_PIXI_APP_PHASE__ = "ready"); }).catch(function (t) { var n; window.__TRUEOS_PIXI_APP_READY__ = !1, window.__TRUEOS_PIXI_APP_ERROR__ = Yi(t), console.error(t); var e = document.createElement("pre"); e.textContent = String((n = t == null ? void 0 : t.stack) != null ? n : t), document.body.appendChild(e); });
})();
