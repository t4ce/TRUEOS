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
    var zn = Object.defineProperty, Di = Object.defineProperties;
    var Ai = Object.getOwnPropertyDescriptors;
    var Kn = Object.getOwnPropertySymbols;
    var vi = Object.prototype.hasOwnProperty, Ni = Object.prototype.propertyIsEnumerable;
    var gn = function (t, e, n) { return e in t ? zn(t, e, { enumerable: !0, configurable: !0, writable: !0, value: n }) : t[e] = n; }, re = function (t, e) {
        var e_1, _a;
        for (var n in e || (e = {}))
            vi.call(e, n) && gn(t, n, e[n]);
        if (Kn)
            try {
                for (var _b = __values(Kn(e)), _c = _b.next(); !_c.done; _c = _b.next()) {
                    var n = _c.value;
                    Ni.call(e, n) && gn(t, n, e[n]);
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
    }, ge = function (t, e) { return Di(t, Ai(e)); };
    var Gi = function (t, e) { return function () { return (t && (e = t(t = 0)), e); }; };
    var Li = function (t, e) { for (var n in e)
        zn(t, n, { get: e[n], enumerable: !0 }); };
    var et = function (t, e, n) { return gn(t, typeof e != "symbol" ? e + "" : e, n); };
    var ze = function (t, e, n) { return new Promise(function (r, i) { var o = function (a) { try {
        l(n.next(a));
    }
    catch (d) {
        i(d);
    } }, s = function (a) { try {
        l(n.throw(a));
    }
    catch (d) {
        i(d);
    } }, l = function (a) { return a.done ? r(a.value) : Promise.resolve(a.value).then(o, s); }; l((n = n.apply(t, e)).next()); }); };
    var mi = {};
    Li(mi, { default: function () { return Ao; } });
    var Ao, fi = Gi(function () { Ao = {}; });
    var De = /** @class */ (function () {
        function De(e, n) {
            if (e === void 0) { e = 0; }
            if (n === void 0) { n = 0; }
            et(this, "x");
            et(this, "y");
            this.x = Number(e) || 0, this.y = Number(n) || 0;
        }
        De.prototype.set = function (e, n) {
            if (e === void 0) { e = 0; }
            if (n === void 0) { n = e; }
            this.x = Number(e) || 0, this.y = Number(n) || 0;
        };
        return De;
    }()), gt = /** @class */ (function () {
        function gt(e, n, r, i) {
            if (e === void 0) { e = 0; }
            if (n === void 0) { n = 0; }
            if (r === void 0) { r = 0; }
            if (i === void 0) { i = 0; }
            et(this, "x");
            et(this, "y");
            et(this, "width");
            et(this, "height");
            this.x = Number(e) || 0, this.y = Number(n) || 0, this.width = Number(r) || 0, this.height = Number(i) || 0;
        }
        return gt;
    }()), bn = /** @class */ (function () {
        function bn() {
            et(this, "parent");
            et(this, "children");
            et(this, "label");
            et(this, "name");
            et(this, "position");
            et(this, "scale");
            et(this, "pivot");
            et(this, "visible");
            et(this, "alpha");
            et(this, "mask");
            et(this, "rotation");
            et(this, "zIndex");
            et(this, "eventMode");
            et(this, "cursor");
            et(this, "hitArea");
            et(this, "listeners");
            this.parent = null, this.position = new De, this.scale = new De(1, 1), this.pivot = new De, this.visible = !0, this.alpha = 1, this.mask = null, this.rotation = 0, this.zIndex = 0, this.eventMode = null, this.cursor = null, this.hitArea = null, this.listeners = {};
        }
        Object.defineProperty(bn.prototype, "x", {
            get: function () { return this.position.x; },
            set: function (e) { this.position.x = Number(e) || 0; },
            enumerable: false,
            configurable: true
        });
        Object.defineProperty(bn.prototype, "y", {
            get: function () { return this.position.y; },
            set: function (e) { this.position.y = Number(e) || 0; },
            enumerable: false,
            configurable: true
        });
        bn.prototype.on = function (e, n) { return this; };
        bn.prototype.removeAllListeners = function (e) { return e == null ? this.listeners = {} : delete this.listeners[String(e)], this; };
        bn.prototype.removeFromParent = function () { var e; return (e = this.parent) == null || e.removeChild(this), this; };
        bn.prototype.destroy = function (e) { this.removeFromParent(), this.removeAllListeners(); };
        bn.prototype.toLocal = function (e) { var n = e || {}; return { x: (Number(n.x) || 0) - this.getGlobalX(), y: (Number(n.y) || 0) - this.getGlobalY() }; };
        bn.prototype.getGlobalPosition = function () { return { x: this.getGlobalX(), y: this.getGlobalY() }; };
        bn.prototype.getGlobalX = function () { return (this.parent ? this.parent.getGlobalX() : 0) + this.x; };
        bn.prototype.getGlobalY = function () { return (this.parent ? this.parent.getGlobalY() : 0) + this.y; };
        return bn;
    }()), _t = /** @class */ (function (_super) {
        __extends(_t, _super);
        function _t() {
            var _this = _super.call(this) || this;
            et(_this, "children");
            et(_this, "sortableChildren");
            _this.children = [], _this.sortableChildren = !1;
            return _this;
        }
        _t.prototype.addChild = function () {
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
        _t.prototype.addChildAt = function (n, r) { var o; (o = n.parent) == null || o.removeChild(n), n.parent = this; var i = Math.max(0, Math.min(Number(r) | 0, this.children.length)); return this.children.splice(i, 0, n), n; };
        _t.prototype.removeChild = function () {
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
        _t.prototype.removeChildren = function (n, r) {
            var e_4, _a;
            if (n === void 0) { n = 0; }
            if (r === void 0) { r = this.children.length; }
            var i = Math.max(0, Number(n) | 0), o = Math.max(i, Math.min(Number(r) | 0, this.children.length)), s = this.children.splice(i, o - i);
            try {
                for (var s_1 = __values(s), s_1_1 = s_1.next(); !s_1_1.done; s_1_1 = s_1.next()) {
                    var l = s_1_1.value;
                    l.parent = null;
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
        _t.prototype.setChildIndex = function (n, r) { var i = this.children.indexOf(n); if (i < 0)
            return; this.children.splice(i, 1); var o = Math.max(0, Math.min(Number(r) | 0, this.children.length)); this.children.splice(o, 0, n); };
        _t.prototype.getChildIndex = function (n) { return this.children.indexOf(n); };
        _t.prototype.getChildByLabel = function (n) { for (var r = 0; r < this.children.length; r += 1) {
            var i = this.children[r];
            if (i && i.label === n)
                return i;
        } return null; };
        return _t;
    }(bn)), wt = /** @class */ (function (_super) {
        __extends(wt, _super);
        function wt() {
            var _this = _super.call(this) || this;
            et(_this, "commands");
            _this.commands = [];
            return _this;
        }
        wt.prototype.clear = function () { return this.commands.length = 0, this; };
        wt.prototype.rect = function (n, r, i, o) { return this.commands.push(["rect", n, r, i, o]), this; };
        wt.prototype.roundRect = function (n, r, i, o, s) {
            if (s === void 0) { s = 0; }
            return this.commands.push(["roundRect", n, r, i, o, s]), this;
        };
        wt.prototype.circle = function (n, r, i) { return this.commands.push(["circle", n, r, i]), this; };
        wt.prototype.ellipse = function (n, r, i, o) { return this.commands.push(["ellipse", n, r, i, o]), this; };
        wt.prototype.moveTo = function (n, r) { return this.commands.push(["moveTo", n, r]), this; };
        wt.prototype.lineTo = function (n, r) { return this.commands.push(["lineTo", n, r]), this; };
        wt.prototype.closePath = function () { return this.commands.push(["closePath"]), this; };
        wt.prototype.poly = function (n) { return this.commands.push(["poly", n]), this; };
        wt.prototype.fill = function (n) { return this.commands.push(["fill", n]), this; };
        wt.prototype.stroke = function (n) { return this.commands.push(["stroke", n]), this; };
        wt.prototype.image = function (n, r, i, o, s) { return this.commands.push(["image", n, r, i, o, s]), this; };
        wt.prototype.svg = function (n) { return this.commands.push(["svg", n]), this; };
        return wt;
    }(_t)), Qt = /** @class */ (function (_super) {
        __extends(Qt, _super);
        function Qt(n) {
            if (n === void 0) { n = ""; }
            var r, i;
            var _this = _super.call(this) || this;
            et(_this, "_text");
            et(_this, "_style");
            et(_this, "_resolution");
            _this._text = "", _this._style = {}, _this._resolution = 1, typeof n == "string" ? _this._text = n : (_this._text = String((r = n.text) != null ? r : ""), _this._style = re({}, (i = n.style) != null ? i : {}));
            return _this;
        }
        Object.defineProperty(Qt.prototype, "text", {
            get: function () { return this._text; },
            set: function (n) { this._text = String(n != null ? n : ""); },
            enumerable: false,
            configurable: true
        });
        Object.defineProperty(Qt.prototype, "style", {
            get: function () { return this._style; },
            set: function (n) { this._style = n != null ? n : {}; },
            enumerable: false,
            configurable: true
        });
        Object.defineProperty(Qt.prototype, "resolution", {
            get: function () { return this._resolution; },
            set: function (n) { this._resolution = Math.max(1, Number(n) || 1); },
            enumerable: false,
            configurable: true
        });
        Object.defineProperty(Qt.prototype, "width", {
            get: function () { var n = Number(this._style.fontSize) || 16; return this._text.length * n * .58; },
            enumerable: false,
            configurable: true
        });
        Object.defineProperty(Qt.prototype, "height", {
            get: function () { var n = Number(this._style.fontSize) || 16; return Number(this._style.lineHeight) || n * 1.25; },
            enumerable: false,
            configurable: true
        });
        Qt.prototype.setSize = function (n, r) { return this; };
        return Qt;
    }(_t)), Te = /** @class */ (function () {
        function Te(e) {
            if (e === void 0) { e = {}; }
            et(this, "options");
            this.options = e;
        }
        Te.prototype.addAttribute = function (e, n) { return this; };
        Te.prototype.destroy = function () { };
        return Te;
    }()), je = /** @class */ (function (_super) {
        __extends(je, _super);
        function je(n) {
            if (n === void 0) { n = {}; }
            var r, i;
            var _this = _super.call(this) || this;
            et(_this, "geometry");
            et(_this, "shader");
            _this.geometry = (r = n.geometry) != null ? r : new Te, _this.shader = (i = n.shader) != null ? i : new Ae;
            return _this;
        }
        return je;
    }(_t)), Ve = /** @class */ (function () {
        function Ve(e) {
            if (e === void 0) { e = {}; }
            et(this, "options");
            this.options = e;
        }
        return Ve;
    }()), xn = { VERTEX: 1, COPY_DST: 2 }, Ae = /** @class */ (function () {
        function Ae(e) {
            if (e === void 0) { e = {}; }
            et(this, "options");
            this.options = e;
        }
        return Ae;
    }());
    var jn = "", Vn = "", Jn = "", Je = /** @class */ (function () {
        function Je() {
            var _this = this;
            et(this, "stage");
            et(this, "screen");
            et(this, "canvas");
            et(this, "renderer");
            et(this, "ticker");
            var e = Math.max(1, Number(globalThis.innerWidth || 1920) | 0), n = Math.max(1, Number(globalThis.innerHeight || 1080) | 0);
            this.stage = new _t, this.screen = new gt(0, 0, e, n), this.canvas = document.createElement("canvas"), this.ticker = { stop: function () { }, add: function () { }, remove: function () { } }, this.renderer = { width: e, height: n, screen: this.screen, render: function (r) { return r; }, resize: function (r, i) { var o = Math.max(1, Number(r || e) | 0), s = Math.max(1, Number(i || n) | 0); _this.renderer.width = o, _this.renderer.height = s, _this.screen.width = o, _this.screen.height = s; } };
        }
        Je.prototype.init = function (e) { return ze(this, null, function () { return __generator(this, function (_a) {
            return [2 /*return*/];
        }); }); };
        return Je;
    }());
    var be = { fontFamily: "system-ui, -apple-system, Segoe UI, Arial", fontSize: 16, background: 16777215, text: 1118481, mutedText: 6710886, boxBorder: 14540253, hr: 13421772, control: { border: 0, focusBorder: 3900150, background: 16777215, accent: 3900150, radius: 0, button: { fill: 15921906, hoverFill: 15395562, activeFill: 14737632, border: 6710886, text: 1118481, radius: 0 }, progress: { border: 10066329, background: 16777215, fill: 6990335 }, table: { border: 10066329, cellBorder: 11579568, headerFill: 16250871 } } };
    var Me = 24, xt = 1;
    function zt(t) { var i, o; var e = t.wrapWidth, n = (i = t.wordWrap) != null ? i : e != null, r = (o = t.wordWrapWidth) != null ? o : e == null ? void 0 : Math.max(0, Math.ceil(e) + Me); return new Qt({ text: t.text, style: { fontFamily: t.fontFamily, fontSize: t.fontSize, fill: t.fill, fontWeight: t.bold ? "700" : "400", wordWrap: n, wordWrapWidth: r } }); }
    function yn(t, e) { var n = t.children; if (!Array.isArray(n))
        return null; for (var r = 0; r < n.length; r += 1) {
        var i = n[r];
        if (i && i.label === e)
            return i;
    } return null; }
    function xe(t, e) { var n = yn(t, e); if (n)
        return n; var r = new _t; return r.label = e, t.addChild(r), r; }
    function St(t, e) { var n = yn(t, e); if (n)
        return n; var r = new wt; return r.label = e, t.addChild(r), r; }
    function Pt(t, e, n) { var r = yn(t, e); if (r)
        return r; var i = new Qt({ text: "" }); return i.label = e, n == null || n(i), t.addChild(i), i; }
    function Et(t) { t.clear(), t.removeAllListeners(), t.hitArea = null; }
    function $t(t) { t.removeAllListeners(); }
    function ue(t, e, n) {
        var r = String(t != null ? t : ""), i = [], o = 0;
        for (var s = 0; s <= r.length; s++) {
            if (!(s === r.length || r[s] === "\n"))
                continue;
            var a = o, d = s;
            if (a === d)
                i.push({ start: a, end: d, text: "" });
            else {
                var p = a, m = -1;
                for (var x = p; x < d; x++) {
                    r[x] === " " && (m = x);
                    var T = r.slice(p, x + 1);
                    if (n(T) <= e || x === p)
                        continue;
                    var g = m >= p ? m + 1 : x;
                    g <= p && (g = Math.min(d, p + 1)), i.push({ start: p, end: g, text: r.slice(p, g) }), p = g, x = p - 1, m = -1;
                }
                p <= d && i.push({ start: p, end: d, text: r.slice(p, d) });
            }
            o = s + 1;
        }
        return i;
    }
    function de(t, e) { return e <= 0 ? [] : t.length <= e ? t : t.slice(0, e); }
    function Ee(t) { var e = t.fullText, n = t.lines, r = t.localX, i = t.localY, o = t.lineHeight, s = t.measure; if (n.length === 0)
        return 0; var l = Math.max(0, r), a = Math.max(0, i), d = Math.max(1, o), p = Math.max(0, Math.min(n.length - 1, Math.floor(a / d))), m = n[p], x = m.start, c = Number.POSITIVE_INFINITY; for (var T = m.start; T <= m.end; T++) {
        var g = s(e.slice(m.start, T)), b = Math.abs(g - l);
        b < c && (c = b, x = T);
    } return x; }
    function Zn(t) { var T, g, b, _; var e = t.node, n = t.graphics, r = t.w, i = t.h, o = t.theme, s = Math.max(0, Math.round(r)), l = Math.max(0, Math.round(i)); n.rect(.5, .5, Math.max(0, s - 1), Math.max(0, l - 1)), n.fill(o.control.progress.background), n.stroke({ width: 1, color: o.control.progress.border }); var a = Number((g = (T = e.attrs) == null ? void 0 : T.value) != null ? g : "0"), d = Number((_ = (b = e.attrs) == null ? void 0 : b.max) != null ? _ : "1"), p = d > 0 ? Math.max(0, Math.min(1, a / d)) : 0, m = 3, x = Math.max(0, s - m * 2), c = Math.max(0, l - m * 2); n.rect(m, m, Math.max(0, x * p), c), n.fill(o.control.progress.fill); }
    function Qn(t, e) { t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0), t.setHeight(14), t.setMinWidth(240); }
    function ye(t, e, n) { var l; var r = t.get(e); if (r)
        return r; var i = Number((l = n == null ? void 0 : n.value) != null ? l : "0"), o = Number.isFinite(i) ? i : 0, s = { value: Math.max(0, Math.min(1, o)) }; return t.set(e, s), s; }
    function qn(t, e) { t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0), t.setHeight(14), t.setMinWidth(240); }
    function tr(t) { var e = t.node, n = t.Yoga, r = t.measurer, i = n.Node.create(); return i.setPadding(n.EDGE_LEFT, 0), i.setPadding(n.EDGE_RIGHT, 0), i.setPadding(n.EDGE_TOP, 0), i.setPadding(n.EDGE_BOTTOM, 0), i.setMargin(n.EDGE_RIGHT, 6), i.setMeasureFunc(function () { var o = r.measure("100"); return { width: o.width, height: o.height }; }), { yogaNode: i, buildBox: function () { return ({ kind: "block", key: e.key, tagName: e.tagName, attrs: e.attrs, x: i.getComputedLeft(), y: i.getComputedTop(), width: i.getComputedWidth(), height: i.getComputedHeight(), children: [] }); } }; }
    function er(t) { var d, p; var e = t.node, n = t.container, r = t.theme, i = t.sliderStates, o = (d = e.attrs) == null ? void 0 : d["data-slider-key"], s = null; if (o) {
        var m = i.get(o);
        if (m)
            s = m;
        else {
            var x = (p = e.attrs) == null ? void 0 : p["data-slider-init"];
            s = ye(i, o, x != null ? { value: String(x) } : void 0);
        }
    } var l = s ? Math.round(s.value * 100) : 0, a = Pt(n, "__pct", function (m) { m.style = { fontFamily: r.fontFamily, fontSize: r.fontSize, fill: r.text, fontWeight: "400", wordWrap: !1 }; }); a.text = String(l), a.position.set(0, xt); }
    function Ze(t) { var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.absX, l = t.absY, a = t.theme, d = t.sliderStates, p = t.sliderBounds, m = t.sliderDrags, x = t.requestPaint, c = e.key, T = c ? ye(d, c, e.attrs) : null, g = Math.max(0, Math.round(i)), b = Math.max(0, Math.round(o)), _ = 3; c && p.set(c, { x: s, y: l, w: g, h: b, innerPad: _ }), r.rect(.5, .5, Math.max(0, g - 1), Math.max(0, b - 1)), r.fill(a.control.progress.background), r.stroke({ width: 1, color: a.control.progress.border }); var w = T ? Math.max(0, Math.min(1, T.value)) : 0, M = Math.max(0, g - _ * 2), $ = Math.max(0, b - _ * 2); r.rect(_, _, Math.max(0, M * w), $), r.fill(a.control.progress.fill); var A = _ + M * w, H = $ / 2; r.moveTo(A, _ - H), r.lineTo(A, _ + $ + H), r.stroke({ width: 2, color: a.text }), c && ($t(n), n.eventMode = "static", n.cursor = "pointer", n.hitArea = new gt(0, 0, Math.max(0, g), Math.max(0, b)), n.on("pointerdown", function (N) {
        var e_5, _a;
        var F, q, Z, tt, Y, j;
        if ((N == null ? void 0 : N.button) === 2)
            return;
        var k = t.getPointerId ? t.getPointerId(N) : Number((Z = (q = N == null ? void 0 : N.pointerId) != null ? q : (F = N == null ? void 0 : N.data) == null ? void 0 : F.pointerId) != null ? Z : 0);
        if (k <= 0)
            return;
        try {
            for (var _b = __values(m.entries()), _c = _b.next(); !_c.done; _c = _b.next()) {
                var _d = __read(_c.value, 2), v = _d[0], f = _d[1];
                f.key === c && v !== k && m.delete(v);
            }
        }
        catch (e_5_1) { e_5 = { error: e_5_1 }; }
        finally {
            try {
                if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
            }
            finally { if (e_5) throw e_5.error; }
        }
        m.set(k, { key: c });
        var E = p.get(c), W = (Y = (tt = N.global) == null ? void 0 : tt.x) != null ? Y : 0, U = E ? W - E.x : 0, G = E ? Math.max(1, E.w - E.innerPad * 2) : 1, h = (U - ((j = E == null ? void 0 : E.innerPad) != null ? j : 0)) / G, P = ye(d, c, e.attrs);
        P.value = Math.max(0, Math.min(1, h)), x == null || x();
    })); }
    function nr(t) { var $; var e = t.node, n = t.container, r = t.w, i = t.h, o = t.theme, s = t.detailsOpen, l = t.requestRerender, a = ($ = e.attrs) == null ? void 0 : $["data-details-key"], d = e.attrs ? Object.prototype.hasOwnProperty.call(e.attrs, "data-details-open") : !1, p = a && s.has(a) ? s.get(a) === !0 : d, m = function (A) { var k; if (!a || (A == null ? void 0 : A.button) === 2)
        return; var N = !(s.has(a) ? s.get(a) === !0 : d); s.set(a, N), l == null || l(), (k = A == null ? void 0 : A.stopPropagation) == null || k.call(A); }, x = 16, c = St(n, "__arrow"); Et(c); var T = 2, g = 3, b = g, _ = g, w = x - g, M = x - g; p ? (c.moveTo(b, _), c.lineTo((b + w) / 2, M), c.lineTo(w, _)) : (c.moveTo(b, _), c.lineTo(w, (_ + M) / 2), c.lineTo(b, M)), c.stroke({ width: T, color: o.text }), c.position.set(4, Math.max(0, (i - x) / 2)), c.eventMode = "static", c.cursor = "pointer", c.hitArea = new gt(0, 0, x + 8, x + 8), c.on("pointerdown", m), a && ($t(n), n.eventMode = "static", n.cursor = "pointer", n.hitArea = new gt(0, 0, Math.max(0, r), Math.max(0, i)), n.on("pointerdown", m)); }
    function rr(t, e) { t.setFlexDirection(e.FLEX_DIRECTION_ROW), t.setAlignItems(e.ALIGN_CENTER), t.setJustifyContent(e.JUSTIFY_SPACE_BETWEEN), t.setPadding(e.EDGE_TOP, 6), t.setPadding(e.EDGE_BOTTOM, 6), t.setPadding(e.EDGE_LEFT, 26), t.setPadding(e.EDGE_RIGHT, 12), t.setMinHeight(36); }
    function ir(t, e) { t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0); }
    function or(t, e) { var i, o, s; if (!t || t.tagName !== "details" || !t.key)
        return (i = t == null ? void 0 : t.children) != null ? i : []; var n = t.attrs ? Object.prototype.hasOwnProperty.call(t.attrs, "open") : !1; return (e.has(t.key) ? e.get(t.key) === !0 : n) ? (o = t.children) != null ? o : [] : ((s = t.children) != null ? s : []).filter(function (l) { return l && l.kind === "block" && l.tagName === "summary"; }); }
    function sr(t) { var e = t.graphics, n = t.w, r = t.theme; e.rect(0, 0, Math.round(n), 1), e.fill(r.hr); }
    function ar(t, e) { t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0), t.setMargin(e.EDGE_TOP, 2), t.setMargin(e.EDGE_BOTTOM, 2), t.setHeight(1); }
    function lr(t) { var b, _; var e = t.container, n = t.graphics, r = t.w, i = t.h, o = t.label, s = t.theme, l = t.registerHoverHandlers, a = function (w) { n.clear(); var M = 1, $ = M / 2; s.control.button.radius > 0 ? n.roundRect($, $, Math.max(0, r - M), Math.max(0, i - M), s.control.button.radius) : n.rect($, $, Math.max(0, r - M), Math.max(0, i - M)), n.fill(w), n.stroke({ width: M, color: s.control.button.border }); }; a(s.control.button.fill); var d = Pt(e, "__label", function (w) { w.style = { fontFamily: s.fontFamily, fontSize: s.fontSize, fill: s.control.button.text, fontWeight: "400", wordWrap: !1, wordWrapWidth: 0 }; }), p = String(o != null ? o : "").trim(); d.text = p, d.visible = p.length > 0, d.style = ge(re({}, d.style), { fontFamily: s.fontFamily, fontSize: s.fontSize, fill: s.control.button.text, wordWrap: !1, wordWrapWidth: Math.max(0, Math.ceil(r - 16)) }); var m = Number((b = d.width) != null ? b : 0), x = Number((_ = d.height) != null ? _ : 0), c = s.fontSize * 1.25; d.position.set(m > 0 ? Math.max(8, Math.floor((r - m) / 2)) : 8, Math.max(0, Math.floor((i - (x > 0 ? x : c)) / 2)) + xt); var T = function () { return a(s.control.button.hoverFill); }, g = function () { return a(s.control.button.fill); }; l == null || l({ over: T, out: g }), $t(e), e.eventMode = "static", e.cursor = "pointer", e.on("pointerover", T), e.on("pointerout", g), e.on("pointerdown", function () { return a(s.control.button.activeFill); }), e.on("pointerup", function () { return a(s.control.button.hoverFill); }); }
    function cr(t, e) { t.setFlexDirection(e.FLEX_DIRECTION_ROW), t.setPadding(e.EDGE_TOP, 6), t.setPadding(e.EDGE_BOTTOM, 6), t.setMinHeight(36), t.setMinWidth(100), t.setAlignItems(e.ALIGN_CENTER), t.setJustifyContent(e.JUSTIFY_CENTER); }
    function ur(t) { var e = t.graphics, n = t.w, r = t.h, i = t.boxBorder, o = Math.max(0, Math.round(n)), s = Math.max(0, Math.round(r)); e.rect(0, 0, o, s), e.stroke({ width: 1, color: i, alignment: 0 }); }
    function dr(t) { var e = t.nodeTag, n = t.graphics, r = t.w, i = t.h, o = t.theme; e === "th" && (n.rect(0, 0, r, i), n.fill(o.control.table.headerFill)), n.rect(0, 0, r, i), n.stroke({ width: 1, color: o.control.table.cellBorder }); }
    function hr(t, e) { t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0); }
    function mr(t, e) { t.setFlexDirection(e.FLEX_DIRECTION_ROW), t.setFlexWrap(e.WRAP_NO_WRAP), t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0), t.setMargin(e.EDGE_BOTTOM, 0); }
    function fr(t, e) { t.setFlexGrow(1), t.setFlexShrink(1), t.setMinWidth(80), t.setPadding(e.EDGE_LEFT, 8), t.setPadding(e.EDGE_RIGHT, 8), t.setPadding(e.EDGE_TOP, 6), t.setPadding(e.EDGE_BOTTOM, 6), t.setMargin(e.EDGE_BOTTOM, 0); }
    function wn(t) { var e = String(t != null ? t : "").toLowerCase(); if (e.length !== 2 || e.charAt(0) !== "h")
        return !1; var n = e.charCodeAt(1); return n >= 49 && n <= 54; }
    function pr(t, e) { t.setPadding(e.EDGE_TOP, 6), t.setPadding(e.EDGE_BOTTOM, 6), t.setMinHeight(36), t.setJustifyContent(e.JUSTIFY_CENTER); }
    function gr(t, e) {
        var n = Math.max(1, Math.floor(t)), r = Math.max(1, Math.floor(e));
        return "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<svg viewBox=\"0 0 ".concat(n, " ").concat(r, "\" xmlns=\"http://www.w3.org/2000/svg\">\n  <rect x=\"0\" y=\"0\" width=\"").concat(n, "\" height=\"").concat(r, "\" fill=\"#f6f6f6\"/>\n  <rect x=\"0.5\" y=\"0.5\" width=\"").concat(Math.max(0, n - 1), "\" height=\"").concat(Math.max(0, r - 1), "\" fill=\"none\" stroke=\"#999\"/>\n  <path d=\"M2 2 L").concat(Math.max(2, n - 2), " ").concat(Math.max(2, r - 2), "\" stroke=\"#c8c8c8\"/>\n  <path d=\"M").concat(Math.max(2, n - 2), " 2 L2 ").concat(Math.max(2, r - 2), "\" stroke=\"#c8c8c8\"/>\n</svg>");
    }
    function br(_a) {
        var _b = _a === void 0 ? {} : _a, _c = _b.ring, t = _c === void 0 ? 34 : _c, _d = _b.core, e = _d === void 0 ? 14 : _d, _f = _b.hueA, n = _f === void 0 ? "#00e5ff" : _f, _g = _b.hueB, r = _g === void 0 ? "#ff2bd6" : _g;
        var i = Math.max(0, t - 10), o = Math.max(0, e * .35);
        return "\n<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 100 100\">\n  <rect width=\"100\" height=\"100\" fill=\"#ffffff\"/>\n  <rect width=\"100\" height=\"100\" fill=\"".concat(n, "\" opacity=\"0.08\"/>\n\n  <circle cx=\"50\" cy=\"50\" r=\"").concat(t, "\" fill=\"none\" stroke=\"").concat(r, "\" stroke-width=\"4\" opacity=\"0.95\"/>\n  <circle cx=\"50\" cy=\"50\" r=\"").concat(i, "\" fill=\"none\" stroke=\"").concat(n, "\" stroke-width=\"1\" opacity=\"0.35\"/>\n\n  <circle cx=\"50\" cy=\"50\" r=\"").concat(e, "\" fill=\"").concat(n, "\" opacity=\"0.9\"/>\n  <circle cx=\"43\" cy=\"43\" r=\"").concat(o, "\" fill=\"#ffffff\" opacity=\"0.55\"/>\n\n  <path d=\"M50 16 L52 22 L58 24 L52 26 L50 32 L48 26 L42 24 L48 22 Z\" fill=\"#ffffff\" opacity=\"0.85\"/>\n  <path d=\"M82 52 L85 56 L90 57 L85 58 L82 62 L79 58 L74 57 L79 56 Z\" fill=\"#ffffff\" opacity=\"0.70\"/>\n  <path d=\"M20 70 L22 74 L27 75 L22 76 L20 80 L18 76 L13 75 L18 74 Z\" fill=\"#ffffff\" opacity=\"0.65\"/>\n</svg>\n");
    }
    var xr = new Map;
    function ve() { var t = globalThis; return !0; }
    function _r(t) { var e = globalThis, n = String(t != null ? t : "").trim(); if (!n)
        return null; var r = e.__TRUEOS_READY_IMAGE_TEXTURE_CACHE__; (!r || typeof r.get != "function" || typeof r.set != "function") && (r = new Map, e.__TRUEOS_READY_IMAGE_TEXTURE_CACHE__ = r); var i = r.get(n); if (i)
        return i; var o = e.__trueosResolveReadyImageTexture; if (typeof o != "function")
        return null; var s = o(n); if (s && typeof s.then == "function") {
        var l = s.then(function (a) { return (r.set(n, a), a); }).catch(function (a) { throw r.delete(n), a; });
        return r.set(n, l), l;
    } return r.set(n, s), s; }
    function Fi(t) { var s; var e = globalThis, n = String(t != null ? t : "").trim(); if (!n)
        return null; var r = e.__trueosPeekReadyImageTexture; if (typeof r != "function")
        return null; var i = r(n); return !i || typeof i.then == "function" ? null : Math.max(0, Number((s = i == null ? void 0 : i.texId) != null ? s : 0) | 0) > 0 ? i : null; }
    function Bi(t, e) { var n = globalThis, r = String(t != null ? t : "").trim(); if (!r || !e)
        return; var i = n.__TRUEOS_READY_IMAGE_TEXTURE_CACHE__; (!i || typeof i.get != "function" || typeof i.set != "function") && (i = new Map, n.__TRUEOS_READY_IMAGE_TEXTURE_CACHE__ = i), i.set(r, e); }
    function Tr(t, e) { var r, i, o, s, l; var n = Math.max(0, Number((r = e == null ? void 0 : e.texId) != null ? r : 0) | 0); if (n <= 0)
        throw new Error("image texture missing"); t.state = "ready", t.texId = n, t.width = Math.max(0, Number((o = (i = e == null ? void 0 : e.pixelWidth) != null ? i : e == null ? void 0 : e.width) != null ? o : 0) | 0), t.height = Math.max(0, Number((l = (s = e == null ? void 0 : e.pixelHeight) != null ? s : e == null ? void 0 : e.height) != null ? l : 0) | 0); }
    function Wi(t, e) { var n = Fi(t) || _r(t); return !n || typeof n.then == "function" ? !1 : (Tr(e, n), Bi(t, n), !0); }
    function yr(t, e) { var n = String(t != null ? t : "").trim(); if (!n)
        return null; var r = xr.get(n); if (r) {
        if (ve() && r.state === "loading")
            try {
                Wi(n, r);
            }
            catch (l) {
                r.state = "error";
            }
        return r;
    } if (ve())
        return null; var i = { state: "loading", texId: 0, width: 0, height: 0 }; xr.set(n, i); var o = function (l) { Tr(i, l), ve() || e == null || e(); }, s = function () { i.state = "error", ve() || e == null || e(); }; try {
        var l = _r(n);
        if (!l)
            return i;
        if (l && typeof l.then == "function") {
            if (ve())
                return i;
            l.then(o).catch(s);
        }
        else
            o(l);
    }
    catch (l) {
        s();
    } return i; }
    function Hi(t) { var e = String(t != null ? t : ""); if (!e.startsWith("data:image/svg+xml"))
        return null; var n = e.indexOf(","); if (n === -1)
        return null; var r = e.slice(0, n).toLowerCase(), i = e.slice(n + 1); try {
        return r.includes(";base64") ? atob(i) : decodeURIComponent(i);
    }
    catch (o) {
        return null;
    } }
    function $i(t) { return wr(wr(String(t), "tspan"), "text"); }
    function Ui(t) { return "data:image/svg+xml;charset=utf-8,".concat(encodeURIComponent(t)); }
    function wr(t, e) { var n = "", r = 0, i = t.toLowerCase(), o = "<" + e, s = "</" + e; for (; r < t.length;) {
        var l = i.indexOf(o, r);
        if (l < 0) {
            n += t.slice(r);
            break;
        }
        n += t.slice(r, l);
        var a = i.indexOf(s, l + o.length);
        if (a < 0)
            break;
        var d = t.indexOf(">", a + s.length);
        r = d < 0 ? t.length : d + 1;
    } return n; }
    function Mr(t) { var $, A, H, N; var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.theme, l = t.requestRerender, a = (A = ($ = e.attrs) == null ? void 0 : $.alt) != null ? A : "", d = (N = (H = e.attrs) == null ? void 0 : H.src) != null ? N : "", p = d.trim().length > 0, m = a.trim().length > 0 ? a : d.trim().length > 0 ? d : "img", x = r.image, c = p ? yr(d, l) : null; if ((c == null ? void 0 : c.state) === "ready" && c.texId > 0 && typeof x == "function") {
        x.call(r, c.texId, 0, 0, Math.max(0, i), Math.max(0, o));
        var k = n.getChildByLabel ? n.getChildByLabel("__label") : n.children.find(function (E) { return (E == null ? void 0 : E.label) === "__label"; });
        k && (k.visible = !1);
        return;
    } var T = p ? Hi(d) : null, g = $i(T != null ? T : p ? gr(i, o) : br({ ring: 34, core: 14 })), b = St(n, "__svg"), _ = yr(Ui(g), l); if ((_ == null ? void 0 : _.state) === "ready" && _.texId > 0 && typeof b.image == "function") {
        var k = "texture:".concat(_.texId, ":").concat(Math.round(i), "x").concat(Math.round(o));
        if (b.__key !== k && (Et(b), b.image(_.texId, 0, 0, Math.max(0, i), Math.max(0, o)), b.__key = k), b.scale.set(1), b.position.set(0, 0), !p) {
            var E = n.getChildByLabel ? n.getChildByLabel("__label") : n.children.find(function (W) { return (W == null ? void 0 : W.label) === "__label"; });
            E && (E.visible = !1);
            return;
        }
        if (m.trim().length > 0) {
            var E = Pt(n, "__label", function (W) { W.style = { fontFamily: s.fontFamily, fontSize: s.fontSize, fill: s.mutedText, fontWeight: "400", wordWrap: !0, wordWrapWidth: 0 }; });
            E.text = m, E.style.wordWrapWidth = Math.max(0, Math.ceil(i - 16) + Me), E.position.set(8, 8 + xt), E.visible = !0;
        }
        return;
    }
    else
        Et(b); var w = b.svg; if (0 && b.__key !== k)
        try { }
        catch (W) { } r.rect(.5, .5, Math.max(0, i - 1), Math.max(0, o - 1)), r.fill(16185078), r.stroke({ width: 1, color: s.control.border }); var M = Pt(n, "__label", function (k) { k.style = { fontFamily: s.fontFamily, fontSize: s.fontSize, fill: s.mutedText, fontWeight: "400", wordWrap: !0, wordWrapWidth: 0 }; }); M.text = m, M.style.wordWrapWidth = Math.max(0, Math.ceil(i - 16) + Me), M.position.set(8, 8 + xt); }
    function Er(t, e, n) { var d, p, m, x; t.setPadding(n.EDGE_LEFT, 0), t.setPadding(n.EDGE_RIGHT, 0), t.setPadding(n.EDGE_TOP, 0), t.setPadding(n.EDGE_BOTTOM, 0); var r = Number((p = (d = e.attrs) == null ? void 0 : d.width) != null ? p : "0"), i = Number((x = (m = e.attrs) == null ? void 0 : m.height) != null ? x : "0"), o = Number.isFinite(r) && r > 0, s = Number.isFinite(i) && i > 0, l = o ? r : 240, a = s ? i : 140; (o || s) && (t.setAlignSelf(n.ALIGN_FLEX_START), t.setFlexGrow(0), t.setFlexShrink(0)), t.setWidth(l), t.setHeight(a), t.setMinWidth(120), t.setMinHeight(80); }
    var kr = new Map;
    function Ne() { var t = globalThis; return !0; }
    function Pr(t) { var e = globalThis, n = String(t != null ? t : "").trim(); if (!n)
        return null; var r = e.__TRUEOS_READY_IMAGE_TEXTURE_CACHE__; (!r || typeof r.get != "function" || typeof r.set != "function") && (r = new Map, e.__TRUEOS_READY_IMAGE_TEXTURE_CACHE__ = r); var i = r.get(n); if (i)
        return i; var o = e.__trueosResolveReadyImageTexture; if (typeof o != "function")
        return null; var s = o(n); if (s && typeof s.then == "function") {
        var l = s.then(function (a) { return (r.set(n, a), a); }).catch(function (a) { throw r.delete(n), a; });
        return r.set(n, l), l;
    } return r.set(n, s), s; }
    function Xi(t) { var s; var e = globalThis, n = String(t != null ? t : "").trim(); if (!n)
        return null; var r = e.__trueosPeekReadyImageTexture; if (typeof r != "function")
        return null; var i = r(n); return !i || typeof i.then == "function" ? null : Math.max(0, Number((s = i == null ? void 0 : i.texId) != null ? s : 0) | 0) > 0 ? i : null; }
    function Yi(t, e) { var n = globalThis, r = String(t != null ? t : "").trim(); if (!r || !e)
        return; var i = n.__TRUEOS_READY_IMAGE_TEXTURE_CACHE__; (!i || typeof i.get != "function" || typeof i.set != "function") && (i = new Map, n.__TRUEOS_READY_IMAGE_TEXTURE_CACHE__ = i), i.set(r, e); }
    function Ir(t, e) { var r, i, o, s, l; var n = Math.max(0, Number((r = e == null ? void 0 : e.texId) != null ? r : 0) | 0); if (n <= 0)
        throw new Error("svg texture missing"); t.state = "ready", t.texId = n, t.width = Math.max(0, Number((o = (i = e == null ? void 0 : e.pixelWidth) != null ? i : e == null ? void 0 : e.width) != null ? o : 0) | 0), t.height = Math.max(0, Number((l = (s = e == null ? void 0 : e.pixelHeight) != null ? s : e == null ? void 0 : e.height) != null ? l : 0) | 0); }
    function Ki(t, e) { var n = Xi(t) || Pr(t); return !n || typeof n.then == "function" ? !1 : (Ir(e, n), Yi(t, n), !0); }
    function zi(t) { return Sr(Sr(String(t), "tspan"), "text"); }
    function Sr(t, e) { var n = "", r = 0, i = t.toLowerCase(), o = "<" + e, s = "</" + e; for (; r < t.length;) {
        var l = i.indexOf(o, r);
        if (l < 0) {
            n += t.slice(r);
            break;
        }
        n += t.slice(r, l);
        var a = i.indexOf(s, l + o.length);
        if (a < 0)
            break;
        var d = t.indexOf(">", a + s.length);
        r = d < 0 ? t.length : d + 1;
    } return n; }
    function Cr(t) { var e = String(t), r = e.toLowerCase().indexOf("viewbox"); if (r < 0)
        return null; var i = e.indexOf("=", r + 7); if (i < 0)
        return null; var o = i + 1; for (; o < e.length;) {
        var c = e.charCodeAt(o);
        if (c !== 32 && c !== 9 && c !== 10 && c !== 13 && c !== 12)
            break;
        o += 1;
    } var s = e.charAt(o); if (s !== '"' && s !== "'")
        return null; var l = e.indexOf(s, o + 1); if (l < 0)
        return null; var a = ji(e.slice(o + 1, l)); if (a.length < 4)
        return null; var d = Number(a[0]), p = Number(a[1]), m = Number(a[2]), x = Number(a[3]); return ![d, p, m, x].every(function (c) { return Number.isFinite(c); }) || m <= 0 || x <= 0 ? null : { minX: d, minY: p, w: m, h: x }; }
    function ji(t) { var e = [], n = ""; for (var r = 0; r < t.length; r += 1) {
        var i = t.charCodeAt(r);
        i === 32 || i === 9 || i === 10 || i === 13 || i === 12 ? n.length > 0 && (e.push(n), n = "") : n += t.charAt(r);
    } return n.length > 0 && e.push(n), e; }
    function Vi(t, e) { var n = String(t != null ? t : ""); if (!n.trim())
        return null; var r = kr.get(n), i = "data:image/svg+xml;charset=utf-8,".concat(encodeURIComponent(n)); if (r) {
        if (Ne() && r.state === "loading")
            try {
                Ki(i, r);
            }
            catch (a) {
                r.state = "error";
            }
        return r;
    } if (Ne())
        return null; var o = { state: "loading", texId: 0, width: 0, height: 0 }; kr.set(n, o); var s = function (a) { Ir(o, a), Ne() || e == null || e(); }, l = function () { o.state = "error", Ne() || e == null || e(); }; try {
        var a = Pr(i);
        if (!a)
            return o;
        if (a && typeof a.then == "function") {
            if (Ne())
                return o;
            a.then(s).catch(l);
        }
        else
            s(a);
    }
    catch (a) {
        l();
    } return o; }
    function Ji(t, e, n) { var r = Math.max(0, e), i = Math.max(0, n), o = Cr(t); if (!o || r <= 0 || i <= 0)
        return { x: 0, y: 0, w: r, h: i }; var s = r / o.w, l = i / o.h, a = Math.min(s, l), d = Math.max(0, o.w * a), p = Math.max(0, o.h * a); return { x: Math.max(0, (r - d) / 2), y: Math.max(0, (i - p) / 2), w: d, h: p }; }
    function Or(t, e, n) { var d, p, m, x; t.setPadding(n.EDGE_LEFT, 0), t.setPadding(n.EDGE_RIGHT, 0), t.setPadding(n.EDGE_TOP, 0), t.setPadding(n.EDGE_BOTTOM, 0); var r = Number((p = (d = e.attrs) == null ? void 0 : d.width) != null ? p : "0"), i = Number((x = (m = e.attrs) == null ? void 0 : m.height) != null ? x : "0"), o = Number.isFinite(r) && r > 0, s = Number.isFinite(i) && i > 0, l = o ? r : 300, a = s ? i : 150; (o || s) && (t.setAlignSelf(n.ALIGN_FLEX_START), t.setFlexGrow(0), t.setFlexShrink(0)), t.setWidth(l), t.setHeight(a), t.setMinWidth(Math.min(120, l)), t.setMinHeight(Math.min(80, a)); }
    function Rr(t) { var e = t.svgMarkup, n = t.container, r = t.w, i = t.h, o = t.requestRerender, s = zi(e), l = St(n, "__svg"), a = l.__svgString, d = l.__w, p = l.__h, m = a !== s, x = Vi(s, o); if (l.scale.set(1), l.position.set(0, 0), (x == null ? void 0 : x.state) === "ready" && x.texId > 0 && typeof l.image == "function") {
        if (m || d !== r || p !== i || l.__texId !== x.texId) {
            var T = Ji(s, r, i);
            Et(l), l.image(x.texId, T.x, T.y, T.w, T.h), l.__svgString = s, l.__w = r, l.__h = i, l.__texId = x.texId;
        }
        return;
    } Et(l); return; if (typeof c == "function") {
        if (m || d !== r || p !== i) {
            Et(l);
            var g = void 0;
            try {
                g = c.call(l, s);
            }
            catch (b) {
                g = null;
            }
            g && typeof g.then == "function" && g.then(function () { return o == null ? void 0 : o(); }).catch(function () { }), l.__svgString = s, l.__w = r, l.__h = i;
        }
        var T = Cr(s);
        if (T) {
            var g = r / T.w, b = i / T.h, _ = Math.min(g, b), w = T.w * _, M = T.h * _;
            l.scale.set(_), l.position.set(-T.minX * _ + (r - w) / 2, -T.minY * _ + (i - M) / 2);
        }
        return;
    } }
    function Dr(t, e, n) { var d, p, m, x; t.setPadding(n.EDGE_LEFT, 0), t.setPadding(n.EDGE_RIGHT, 0), t.setPadding(n.EDGE_TOP, 0), t.setPadding(n.EDGE_BOTTOM, 0); var r = Number((p = (d = e.attrs) == null ? void 0 : d.width) != null ? p : "0"), i = Number((x = (m = e.attrs) == null ? void 0 : m.height) != null ? x : "0"), o = Number.isFinite(r) && r > 0, s = Number.isFinite(i) && i > 0, l = o ? r : 300, a = s ? i : 150; (o || s) && (t.setAlignSelf(n.ALIGN_FLEX_START), t.setFlexGrow(0), t.setFlexShrink(0)), t.setWidth(l), t.setHeight(a), t.setMinWidth(Math.min(120, l)), t.setMinHeight(Math.min(80, a)); }
    function Ar(t) { var e = t.graphics, n = t.container, r = t.w, i = t.h, o = t.theme, s = 1, l = s / 2; e.rect(l, l, Math.max(0, r - s), Math.max(0, i - s)), e.fill(16777215), e.stroke({ width: s, color: o.control.border, alignment: 0 }), e.moveTo(6, i - 6), e.lineTo(r - 6, 6), e.stroke({ width: 1, color: 0, alpha: .1 }); var a = zt({ text: "canvas", fontFamily: o.fontFamily, fontSize: Math.max(10, Math.floor(o.fontSize * .85)), fill: o.mutedText, wordWrap: !1 }); a.position.set(8, 8 + xt), n.addChild(a); }
    function vr(t, e, n) { var p, m, x, c, T, g; var r = String((m = (p = e.attrs) == null ? void 0 : p["data-root"]) != null ? m : "") === "1"; if (t.setFlexDirection(n.FLEX_DIRECTION_COLUMN), t.setAlignItems(n.ALIGN_STRETCH), r) {
        t.setPadding(n.EDGE_LEFT, 0), t.setPadding(n.EDGE_RIGHT, 0), t.setPadding(n.EDGE_TOP, 0), t.setPadding(n.EDGE_BOTTOM, 0), t.setAlignSelf(n.ALIGN_STRETCH), t.setFlexGrow(1), t.setFlexShrink(1), t.setMinWidth(0), t.setMinHeight(0);
        return;
    } t.setPadding(n.EDGE_LEFT, 8), t.setPadding(n.EDGE_RIGHT, 8), t.setPadding(n.EDGE_BOTTOM, 8), t.setPadding(n.EDGE_TOP, 34); var i = Number((c = (x = e.attrs) == null ? void 0 : x.width) != null ? c : "0"), o = Number((g = (T = e.attrs) == null ? void 0 : T.height) != null ? g : "0"), s = Number.isFinite(i) && i > 0, l = Number.isFinite(o) && o > 0, a = s ? i : 420, d = l ? o : 240; (s || l) && (t.setAlignSelf(n.ALIGN_FLEX_START), t.setFlexGrow(0), t.setFlexShrink(0)), t.setWidth(a), t.setHeight(d), t.setMinWidth(Math.min(200, a)), t.setMinHeight(Math.min(160, d)); }
    function Nr(t) { var c, T, g, b; var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.theme; if (String((T = (c = e.attrs) == null ? void 0 : c["data-root"]) != null ? T : "") === "1")
        return; var a = 1, d = a / 2; r.rect(d, d, Math.max(0, i - a), Math.max(0, o - a)), r.fill(16777215), r.stroke({ width: a, color: s.control.border, alignment: 0 }), r.rect(d, d, Math.max(0, i - a), 26), r.fill({ color: 0, alpha: .04 }); var m = String((b = (g = e.attrs) == null ? void 0 : g.srcdoc) != null ? b : "").trim().length > 0 ? "srcdoc" : "empty", x = Pt(n, "__title", function (_) { _.style = { fontFamily: s.fontFamily, fontSize: Math.max(10, Math.floor(s.fontSize * .85)), fill: s.mutedText, fontWeight: "400", wordWrap: !1 }; }); x.text = "iframe (".concat(m, ")"), x.position.set(8, 6 + xt), n.eventMode = "static", n.cursor = "default", n.hitArea = new gt(0, 0, Math.max(0, i), Math.max(0, o)); }
    function Gr(t, e, n) { var i, o; var r = ((o = (i = e.attrs) == null ? void 0 : i.type) != null ? o : "text").toLowerCase(); r === "checkbox" || r === "radio" ? (t.setPadding(n.EDGE_LEFT, 0), t.setPadding(n.EDGE_RIGHT, 0), t.setPadding(n.EDGE_TOP, 0), t.setPadding(n.EDGE_BOTTOM, 0), t.setWidth(16), t.setHeight(16), t.setMinWidth(16), t.setMargin(n.EDGE_RIGHT, 6)) : (t.setPadding(n.EDGE_TOP, 6), t.setPadding(n.EDGE_BOTTOM, 6), t.setHeight(36), t.setMinHeight(36), t.setMinWidth(220)); }
    function Lr(t) {
        var e_6, _a, e_7, _b;
        var U, G, h, P, F, q, Z, tt, Y, j;
        var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.absX, l = t.absY, a = t.theme, d = t.textMeasure, p = t.uiState, m = t.getOrInitInputState, x = t.clamp, c = t.radioGroups, T = t.textDrags, g = t.requestPaint, b = ((G = (U = e.attrs) == null ? void 0 : U.type) != null ? G : "text").toLowerCase(), _ = e.key, w = _ ? m(_, e.attrs) : void 0, M = (h = t.showCaret) != null ? h : !1, $ = (P = t.caretPointerId) != null ? P : null, A = t.focusColor, H = t.getCursorColor;
        n.sortableChildren = !0;
        try {
            for (var _c = __values(n.children), _d = _c.next(); !_d.done; _d = _c.next()) {
                var v = _d.value;
                var f = v.label;
                f && (f.startsWith("__sel:") || f === "__caret") && (v.visible = !1);
            }
        }
        catch (e_6_1) { e_6 = { error: e_6_1 }; }
        finally {
            try {
                if (_d && !_d.done && (_a = _c.return)) _a.call(_c);
            }
            finally { if (e_6) throw e_6.error; }
        }
        var N = 8, k = 6 + xt, E = 5, W = a.fontSize * 1.25;
        if (b === "checkbox")
            r.rect(.5, .5, Math.max(0, i - 1), Math.max(0, o - 1)), r.fill(a.control.background), r.stroke({ width: 1, color: a.control.border }), w != null && w.indeterminate ? (r.moveTo(4, 4), r.lineTo(Math.max(4, i - 4), Math.max(4, o - 4)), r.stroke({ width: 2, color: a.control.accent }), r.moveTo(Math.max(4, i - 4), 4), r.lineTo(4, Math.max(4, o - 4)), r.stroke({ width: 2, color: a.control.accent })) : w != null && w.checked && (r.rect(3, 3, Math.max(0, i - 3 * 2), Math.max(0, o - 3 * 2)), r.fill(a.control.accent));
        else if (b === "radio") {
            {
                var I = Math.max(0, Math.min(i, o) / 2 - .5);
                r.circle(i / 2, o / 2, I), r.fill(a.control.background), r.stroke({ width: 1, color: a.control.border });
            }
            if (w != null && w.checked) {
                var v = Math.max(0, Math.min(i, o) / 2 - 4.5);
                r.circle(i / 2, o / 2, v), r.fill(a.control.accent);
            }
        }
        else {
            var v = A != null ? 2 : 1, f = v / 2;
            a.control.radius > 0 ? r.roundRect(f, f, Math.max(0, i - v), Math.max(0, o - v), a.control.radius) : r.rect(f, f, Math.max(0, i - v), Math.max(0, o - v)), r.fill(a.control.background), r.stroke({ width: v, color: A != null ? A : a.control.border });
            var I = b === "password" ? "\u2022".repeat(((F = w == null ? void 0 : w.value) != null ? F : "").length) : (q = w == null ? void 0 : w.value) != null ? q : "", O = Math.max(0, i - N * 2);
            _ && p.fieldBounds.set(_, { x: s, y: l, w: i, h: o, innerLeft: N, innerTop: k, innerWidth: O, maxLines: E, isPassword: b === "password" });
            var C = ue(I, O, d), y = de(C, E), S = y.length > 0 ? y[y.length - 1].end : 0;
            if (_ && w && typeof w.value == "string") {
                var B = w.selections;
                if (B && B.size > 0)
                    try {
                        for (var _f = __values(B.entries()), _g = _f.next(); !_g.done; _g = _f.next()) {
                            var _h = __read(_g.value, 2), X = _h[0], z = _h[1];
                            var K = x((Z = z.start) != null ? Z : 0, 0, I.length), V = x((tt = z.end) != null ? tt : K, 0, I.length), nt = x(Math.min(K, V), 0, S), Q = x(Math.max(K, V), 0, S);
                            if (nt === Q)
                                continue;
                            var it = St(n, "__sel:".concat(X));
                            Et(it), it.zIndex = 0, it.visible = !0;
                            for (var st = 0; st < y.length; st++) {
                                var dt = y[st], yt = Math.max(nt, dt.start), Rt = Math.min(Q, dt.end);
                                if (yt >= Rt)
                                    continue;
                                var Gt = N + d(I.slice(dt.start, yt)), at = d(I.slice(yt, Rt));
                                it.rect(Gt, k + st * W, at, W);
                            }
                            it.fill({ color: H(X), alpha: .22 });
                        }
                    }
                    catch (e_7_1) { e_7 = { error: e_7_1 }; }
                    finally {
                        try {
                            if (_g && !_g.done && (_b = _f.return)) _b.call(_f);
                        }
                        finally { if (e_7) throw e_7.error; }
                    }
                if (M && $ != null) {
                    var X = (Y = w.selections) == null ? void 0 : Y.get($), z = X ? X.end : 0, K = x(z, 0, S), V = Math.max(0, y.length - 1);
                    for (var st = 0; st < y.length; st++) {
                        var dt = y[st];
                        if (K >= dt.start && K <= dt.end) {
                            V = st;
                            break;
                        }
                    }
                    var nt = (j = y[V]) != null ? j : { start: 0, end: 0, text: "" }, Q = N + d(I.slice(nt.start, K)), it = St(n, "__caret");
                    Et(it), it.zIndex = 2, it.visible = !0, it.moveTo(Q, k + V * W), it.lineTo(Q, k + V * W + W), it.stroke({ width: 1, color: A != null ? A : a.control.focusBorder });
                }
            }
            var D = y.map(function (B) { return B.text; }).join("\n"), R = Pt(n, "__valueText", function (B) { B.style = { fontFamily: a.fontFamily, fontSize: a.fontSize, fill: a.text, fontWeight: "400", wordWrap: !1 }, B.zIndex = 1; });
            R.text = D, R.position.set(N, k);
        }
        _ && ($t(n), n.eventMode = "static", n.cursor = "text", n.on("pointerdown", function (v) {
            var e_8, _a, e_9, _b, e_10, _c;
            var I, O, C, y, S, D, R, B, X, z, K, V, nt;
            if ((v == null ? void 0 : v.button) === 2)
                return;
            var f = t.getPointerId ? t.getPointerId(v) : Number((C = (O = v == null ? void 0 : v.pointerId) != null ? O : (I = v == null ? void 0 : v.data) == null ? void 0 : I.pointerId) != null ? C : 0);
            if (!(f <= 0)) {
                if (p.focusedKeyByPointer.set(f, _), p.keyboardOwnerPointerId = f, b === "checkbox") {
                    var Q = m(_, e.attrs), it = Q.indeterminate === !0, st = Q.checked === !0;
                    !st && !it ? (Q.checked = !0, Q.indeterminate = !1) : st && !it ? (Q.checked = !1, Q.indeterminate = !0) : (Q.checked = !1, Q.indeterminate = !1);
                }
                else if (b === "radio") {
                    var it = "radio:".concat((S = (y = e.attrs) == null ? void 0 : y.name) != null ? S : "__default__"), st = (D = c.get(it)) != null ? D : [];
                    try {
                        for (var st_1 = __values(st), st_1_1 = st_1.next(); !st_1_1.done; st_1_1 = st_1.next()) {
                            var dt = st_1_1.value;
                            var yt = m(dt, void 0);
                            yt.checked = dt === _;
                        }
                    }
                    catch (e_8_1) { e_8 = { error: e_8_1 }; }
                    finally {
                        try {
                            if (st_1_1 && !st_1_1.done && (_a = st_1.return)) _a.call(st_1);
                        }
                        finally { if (e_8) throw e_8.error; }
                    }
                }
                else {
                    var Q = m(_, e.attrs);
                    if (typeof Q.value == "string") {
                        try {
                            for (var _d = __values(p.inputs.entries()), _f = _d.next(); !_f.done; _f = _d.next()) {
                                var _g = __read(_f.value, 2), Mt = _g[0], Dt = _g[1];
                                Mt !== _ && ((R = Dt.selections) == null || R.delete(f));
                            }
                        }
                        catch (e_9_1) { e_9 = { error: e_9_1 }; }
                        finally {
                            try {
                                if (_f && !_f.done && (_b = _d.return)) _b.call(_d);
                            }
                            finally { if (e_9) throw e_9.error; }
                        }
                        var it = b === "password" ? "\u2022".repeat(Q.value.length) : Q.value, st = p.fieldBounds.get(_), dt = (B = st == null ? void 0 : st.innerWidth) != null ? B : Math.max(0, i - N * 2), yt = de(ue(it, dt, d), E), Rt = ((z = (X = v.global) == null ? void 0 : X.x) != null ? z : 0) - s - N, Gt = ((V = (K = v.global) == null ? void 0 : K.y) != null ? V : 0) - l - k, at = Ee({ fullText: it, lines: yt, localX: Rt, localY: Gt, lineHeight: W, measure: d });
                        Q.selections || (Q.selections = new Map), Q.selections.set(f, { start: at, end: at });
                        try {
                            for (var _h = __values(T.entries()), _j = _h.next(); !_j.done; _j = _h.next()) {
                                var _k = __read(_j.value, 2), Mt = _k[0], Dt = _k[1];
                                Dt.key === _ && Mt !== f && T.delete(Mt);
                            }
                        }
                        catch (e_10_1) { e_10 = { error: e_10_1 }; }
                        finally {
                            try {
                                if (_j && !_j.done && (_c = _h.return)) _c.call(_h);
                            }
                            finally { if (e_10) throw e_10.error; }
                        }
                        T.set(f, { key: _, anchor: at });
                    }
                }
                (b === "checkbox" || b === "radio") && ((nt = v.stopPropagation) == null || nt.call(v)), g == null || g();
            }
        }), (b === "checkbox" || b === "radio") && (n.cursor = "pointer"), n.hitArea = new gt(0, 0, Math.max(0, i), Math.max(0, o)));
    }
    function Fr(t, e) { t.setPadding(e.EDGE_TOP, 6), t.setPadding(e.EDGE_BOTTOM, 6), t.setHeight(108), t.setMinHeight(108), t.setMinWidth(220); }
    function Br(t) {
        var e_11, _a, e_12, _b;
        var tt, Y, j, v, f, I, O, C;
        var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.absX, l = t.absY, a = t.theme, d = t.textMeasure, p = t.uiState, m = t.getOrInitInputState, x = t.clamp, c = t.textDrags, T = t.requestPaint, g = e.key, b = g ? m(g, ge(re({}, (tt = e.attrs) != null ? tt : {}), { type: "text" })) : void 0, _ = (Y = t.showCaret) != null ? Y : !1, w = (j = t.caretPointerId) != null ? j : null, M = t.focusColor, $ = t.getCursorColor;
        n.sortableChildren = !0;
        try {
            for (var _c = __values(n.children), _d = _c.next(); !_d.done; _d = _c.next()) {
                var y = _d.value;
                var S = y.label;
                S && (S.startsWith("__sel:") || S === "__caret") && (y.visible = !1);
            }
        }
        catch (e_11_1) { e_11 = { error: e_11_1 }; }
        finally {
            try {
                if (_d && !_d.done && (_a = _c.return)) _a.call(_c);
            }
            finally { if (e_11) throw e_11.error; }
        }
        var A = 8, H = 6 + xt, N = 5, k = a.fontSize * 1.25, E = M != null ? 2 : 1, W = E / 2;
        a.control.radius > 0 ? r.roundRect(W, W, Math.max(0, i - E), Math.max(0, o - E), a.control.radius) : r.rect(W, W, Math.max(0, i - E), Math.max(0, o - E)), r.fill(a.control.background), r.stroke({ width: E, color: M != null ? M : a.control.border });
        var U = (v = b == null ? void 0 : b.value) != null ? v : "", G = Math.max(0, i - A * 2);
        g && p.fieldBounds.set(g, { x: s, y: l, w: i, h: o, innerLeft: A, innerTop: H, innerWidth: G, maxLines: N, isPassword: !1 });
        var h = ue(U, G, d), P = de(h, N), F = P.length > 0 ? P[P.length - 1].end : 0;
        if (g && b && typeof b.value == "string") {
            var y = b.selections;
            if (y && y.size > 0)
                try {
                    for (var _f = __values(y.entries()), _g = _f.next(); !_g.done; _g = _f.next()) {
                        var _h = __read(_g.value, 2), S = _h[0], D = _h[1];
                        var R = x((f = D.start) != null ? f : 0, 0, U.length), B = x((I = D.end) != null ? I : R, 0, U.length), X = x(Math.min(R, B), 0, F), z = x(Math.max(R, B), 0, F);
                        if (X === z)
                            continue;
                        var K = St(n, "__sel:".concat(S));
                        Et(K), K.zIndex = 0, K.visible = !0;
                        for (var V = 0; V < P.length; V++) {
                            var nt = P[V], Q = Math.max(X, nt.start), it = Math.min(z, nt.end);
                            if (Q >= it)
                                continue;
                            var st = A + d(U.slice(nt.start, Q)), dt = d(U.slice(Q, it));
                            K.rect(st, H + V * k, dt, k);
                        }
                        K.fill({ color: $(S), alpha: .22 });
                    }
                }
                catch (e_12_1) { e_12 = { error: e_12_1 }; }
                finally {
                    try {
                        if (_g && !_g.done && (_b = _f.return)) _b.call(_f);
                    }
                    finally { if (e_12) throw e_12.error; }
                }
            if (_ && w != null) {
                var S = (O = b.selections) == null ? void 0 : O.get(w), D = S ? S.end : 0, R = x(D, 0, F), B = Math.max(0, P.length - 1);
                for (var V = 0; V < P.length; V++) {
                    var nt = P[V];
                    if (R >= nt.start && R <= nt.end) {
                        B = V;
                        break;
                    }
                }
                var X = (C = P[B]) != null ? C : { start: 0, end: 0, text: "" }, z = A + d(U.slice(X.start, R)), K = St(n, "__caret");
                Et(K), K.zIndex = 2, K.visible = !0, K.moveTo(z, H + B * k), K.lineTo(z, H + B * k + k), K.stroke({ width: 1, color: M != null ? M : a.control.focusBorder });
            }
        }
        var q = P.map(function (y) { return y.text; }).join("\n"), Z = Pt(n, "__valueText", function (y) { y.style = { fontFamily: a.fontFamily, fontSize: a.fontSize, fill: a.text, fontWeight: "400", wordWrap: !1 }, y.zIndex = 1; });
        Z.text = q, Z.position.set(A, H), g && ($t(n), n.eventMode = "static", n.cursor = "text", n.hitArea = new gt(0, 0, Math.max(0, i), Math.max(0, o)), n.on("pointerdown", function (y) {
            var e_13, _a, e_14, _b;
            var R, B, X, z, K, V, nt, Q, it, st;
            if ((y == null ? void 0 : y.button) === 2)
                return;
            var S = t.getPointerId ? t.getPointerId(y) : Number((X = (B = y == null ? void 0 : y.pointerId) != null ? B : (R = y == null ? void 0 : y.data) == null ? void 0 : R.pointerId) != null ? X : 0);
            if (S <= 0)
                return;
            p.focusedKeyByPointer.set(S, g), p.keyboardOwnerPointerId = S;
            var D = m(g, ge(re({}, (z = e.attrs) != null ? z : {}), { type: "text" }));
            if (typeof D.value == "string") {
                try {
                    for (var _c = __values(p.inputs.entries()), _d = _c.next(); !_d.done; _d = _c.next()) {
                        var _f = __read(_d.value, 2), Ct = _f[0], vt = _f[1];
                        Ct !== g && ((K = vt.selections) == null || K.delete(S));
                    }
                }
                catch (e_13_1) { e_13 = { error: e_13_1 }; }
                finally {
                    try {
                        if (_d && !_d.done && (_a = _c.return)) _a.call(_c);
                    }
                    finally { if (e_13) throw e_13.error; }
                }
                var dt = p.fieldBounds.get(g), yt = (V = dt == null ? void 0 : dt.innerWidth) != null ? V : Math.max(0, i - A * 2), Rt = D.value, Gt = de(ue(Rt, yt, d), N), at = ((Q = (nt = y.global) == null ? void 0 : nt.x) != null ? Q : 0) - s - A, Mt = ((st = (it = y.global) == null ? void 0 : it.y) != null ? st : 0) - l - H, Dt = Ee({ fullText: Rt, lines: Gt, localX: at, localY: Mt, lineHeight: k, measure: d });
                D.selections || (D.selections = new Map), D.selections.set(S, { start: Dt, end: Dt });
                try {
                    for (var _g = __values(c.entries()), _h = _g.next(); !_h.done; _h = _g.next()) {
                        var _j = __read(_h.value, 2), Ct = _j[0], vt = _j[1];
                        vt.key === g && Ct !== S && c.delete(Ct);
                    }
                }
                catch (e_14_1) { e_14 = { error: e_14_1 }; }
                finally {
                    try {
                        if (_h && !_h.done && (_b = _g.return)) _b.call(_g);
                    }
                    finally { if (e_14) throw e_14.error; }
                }
                c.set(S, { key: g, anchor: Dt });
            }
            T == null || T();
        }));
    }
    function Wr(t, e) { t.setFlexDirection(e.FLEX_DIRECTION_ROW), t.setAlignItems(e.ALIGN_CENTER), t.setJustifyContent(e.JUSTIFY_FLEX_START), t.setPadding(e.EDGE_LEFT, 8), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0); }
    function Zi(t, e, n, r, i) { t.circle(e, n, r), t.stroke({ width: 2, color: i }); var o = e + r * .65, s = n + r * .65, l = e + r * 1.55, a = n + r * 1.55; t.moveTo(o, s), t.lineTo(l, a), t.stroke({ width: 2, color: i }); }
    function Hr(t, e) { t.setFlexDirection(e.FLEX_DIRECTION_ROW), t.setFlexWrap(e.WRAP_NO_WRAP), t.setAlignItems(e.ALIGN_CENTER), t.setJustifyContent(e.JUSTIFY_FLEX_START), t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0); }
    function $r(t, e) { t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0), t.setWidth(36), t.setHeight(36), t.setMinWidth(36), t.setMinHeight(36), t.setFlexGrow(0), t.setFlexShrink(0), t.setMargin(e.EDGE_RIGHT, 6); }
    function Ur(t) { var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.theme, l = t.uiState, a = t.getPointerId, d = t.focusInputKey, p = t.requestPaint, m = function (c) { r.clear(); var T = 1, g = T / 2; s.control.button.radius > 0 ? r.roundRect(g, g, Math.max(0, i - T), Math.max(0, o - T), s.control.button.radius) : r.rect(g, g, Math.max(0, i - T), Math.max(0, o - T)), r.fill(c), r.stroke({ width: T, color: s.control.button.border }); var b = i / 2 - 2, _ = o / 2 - 2, w = Math.max(5, Math.min(7, Math.min(i, o) * .22)); Zi(r, b, _, w, s.text); }; m(s.control.button.fill), $t(n), n.eventMode = "static", n.cursor = "pointer", n.hitArea = new gt(0, 0, Math.max(0, i), Math.max(0, o)), n.on("pointerover", function () { return m(s.control.button.hoverFill); }), n.on("pointerout", function () { return m(s.control.button.fill); }), n.on("pointerdown", function (c) { var T; if ((c == null ? void 0 : c.button) !== 2) {
        if (m(s.control.button.activeFill), d) {
            var g = a(c);
            g > 0 && (l.focusedKeyByPointer.set(g, d), l.keyboardOwnerPointerId = g);
        }
        p == null || p(), (T = c.stopPropagation) == null || T.call(c);
    } }), n.on("pointerup", function () { return m(s.control.button.hoverFill); }); var x = e.attrs; }
    function Qe(t, e) { var n = t.get(e); if (n)
        return n; var r = { x: 0, y: 0 }; return t.set(e, r), r; }
    function Xr(t, e) { t.setPositionType(e.POSITION_TYPE_ABSOLUTE), t.setPosition(e.EDGE_LEFT, 0), t.setPosition(e.EDGE_TOP, 0), t.setAlignSelf(e.ALIGN_FLEX_START), t.setFlexGrow(0), t.setFlexShrink(0), t.setPadding(e.EDGE_LEFT, 12), t.setPadding(e.EDGE_RIGHT, 12), t.setPadding(e.EDGE_TOP, 12), t.setPadding(e.EDGE_BOTTOM, 12), t.setWidth(540), t.setMinWidth(360), t.setMinHeight(148); }
    function Yr(t) { var $, A; var e = t.node, n = t.container, r = t.w, i = t.h, o = t.theme, s = t.selectedBy, l = t.getCursorColor, a = t.dialogStates, d = t.dialogDrags, p = t.bringToFront, m = t.requestPaint, x = e.key; if (!x)
        return; var c = s.get(x), T = c == null ? o.boxBorder : l(c), g = Math.max(0, Math.round(r)), b = Math.max(0, Math.round(i)), _ = St(n, "__dialogBorder"); Et(_), _.rect(0, 0, g, b), _.fill({ color: 16777215, alpha: .8 }); var w = c == null ? 1 : 2, M = w / 2; _.rect(M, M, Math.max(0, g - w), Math.max(0, b - w)), _.stroke({ width: w, color: T, alignment: 0 }), _.eventMode = "static", _.cursor = "move", _.hitArea = new gt(0, 0, g, b), _.on("pointerdown", function (H) {
        var e_15, _a;
        var W, U, G, h, P, F, q, Z;
        var N = function (tt) { try {
            typeof console != "undefined" && typeof console.log == "function" && console.log("[dialog pointerdown] ".concat(tt));
        }
        catch (Y) { } };
        if (N("start"), (H == null ? void 0 : H.button) === 2)
            return;
        N("pointer-id");
        var k = t.getPointerId ? t.getPointerId(H) : Number((G = (U = H == null ? void 0 : H.pointerId) != null ? U : (W = H == null ? void 0 : H.data) == null ? void 0 : W.pointerId) != null ? G : 0);
        if (k <= 0 || k <= 0)
            return;
        N("clear-other-drags");
        try {
            for (var _b = __values(d.entries()), _c = _b.next(); !_c.done; _c = _b.next()) {
                var _d = __read(_c.value, 2), tt = _d[0], Y = _d[1];
                Y.key === x && tt !== k && d.delete(tt);
            }
        }
        catch (e_15_1) { e_15 = { error: e_15_1 }; }
        finally {
            try {
                if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
            }
            finally { if (e_15) throw e_15.error; }
        }
        N("select"), s.set(x, k), N("bring-to-front"), p == null || p(x), N("state");
        var E = Qe(a, x);
        N("set-drag"), d.set(k, { key: x, startGX: (P = (h = H.global) == null ? void 0 : h.x) != null ? P : 0, startGY: (q = (F = H.global) == null ? void 0 : F.y) != null ? q : 0, originX: E.x, originY: E.y }), N("request-paint"), m == null || m(), N("stop-propagation"), (Z = H.stopPropagation) == null || Z.call(H), N("done");
    }); {
        var H = n.getChildByLabel, N = (A = ($ = H == null ? void 0 : H.call(n, "__children")) != null ? $ : n.children.find(function (k) { return k && k.label === "__children"; })) != null ? A : null;
        if (N && _.parent === n) {
            var k = n.getChildIndex(N), E = Math.max(0, n.children.length - 1), W = Math.max(0, Math.min(k - 1, E));
            n.getChildIndex(_) > W && n.setChildIndex(_, W);
        }
    } }
    function Tn(t, e, n) { var l; var r = t.get(e); if (r)
        return r; var i = Number((l = n == null ? void 0 : n.value) != null ? l : "0"), s = { value: Number.isFinite(i) ? i : 0 }; return t.set(e, s), s; }
    function Kr(t, e) { t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0), t.setHeight(36), t.setMinHeight(36), t.setMinWidth(140), t.setFlexGrow(0), t.setFlexShrink(0); }
    function Qi(t, e, n) { return Math.max(e, Math.min(n, t)); }
    function _n(t, e, n) { var i; var r = Number((i = t == null ? void 0 : t[e]) != null ? i : ""); return Number.isFinite(r) ? r : n; }
    function qi(t, e, n, r, i, o) { var l = e + 3, a = e + r - 3, d = n + 3, p = n + i - 3; t.moveTo(l, p), t.lineTo((l + a) / 2, d), t.lineTo(a, p), t.stroke({ width: 2, color: o }); }
    function to(t, e, n, r, i, o) { var l = e + 3, a = e + r - 3, d = n + 3, p = n + i - 3; t.moveTo(l, d), t.lineTo((l + a) / 2, p), t.lineTo(a, d), t.stroke({ width: 2, color: o }); }
    function zr(t) { var G; var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.theme, l = t.getValue, a = t.setValue, d = t.requestPaint, p = e.key, m = e.attrs, x = _n(m, "min", 0), c = _n(m, "max", 255), T = Math.max(1e-9, _n(m, "step", 1)), g = l(), b = 1, _ = b / 2; r.rect(_, _, Math.max(0, i - b), Math.max(0, o - b)), r.fill(s.control.background), r.stroke({ width: b, color: s.control.border }); var w = 22, M = Math.max(0, i - w); r.moveTo(M + .5, 0), r.lineTo(M + .5, o), r.stroke({ width: 1, color: s.control.border, alignment: 0 }); var $ = St(n, "__arrows"); Et($), qi($, M, 0, w, o / 2, s.text), to($, M, o / 2, w, o / 2, s.text); var A = ((G = m == null ? void 0 : m.channel) != null ? G : "").toLowerCase(), H = A === "r" ? "R" : A === "g" ? "G" : A === "b" ? "B" : A === "a" ? "A" : "", N = Pt(n, "__valueText", function (h) { h.style = { fontFamily: s.fontFamily, fontSize: s.fontSize, fill: s.text, fontWeight: "400", wordWrap: !1 }; }); if (N.text = H ? "".concat(H, ": ").concat(Math.round(g)) : String(Math.round(g)), N.position.set(8, 9 + xt), !p)
        return; var k = new gt(M, 0, w, o / 2), E = new gt(M, o / 2, w, o / 2), W = function (h) { var P = l(), F = Qi(P + h * T, x, c); a(F), d == null || d(); }, U = St(n, "__hit"); Et(U), U.eventMode = "static", U.cursor = "default", U.hitArea = new gt(0, 0, Math.max(0, i), Math.max(0, o)), U.on("pointerdown", function (h) {
        var e_16, _a;
        var j, v, f, I, O, C;
        if ((h == null ? void 0 : h.button) === 2)
            return;
        var P = t.getPointerId ? t.getPointerId(h) : Number((f = (v = h == null ? void 0 : h.pointerId) != null ? v : (j = h == null ? void 0 : h.data) == null ? void 0 : j.pointerId) != null ? f : 0);
        if (P <= 0)
            return;
        var F = n.toLocal(h.global), q = (I = F == null ? void 0 : F.x) != null ? I : 0, Z = (O = F == null ? void 0 : F.y) != null ? O : 0, tt = k.contains(q, Z) ? 1 : E.contains(q, Z) ? -1 : null;
        if (!tt)
            return;
        W(tt);
        var Y = t.numberHolds;
        if (Y && p) {
            try {
                for (var _b = __values(Y.entries()), _c = _b.next(); !_c.done; _c = _b.next()) {
                    var _d = __read(_c.value, 2), D = _d[0], R = _d[1];
                    D !== P && (R.timeoutId != null && window.clearTimeout(R.timeoutId), R.intervalId != null && window.clearInterval(R.intervalId), Y.delete(D));
                }
            }
            catch (e_16_1) { e_16 = { error: e_16_1 }; }
            finally {
                try {
                    if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
                }
                finally { if (e_16) throw e_16.error; }
            }
            var y = Y.get(P);
            y && (y.timeoutId != null && window.clearTimeout(y.timeoutId), y.intervalId != null && window.clearInterval(y.intervalId));
            var S_1 = { key: p, timeoutId: null, intervalId: null };
            S_1.timeoutId = window.setTimeout(function () { S_1.timeoutId = null, S_1.intervalId = window.setInterval(function () { W(tt); }, 250); }, 500), Y.set(P, S_1);
        }
        (C = h.stopPropagation) == null || C.call(h);
    }); }
    var qe = null;
    function jr() { return qe || (qe = new Ve({ data: ie, label: "attribute-color-picker-colors", shrinkToFit: !1, usage: xn.VERTEX | xn.COPY_DST }), qe); }
    function Vr(t, e, n) { var d, p, m, x; t.setPadding(n.EDGE_LEFT, 0), t.setPadding(n.EDGE_RIGHT, 0), t.setPadding(n.EDGE_TOP, 0), t.setPadding(n.EDGE_BOTTOM, 0); var r = Number((p = (d = e.attrs) == null ? void 0 : d.width) != null ? p : "0"), i = Number((x = (m = e.attrs) == null ? void 0 : m.height) != null ? x : "0"), o = Number.isFinite(r) && r > 0, s = Number.isFinite(i) && i > 0, l = o ? r : 240, a = s ? i : 200; (o || s) && (t.setAlignSelf(n.ALIGN_FLEX_START), t.setFlexGrow(0), t.setFlexShrink(0)), t.setWidth(l), t.setHeight(a), t.setMinWidth(Math.min(240, l)), t.setMinHeight(Math.min(200, a)); }
    function ae(t) { return Number.isFinite(t) ? Math.max(0, Math.min(255, Math.round(t))) : 0; }
    function tn(t) { return ae(t).toString(16).padStart(2, "0"); }
    function eo(t, e, n, r, i, o, s, l) { var a = s - n, d = l - r, p = i - n, m = o - r, x = t - n, c = e - r, T = a * a + d * d, g = a * p + d * m, b = a * x + d * c, _ = p * p + m * m, w = p * x + m * c, M = 1 / (T * _ - g * g), $ = (_ * b - g * w) * M, A = (T * w - g * b) * M; return $ >= 0 && A >= 0 && $ + A <= 1; }
    function no(t, e, n, r, i, o, s, l) { var a = i - n, d = o - r, p = s - n, m = l - r, x = t - n, c = e - r, T = a * m - p * d; if (Math.abs(T) < 1e-9)
        return { w0: 1, w1: 0, w2: 0 }; var g = (x * m - p * c) / T, b = (a * c - x * d) / T; return { w0: 1 - g - b, w1: g, w2: b }; }
    var ro = { name: "solid-out", fragment: { main: "\n      outColor = vec4(1.0);\n    " } }, en = null;
    function io() { if (en)
        return en; var t = { name: "color-picker-vertex-color", bits: [Vn, Jn, jn, ro] }; return en = new Ae({ glProgram: t, resources: {} }), en; }
    function Jr(t, e, n) { var r = new Float32Array(12), i = [-90, -30, 30, 90, 150, 210]; for (var o = 0; o < 6; o++) {
        var s = i[o] * Math.PI / 180;
        r[o * 2 + 0] = t + Math.cos(s) * n, r[o * 2 + 1] = e + Math.sin(s) * n;
    } return r; }
    var ie = new Uint8Array([255, 0, 0, 255, 128, 128, 0, 255, 0, 255, 0, 255, 0, 128, 128, 255, 0, 0, 255, 255, 128, 0, 128, 255]), ke = new Uint32Array([0, 1, 2, 0, 2, 3, 0, 3, 4, 0, 4, 5]);
    function Mn(t) { var e = t.lx, n = t.ly, r = t.w, i = t.h, o = 10, s = Math.max(0, r - o * 2), l = Math.max(0, i - o * 2), a = o + s / 2, d = o + l / 2, p = Math.max(0, Math.min(s, l) / 2 - 2), m = Jr(a, d, p); for (var x = 0; x < ke.length; x += 3) {
        var c = ke[x + 0], T = ke[x + 1], g = ke[x + 2], b = m[c * 2 + 0], _ = m[c * 2 + 1], w = m[T * 2 + 0], M = m[T * 2 + 1], $ = m[g * 2 + 0], A = m[g * 2 + 1];
        if (!eo(e, n, b, _, w, M, $, A))
            continue;
        var H = no(e, n, b, _, w, M, $, A), N = c * 4, k = T * 4, E = g * 4, W = H.w0 * ie[N + 0] + H.w1 * ie[k + 0] + H.w2 * ie[E + 0], U = H.w0 * ie[N + 1] + H.w1 * ie[k + 1] + H.w2 * ie[E + 1], G = H.w0 * ie[N + 2] + H.w1 * ie[k + 2] + H.w2 * ie[E + 2];
        return { r: ae(W), g: ae(U), b: ae(G) };
    } return null; }
    function Zr(t) { var Y, j; var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.theme, l = t.rgb, a = t.setRgb, d = t.alpha, p = t.setAlpha, m = t.pick, x = t.setPick, c = t.requestPaint, T = t.getPointerId, g = t.setDraggingPointerId, b = 1, _ = b / 2; r.rect(_, _, Math.max(0, i - b), Math.max(0, o - b)), r.fill(16777215), r.stroke({ width: b, color: s.control.border, alignment: 0 }); var w = 10, M = Math.max(0, i - w * 2), $ = Math.max(0, o - w * 2), A = w + M / 2, H = w + $ / 2, N = Math.max(0, Math.min(M, $) / 2 - 2), k = Jr(A, H, N), E = "".concat(Math.round(i), "x").concat(Math.round(o)), W = n.getChildByLabel, U = W ? W.call(n, "__mesh") : n.children.find(function (v) { return (v == null ? void 0 : v.label) === "__mesh"; }); if (U) {
        if (U.__sizeKey !== E) {
            var v = new Float32Array(k.length), f = new Te({ positions: k, uvs: v, indices: ke });
            f.addAttribute("aColor", { buffer: jr(), format: "unorm8x4", stride: 4, offset: 0 });
            try {
                (j = (Y = U.geometry) == null ? void 0 : Y.destroy) == null || j.call(Y);
            }
            catch (I) { }
            U.geometry = f, U.__sizeKey = E;
        }
    }
    else {
        var v = new Float32Array(k.length), f = new Te({ positions: k, uvs: v, indices: ke });
        f.addAttribute("aColor", { buffer: jr(), format: "unorm8x4", stride: 4, offset: 0 }), U = new je({ geometry: f, shader: io() }), U.label = "__mesh", n.addChild(U), U.__sizeKey = E;
    } U.removeAllListeners(), U.eventMode = "static", U.cursor = "crosshair", U.hitArea = new gt(w, w, M, $), U.on("pointerdown", function (v) { var S, D, R; if ((v == null ? void 0 : v.button) === 2)
        return; var f = T(v); if (f <= 0)
        return; var I = n.toLocal(v.global), O = (S = I == null ? void 0 : I.x) != null ? S : 0, C = (D = I == null ? void 0 : I.y) != null ? D : 0, y = Mn({ lx: O, ly: C, w: i, h: o }); y && (x({ x: O, y: C }), a(y), g(f), c == null || c(), (R = v.stopPropagation) == null || R.call(v)); }); {
        var v = St(n, "__border");
        Et(v), v.moveTo(k[0], k[1]);
        for (var f = 1; f < 6; f++)
            v.lineTo(k[f * 2 + 0], k[f * 2 + 1]);
        v.closePath(), v.stroke({ width: 2, color: 0 });
    } var G = St(n, "__overlay"); Et(G); var h = 44, P = 18, F = Math.max(w, i - w - h), q = w; G.rect(F, q, h, P), G.fill({ color: ae(l.r) << 16 | ae(l.g) << 8 | ae(l.b), alpha: Math.max(0, Math.min(1, ae(d) / 255)) }), G.rect(F + .5, q + .5, h - 1, P - 1), G.stroke({ width: 1, color: s.control.border, alignment: 0 }), m && (G.circle(m.x, m.y, 4), G.stroke({ width: 2, color: 16777215 }), G.circle(m.x, m.y, 4), G.stroke({ width: 1, color: 0 })); var Z = "#".concat(tn(l.r)).concat(tn(l.g)).concat(tn(l.b)).concat(tn(d)).toUpperCase(), tt = Pt(n, "__label", function (v) { v.style = { fontFamily: s.fontFamily, fontSize: Math.max(10, Math.floor(s.fontSize * .75)), fill: s.mutedText, fontWeight: "400", wordWrap: !1 }; }); tt.text = Z, tt.position.set(w, Math.max(w, o - w - tt.height)), p && p(ae(d)); }
    function he(t, e, n) { var r = t.get(e); if (r)
        return r; var i = { selectedIndex: Math.max(0, n | 0), open: !1 }; return t.set(e, i), i; }
    function Qr(t, e) { t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0), t.setHeight(36), t.setMinHeight(36), t.setMinWidth(220); }
    function oo(t, e, n, r, i, o) { var l = e + 4, a = e + r - 4, d = n + 4, p = n + i - 4; t.moveTo(l, (d + p) / 2 - 2), t.lineTo((l + a) / 2, (d + p) / 2 + 2), t.lineTo(a, (d + p) / 2 - 2), t.stroke({ width: 2, color: o }); }
    function so(t) {
        var r;
        var n = String((r = t == null ? void 0 : t["data-options"]) != null ? r : "").split("\n").map(function (i) { return i.trim(); }).filter(function (i) { return i.length > 0; });
        return n.length > 0 ? n : ["(empty)"];
    }
    function ao(t) { var n; var e = Number((n = t == null ? void 0 : t["data-selected-index"]) != null ? n : "0"); return Number.isFinite(e) ? Math.max(0, e | 0) : 0; }
    function nn(t) { var U; var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.absX, l = t.absY, a = t.theme, d = t.selectStates, p = t.uiState, m = t.getPointerId, x = t.getCursorColor, c = t.requestPaint, T = t.popupSink, g = e.key; if (!g)
        return; var b = so(e.attrs), _ = ao(e.attrs), w = he(d, g, _); w.selectedIndex = Math.max(0, Math.min(b.length - 1, w.selectedIndex | 0)); var M = (function () {
        var e_17, _a;
        var G = p.keyboardOwnerPointerId;
        if (p.focusedKeyByPointer.get(G) === g)
            return G;
        try {
            for (var _b = __values(p.focusedKeyByPointer.entries()), _c = _b.next(); !_c.done; _c = _b.next()) {
                var _d = __read(_c.value, 2), h = _d[0], P = _d[1];
                if (P === g)
                    return h;
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
    })(), $ = M != null ? x(M) : null, A = $ != null ? 2 : 1, H = A / 2; a.control.radius > 0 ? r.roundRect(H, H, Math.max(0, i - A), Math.max(0, o - A), a.control.radius) : r.rect(H, H, Math.max(0, i - A), Math.max(0, o - A)), r.fill(a.control.background), r.stroke({ width: A, color: $ != null ? $ : a.control.border }); var N = 22, k = Math.max(0, i - N); r.moveTo(k + .5, 0), r.lineTo(k + .5, o), r.stroke({ width: 1, color: a.control.border, alignment: 0 }), oo(r, k, 0, N, o, a.text); var E = (U = b[w.selectedIndex]) != null ? U : "", W = Pt(n, "__label", function (G) { G.style = { fontFamily: a.fontFamily, fontSize: a.fontSize, fill: a.text, fontWeight: "400", wordWrap: !1 }; }); W.text = E, W.position.set(8, 9 + xt), $t(n), n.eventMode = "static", n.cursor = "pointer", n.hitArea = new gt(0, 0, Math.max(0, i), Math.max(0, o)), n.on("pointerdown", function (G) { var P; if ((G == null ? void 0 : G.button) === 2)
        return; var h = m(G); h <= 0 || (p.focusedKeyByPointer.set(h, g), p.keyboardOwnerPointerId = h, w.open = !w.open, c == null || c(), (P = G.stopPropagation) == null || P.call(G)); }), w.open && T.push({ key: g, absX: s, absY: l, w: i, h: o, options: b, selectedIndex: w.selectedIndex }); }
    function qr(t) { var w; var e = t.popup, n = t.stage, r = t.theme, i = t.selectStates, o = t.uiState, s = t.getPointerId, l = t.requestPaint, a = t.viewportW, d = t.viewportH, p = 30, x = Math.min(7, e.options.length), c = x * p, T = e.absX, g = e.absY + e.h; T = Math.max(0, Math.min(T, Math.max(0, a - e.w))), g + c > d - 4 && (g = e.absY - c), g = Math.max(0, Math.min(g, Math.max(0, d - c))); var b = new _t; b.position.set(T, g), n.addChild(b); var _ = new wt; _.rect(0, 0, e.w, c), _.fill(16777215), _.rect(.5, .5, Math.max(0, e.w - 1), Math.max(0, c - 1)), _.stroke({ width: 1, color: r.control.border, alignment: 0 }), b.addChild(_), b.eventMode = "static", b.cursor = "pointer", b.hitArea = new gt(0, 0, e.w, c), b.on("pointerdown", function (M) { var W, U, G; if ((M == null ? void 0 : M.button) === 2)
        return; var $ = s(M), A = b.toLocal(M.global), H = (W = A == null ? void 0 : A.x) != null ? W : -1, N = (U = A == null ? void 0 : A.y) != null ? U : -1; if (H < 0 || H > e.w || N < 0 || N > c)
        return; var k = Math.max(0, Math.min(e.options.length - 1, Math.floor(N / p))), E = i.get(e.key); E && (E.selectedIndex = k, E.open = !1), $ > 0 && (o.focusedKeyByPointer.set($, e.key), o.keyboardOwnerPointerId = $), l == null || l(), (G = M.stopPropagation) == null || G.call(M); }); for (var M = 0; M < x; M++) {
        var $ = M * p;
        if (M === e.selectedIndex) {
            var H = new wt;
            H.rect(1, $ + 1, Math.max(0, e.w - 2), p - 2), H.fill({ color: 0, alpha: .06 }), b.addChild(H);
        }
        var A = zt({ text: (w = e.options[M]) != null ? w : "", fontFamily: r.fontFamily, fontSize: r.fontSize, fill: r.text, wordWrap: !1 });
        A.position.set(8, $ + 7 + xt), b.addChild(A);
    } }
    function It(t, e, n) { var r = Number.isFinite(t) ? t | 0 : 0; return Math.max(e, Math.min(n, r)); }
    function Yt(t) { var e = It(t, 0, 99); return e < 10 ? "0".concat(e) : String(e); }
    function qt(t, e, n) { var r = Number(t); if (!Number.isFinite(r))
        return null; var i = Math.trunc(r); return i < e || i > n ? null : i; }
    function sn(t) { if (t.length !== 4)
        return null; for (var r = 0; r < t.length; r += 1) {
        var i = t.charCodeAt(r);
        if (i < 48 || i > 57)
            return null;
    } var e = Number(t); if (!Number.isFinite(e))
        return null; var n = e - 2e3; return n < 0 || n > 99 ? null : n; }
    function lo(t) { var e = String(t != null ? t : "").trim().split(":"); if (e.length !== 2 && e.length !== 3)
        return null; var n = qt(e[0], 0, 23), r = qt(e[1], 0, 59), i = e.length === 3 ? qt(e[2], 0, 59) : 0; return n == null || r == null || i == null ? null : { hour: n, minute: r, second: i }; }
    function co(t) { var e = String(t != null ? t : "").trim().split("-"); if (e.length !== 2)
        return null; var n = sn(e[0]), r = qt(e[1], 1, 12); return n == null || r == null ? null : { year2: n, month: r }; }
    function uo(t) { var e = String(t != null ? t : "").trim().split("-"); if (e.length !== 3)
        return null; var n = sn(e[0]), r = qt(e[1], 1, 12), i = qt(e[2], 1, 31); if (n == null || r == null || i == null)
        return null; var o = It(Math.floor((i - 1) / 7) + 1, 1, 4); return { year2: n, month: r, weekIndex: o }; }
    function ho(t) { var e = String(t != null ? t : "").trim(), n = e.indexOf("-W"); if (n < 0)
        return null; var r = sn(e.slice(0, n)), i = qt(e.slice(n + 2), 1, 48); if (r == null || i == null)
        return null; var o = It(Math.floor((i - 1) / 4) + 1, 1, 12), s = It((i - 1) % 4 + 1, 1, 4); return { year2: r, month: o, weekIndex: s }; }
    function mo(t) { var e = String(t != null ? t : "").trim(), n = e.indexOf("T"); if (n < 0 && (n = e.indexOf(" ")), n < 0)
        return null; var r = e.slice(0, n).split("-"), i = e.slice(n + 1).split(":"); if (r.length !== 3 || i.length !== 2 && i.length !== 3)
        return null; var o = sn(r[0]), s = qt(r[1], 1, 12), l = qt(r[2], 1, 31), a = qt(i[0], 0, 23), d = qt(i[1], 0, 59), p = i.length === 3 ? qt(i[2], 0, 59) : 0; if (o == null || s == null || l == null || a == null || d == null || p == null)
        return null; var m = It(Math.floor((l - 1) / 7) + 1, 1, 4); return { year2: o, month: s, weekIndex: m, hour: a, minute: d, second: p }; }
    function rn(t) { return "20".concat(Yt(t.year2), "-").concat(Yt(t.month)); }
    function fo(t) { return (It(t.month, 1, 12) - 1) * 4 + It(t.weekIndex, 1, 4); }
    function on(t) { return "20".concat(Yt(t.year2), "-W").concat(Yt(fo(t))); }
    function Se(t) { var e = (It(t.weekIndex, 1, 4) - 1) * 7 + 1; return "20".concat(Yt(t.year2), "-").concat(Yt(t.month), "-").concat(Yt(e)); }
    function Fe(t) { return "".concat(Yt(t.hour), ":").concat(Yt(t.minute), ":").concat(Yt(t.second)); }
    function Ge(t) { return "".concat(Se(t), "T").concat(Fe(t)); }
    function po(t) { var p; var e = t.map, n = t.yearSliderOwners, r = t.inputKey, i = t.kind, o = t.attrs, s = e.get(r); if (s)
        return s.kind = i, s; var l = new Date, a = { kind: i, year2: It(l.getFullYear() - 2e3, 0, 99), month: It(l.getMonth() + 1, 1, 12), weekIndex: 1, hour: It(l.getHours(), 0, 23), minute: It(l.getMinutes(), 0, 59), second: It(l.getSeconds(), 0, 59), openPanel: null, openYear: !1, openMonthGrid: !1, yearSliderKey: "".concat(r, ":year-slider") }, d = String((p = o == null ? void 0 : o.value) != null ? p : ""); if (d.trim().length > 0) {
        if (i === "time") {
            var m = lo(d);
            m && (a.hour = m.hour, a.minute = m.minute, a.second = m.second);
        }
        else if (i === "month") {
            var m = co(d);
            m && (a.year2 = m.year2, a.month = m.month);
        }
        else if (i === "week") {
            var m = ho(d);
            m && (a.year2 = m.year2, a.month = m.month, a.weekIndex = m.weekIndex);
        }
        else if (i === "date") {
            var m = uo(d);
            m && (a.year2 = m.year2, a.month = m.month, a.weekIndex = m.weekIndex);
        }
        else if (i === "datetime-local") {
            var m = mo(d);
            m && (a.year2 = m.year2, a.month = m.month, a.weekIndex = m.weekIndex, a.hour = m.hour, a.minute = m.minute, a.second = m.second);
        }
    } return e.set(r, a), n.set(a.yearSliderKey, r), a; }
    function ei(t, e, n) { t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0), t.setHeight(36), t.setMinHeight(36), t.setMinWidth(n === "datetime-local" ? 340 : 220); }
    function go(t, e, n, r, i) { var o = i != null ? 2 : 1, s = o / 2; e.control.radius > 0 ? t.roundRect(s, s, Math.max(0, n - o), Math.max(0, r - o), e.control.radius) : t.rect(s, s, Math.max(0, n - o), Math.max(0, r - o)), t.fill(e.control.background), t.stroke({ width: o, color: i != null ? i : e.control.border }); }
    function ti(t, e, n, r, i) { var o = e + r / 2, s = n + r / 2; t.moveTo(e, s - 2), t.lineTo(o, s + 2), t.lineTo(e + r, s - 2), t.stroke({ width: 2, color: i }); }
    function ni(t) { var k, E; var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.absX, l = t.absY, a = t.theme, d = t.uiState, p = t.getPointerId, m = t.getCursorColor, x = t.temporalStates, c = t.yearSliderOwners, T = t.getOrInitInputValue, g = t.requestPaint, b = t.popupSink, _ = e.key; if (!_ || !e.tagName)
        return; var w = e.tagName === "timeinput" ? "time" : e.tagName === "monthinput" ? "month" : e.tagName === "weekinput" ? "week" : e.tagName === "dateinput" ? "date" : "datetime-local", M = po({ map: x, yearSliderOwners: c, inputKey: _, kind: w, attrs: e.attrs }), $ = T(_, ge(re({}, (k = e.attrs) != null ? k : {}), { type: "text" })); w === "time" ? $.value = Fe(M) : w === "month" ? $.value = rn(M) : w === "week" ? $.value = on(M) : w === "date" ? $.value = Se(M) : $.value = Ge(M); var A = (function () {
        var e_18, _a;
        var W = d.keyboardOwnerPointerId;
        if (d.focusedKeyByPointer.get(W) === _)
            return W;
        try {
            for (var _b = __values(d.focusedKeyByPointer.entries()), _c = _b.next(); !_c.done; _c = _b.next()) {
                var _d = __read(_c.value, 2), U = _d[0], G = _d[1];
                if (G === _)
                    return U;
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
    })(), H = A != null ? m(A) : null; go(r, a, i, o, H); var N = 8; if (w !== "datetime-local") {
        var W = (E = $.value) != null ? E : "", U = Pt(n, "__shown", function (P) { P.style = { fontFamily: a.fontFamily, fontSize: a.fontSize, fill: a.text, fontWeight: "400", wordWrap: !1 }; });
        U.text = W, U.visible = !0, U.position.set(N, 9 + xt);
        var G = n.getChildByLabel ? n.getChildByLabel("__date") : n.children.find(function (P) { return (P == null ? void 0 : P.label) === "__date"; }), h = n.getChildByLabel ? n.getChildByLabel("__time") : n.children.find(function (P) { return (P == null ? void 0 : P.label) === "__time"; });
        G && (G.visible = !1), h && (h.visible = !1), ti(r, Math.max(0, i - 18), 11, 10, a.text);
    }
    else {
        var W = Math.max(0, Math.round(i * .52));
        r.moveTo(W + .5, 0), r.lineTo(W + .5, o), r.stroke({ width: 1, color: a.control.border, alignment: 0 });
        var U = Se(M), G = Fe(M), h = Pt(n, "__date", function (q) { q.style = { fontFamily: a.fontFamily, fontSize: a.fontSize, fill: a.text, fontWeight: "400", wordWrap: !1 }; });
        h.text = U, h.visible = !0, h.position.set(N, 9 + xt);
        var P = Pt(n, "__time", function (q) { q.style = { fontFamily: a.fontFamily, fontSize: a.fontSize, fill: a.text, fontWeight: "400", wordWrap: !1 }; });
        P.text = G, P.visible = !0, P.position.set(W + N, 9 + xt);
        var F = n.getChildByLabel ? n.getChildByLabel("__shown") : n.children.find(function (q) { return (q == null ? void 0 : q.label) === "__shown"; });
        F && (F.visible = !1), ti(r, Math.max(W + 0, W + (i - W) - 18), 11, 10, a.text);
    } $t(n), n.eventMode = "static", n.cursor = "pointer", n.hitArea = new gt(0, 0, Math.max(0, i), Math.max(0, o)), n.on("pointerdown", function (W) { var G, h, P; if ((W == null ? void 0 : W.button) === 2)
        return; var U = p(W); if (!(U <= 0)) {
        if (d.focusedKeyByPointer.set(U, _), d.keyboardOwnerPointerId = U, w !== "datetime-local")
            M.openPanel = M.openPanel ? null : w === "time" ? "time" : w === "month" ? "month" : "week", M.openYear = !1, M.openMonthGrid = !1;
        else {
            var Z = ((h = (G = W.global) == null ? void 0 : G.x) != null ? h : 0) - s <= i * .52;
            M.openPanel = Z ? M.openPanel === "week" ? null : "week" : M.openPanel === "time" ? null : "time", M.openYear = !1, M.openMonthGrid = !1;
        }
        x.set(_, M), g == null || g(), (P = W.stopPropagation) == null || P.call(W);
    } }), M.openPanel === "month" ? b.push({ kind: "month-panel", inputKey: _, absX: s, absY: l, anchorW: i, anchorH: o }) : M.openPanel === "week" ? b.push({ kind: "week-panel", inputKey: _, absX: s, absY: l, anchorW: i, anchorH: o }) : M.openPanel === "time" && b.push({ kind: "time-panel", inputKey: _, absX: s, absY: l, anchorW: i, anchorH: o }); }
    function Le(t, e, n, r) { t.rect(0, 0, n, r), t.fill(e.control.background), t.rect(.5, .5, Math.max(0, n - 1), Math.max(0, r - 1)), t.stroke({ width: 1, color: e.control.border, alignment: 0 }); }
    function bo(t) { var e = t.stage, n = t.theme, r = t.popup, i = t.st, o = t.viewportW, s = t.viewportH, l = t.getPointerId, a = t.requestPaint, d = t.onPick, p = 4, m = 3, x = 44, c = 34, T = 8, g = T * 2 + p * x, b = T * 2 + m * c, _ = r.absX, w = r.absY + r.anchorH; _ = Math.max(0, Math.min(_, Math.max(0, o - g))), w + b > s - 4 && (w = r.absY - b), w = Math.max(0, Math.min(w, Math.max(0, s - b))); var M = new _t; M.position.set(_, w), e.addChild(M); var $ = new wt; Le($, n, g, b), M.addChild($); for (var A = 0; A < 12; A++) {
        var H = A + 1, N = T + A % p * x, k = T + Math.floor(A / p) * c;
        if (H === i.month) {
            var W = new wt;
            W.rect(N + 1, k + 1, x - 2, c - 2), W.fill({ color: 0, alpha: .06 }), M.addChild(W);
        }
        var E = zt({ text: String(H), fontFamily: n.fontFamily, fontSize: n.fontSize, fill: n.text, wordWrap: !1 });
        E.position.set(N + 14, k + 8 + xt), M.addChild(E), $.rect(N, k, x, c), $.stroke({ width: 1, color: n.control.border, alignment: 0 });
    } M.eventMode = "static", M.cursor = "pointer", M.hitArea = new gt(0, 0, g, b), M.on("pointerdown", function (A) { var q, Z, tt; if ((A == null ? void 0 : A.button) === 2 || l(A) <= 0)
        return; var N = M.toLocal(A.global), k = (q = N == null ? void 0 : N.x) != null ? q : -1, E = (Z = N == null ? void 0 : N.y) != null ? Z : -1, W = k - T, U = E - T; if (W < 0 || U < 0)
        return; var G = Math.floor(W / x), h = Math.floor(U / c); if (G < 0 || G >= p || h < 0 || h >= m)
        return; var F = h * p + G + 1; F < 1 || F > 12 || (d(F), a == null || a(), (tt = A.stopPropagation) == null || tt.call(A)); }); }
    function xo(t) {
        var e_19, _a;
        var e = t.stage, n = t.theme, r = t.popup, i = t.st, o = t.viewportW, s = t.viewportH, l = t.sliders, a = t.sliderBounds, d = t.sliderDrags, p = t.getPointerId, m = t.requestPaint, x = t.onChange, c = 10, T = 250, g = 78, b = r.absX, _ = r.absY;
        b = r.absX + r.anchorW + 6, _ = r.absY, b = Math.max(0, Math.min(b, Math.max(0, o - T))), _ = Math.max(0, Math.min(_, Math.max(0, s - g)));
        var w = new _t;
        w.position.set(b, _), e.addChild(w);
        var M = new wt;
        Le(M, n, T, g), w.addChild(M);
        var $ = zt({ text: "20".concat(Yt(i.year2)), fontFamily: n.fontFamily, fontSize: n.fontSize, fill: n.text, wordWrap: !1 });
        $.position.set(c, 8 + xt), w.addChild($);
        var A = i.yearSliderKey, H = Math.max(0, Math.min(1, It(i.year2, 0, 99) / 99)), N = ye(l, A, { value: String(H) }), k = !1;
        try {
            for (var _b = __values(d.values()), _c = _b.next(); !_c.done; _c = _b.next()) {
                var G = _c.value;
                if (G.key === A) {
                    k = !0;
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
        k || (N.value = H);
        var E = new _t;
        E.position.set(c, 40), w.addChild(E);
        var W = new wt;
        E.addChild(W), Ze({ node: { key: A, attrs: { value: String(N.value) } }, container: E, graphics: W, w: T - c * 2, h: 14, absX: b + c, absY: _ + 40, theme: { text: n.text, control: { progress: n.control.progress } }, sliderStates: l, sliderBounds: a, sliderDrags: d, requestPaint: m, getPointerId: p });
        var U = It(Math.round(N.value * 99), 0, 99);
        U !== i.year2 && x(U), w.eventMode = "static", w.hitArea = new gt(0, 0, T, g), w.on("pointerdown", function (G) { var h; (h = G.stopPropagation) == null || h.call(G); });
    }
    function yo(t) { var e = t.panel, n = t.theme, r = t.x, i = t.y, o = t.w, s = t.st, l = t.onPick, a = 30, d = 6, p = []; for (var m = 0; m < 4; m++) {
        var x = m + 1, c = i + m * (a + d), T = new wt;
        T.rect(r, c, o, a), T.fill({ color: 0, alpha: x === s.weekIndex ? .06 : .03 }), T.rect(r + .5, c + .5, Math.max(0, o - 1), Math.max(0, a - 1)), T.stroke({ width: 1, color: n.control.border, alignment: 0 }), e.addChild(T);
        var g = (It(s.month, 1, 12) - 1) * 4 + x, b = zt({ text: "".concat(x, " [").concat(Yt(g), "]"), fontFamily: n.fontFamily, fontSize: n.fontSize, fill: n.text, wordWrap: !1 });
        b.position.set(r + 10, c + 7 + xt), e.addChild(b), p.push({ x: r, y: c, w: o, h: a, weekIndex: x });
    } return { hitRects: p }; }
    function ri(t) {
        var e_20, _a, e_21, _b;
        var w, M, $, A, H, N;
        var e = t.popups, n = t.stage, r = t.theme, i = t.viewportW, o = t.viewportH, s = t.temporalStates, l = t.getOrInitInputValue, a = t.sliders, d = t.sliderBounds, p = t.sliderDrags, m = t.selects, x = t.selectPopups, c = t.getCursorColor, T = t.uiFocus, g = t.getPointerId, b = t.requestPaint, _ = [];
        var _loop_1 = function (k) {
            var E = s.get(k.inputKey);
            if (E) {
                if (k.kind === "month-panel") {
                    var Y = k.absX, j = k.absY + k.anchorH;
                    Y = Math.max(0, Math.min(Y, Math.max(0, i - 196))), j + 156 > o - 4 && (j = k.absY - 156), j = Math.max(0, Math.min(j, Math.max(0, o - 156)));
                    var v_1 = new _t;
                    v_1.position.set(Y, j), n.addChild(v_1);
                    var f = new wt;
                    Le(f, r, 196, 156), v_1.addChild(f);
                    var I_1 = { x: 10, y: 10, w: 132, h: 24 };
                    {
                        var y = new wt;
                        y.rect(I_1.x, I_1.y, I_1.w, I_1.h), y.fill({ color: 0, alpha: .03 }), y.rect(I_1.x + .5, I_1.y + .5, Math.max(0, I_1.w - 1), Math.max(0, I_1.h - 1)), y.stroke({ width: 1, color: r.control.border, alignment: 0 }), v_1.addChild(y);
                        var S = zt({ text: "Year 20".concat(Yt(E.year2)), fontFamily: r.fontFamily, fontSize: r.fontSize, fill: r.text, wordWrap: !1 });
                        S.position.set(I_1.x + 8, I_1.y + 4 + xt), v_1.addChild(S);
                    }
                    var O_1 = 10, C_1 = 44;
                    for (var y = 0; y < 12; y++) {
                        var S = y + 1, D = O_1 + y % 4 * 44, R = C_1 + Math.floor(y / 4) * 34;
                        if (S === E.month) {
                            var X = new wt;
                            X.rect(D + 1, R + 1, 42, 32), X.fill({ color: 0, alpha: .06 }), v_1.addChild(X);
                        }
                        var B = zt({ text: String(S), fontFamily: r.fontFamily, fontSize: r.fontSize, fill: r.text, wordWrap: !1 });
                        B.position.set(D + 14, R + 8 + xt), v_1.addChild(B), f.rect(D, R, 44, 34), f.stroke({ width: 1, color: r.control.border, alignment: 0 });
                    }
                    v_1.eventMode = "static", v_1.cursor = "pointer", v_1.hitArea = new gt(0, 0, 196, 156), v_1.on("pointerdown", function (y) { var dt, yt, Rt, Gt; if ((y == null ? void 0 : y.button) === 2)
                        return; var S = g(y); if (S <= 0)
                        return; T.focusedKeyByPointer.set(S, k.inputKey), T.keyboardOwnerPointerId = S; var D = v_1.toLocal(y.global), R = (dt = D == null ? void 0 : D.x) != null ? dt : -1, B = (yt = D == null ? void 0 : D.y) != null ? yt : -1; if (R >= I_1.x && R <= I_1.x + I_1.w && B >= I_1.y && B <= I_1.y + I_1.h) {
                        E.openYear = !0, s.set(k.inputKey, E), b == null || b(), (Rt = y.stopPropagation) == null || Rt.call(y);
                        return;
                    } var z = R - O_1, K = B - C_1; if (z < 0 || K < 0)
                        return; var V = Math.floor(z / 44), nt = Math.floor(K / 34); if (V < 0 || V >= 4 || nt < 0 || nt >= 3)
                        return; var it = nt * 4 + V + 1; if (it < 1 || it > 12)
                        return; E.month = it, E.openPanel = null, E.openYear = !1, E.openMonthGrid = !1, s.set(k.inputKey, E); var st = l(k.inputKey, { type: "text" }); st.value = rn(E), b == null || b(), (Gt = y.stopPropagation) == null || Gt.call(y); }), v_1.on("pointerdown", function (y) { var S; (S = y.stopPropagation) == null || S.call(y); }), E.openYear && _.push({ kind: "year-panel", inputKey: k.inputKey, absX: Y, absY: j, anchorW: 196, anchorH: 0 });
                }
                if (k.kind === "week-panel") {
                    var h = k.absX, P = k.absY + k.anchorH;
                    h = Math.max(0, Math.min(h, Math.max(0, i - 280))), P + 192 > o - 4 && (P = k.absY - 192), P = Math.max(0, Math.min(P, Math.max(0, o - 192)));
                    var F_1 = new _t;
                    F_1.position.set(h, P), n.addChild(F_1);
                    var q = new wt;
                    Le(q, r, 280, 192), F_1.addChild(q);
                    var Z_1 = { x: 10, y: 10, w: 104, h: 24 }, tt_1 = { x: 10 + Z_1.w + 10, y: 10, w: 120, h: 24 }, Y = function (f, I) { var O = new wt; O.rect(f.x, f.y, f.w, f.h), O.fill({ color: 0, alpha: .03 }), O.rect(f.x + .5, f.y + .5, Math.max(0, f.w - 1), Math.max(0, f.h - 1)), O.stroke({ width: 1, color: r.control.border, alignment: 0 }), F_1.addChild(O); var C = zt({ text: I, fontFamily: r.fontFamily, fontSize: r.fontSize, fill: r.text, wordWrap: !1 }); C.position.set(f.x + 8, f.y + 4 + xt), F_1.addChild(C); };
                    Y(Z_1, "Month ".concat(E.month)), Y(tt_1, "Year 20".concat(Yt(E.year2)));
                    var j = 44, v_2 = yo({ panel: F_1, theme: r, x: 10, y: j, w: 280 - 10 * 2, st: E, onPick: function () { } }).hitRects;
                    F_1.eventMode = "static", F_1.cursor = "pointer", F_1.hitArea = new gt(0, 0, 280, 192), F_1.on("pointerdown", function (f) {
                        var e_23, _a;
                        var D, R, B, X, z;
                        if ((f == null ? void 0 : f.button) === 2)
                            return;
                        var I = g(f);
                        if (I <= 0)
                            return;
                        T.focusedKeyByPointer.set(I, k.inputKey), T.keyboardOwnerPointerId = I;
                        var O = F_1.toLocal(f.global), C = (D = O == null ? void 0 : O.x) != null ? D : -1, y = (R = O == null ? void 0 : O.y) != null ? R : -1, S = function (K) { return C >= K.x && C <= K.x + K.w && y >= K.y && y <= K.y + K.h; };
                        if (S(Z_1)) {
                            E.openMonthGrid = !E.openMonthGrid, s.set(k.inputKey, E), b == null || b(), (B = f.stopPropagation) == null || B.call(f);
                            return;
                        }
                        if (S(tt_1)) {
                            E.openYear = !0, s.set(k.inputKey, E), b == null || b(), (X = f.stopPropagation) == null || X.call(f);
                            return;
                        }
                        try {
                            for (var v_3 = (e_23 = void 0, __values(v_2)), v_3_1 = v_3.next(); !v_3_1.done; v_3_1 = v_3.next()) {
                                var K = v_3_1.value;
                                if (S(K)) {
                                    E.weekIndex = K.weekIndex;
                                    var V = l(k.inputKey, { type: "text" });
                                    E.kind === "week" ? V.value = on(E) : E.kind === "date" ? V.value = Se(E) : V.value = Ge(E), E.openPanel = null, E.openYear = !1, E.openMonthGrid = !1, s.set(k.inputKey, E), b == null || b(), (z = f.stopPropagation) == null || z.call(f);
                                    return;
                                }
                            }
                        }
                        catch (e_23_1) { e_23 = { error: e_23_1 }; }
                        finally {
                            try {
                                if (v_3_1 && !v_3_1.done && (_a = v_3.return)) _a.call(v_3);
                            }
                            finally { if (e_23) throw e_23.error; }
                        }
                    }), E.openMonthGrid && _.push({ kind: "month-grid", inputKey: k.inputKey, absX: h, absY: P + Z_1.y + Z_1.h + 4, anchorW: 0, anchorH: 0 }), E.openYear && _.push({ kind: "year-panel", inputKey: k.inputKey, absX: h + tt_1.x, absY: P + tt_1.y, anchorW: tt_1.w, anchorH: 0 });
                }
                if (k.kind === "time-panel") {
                    var h_1 = k.absX, P_1 = k.absY + k.anchorH;
                    h_1 = Math.max(0, Math.min(h_1, Math.max(0, i - 330))), P_1 + 80 > o - 4 && (P_1 = k.absY - 80), P_1 = Math.max(0, Math.min(P_1, Math.max(0, o - 80)));
                    var F_2 = new _t;
                    F_2.position.set(h_1, P_1), n.addChild(F_2);
                    var q = new wt;
                    Le(q, r, 330, 80), F_2.addChild(q);
                    var Z = zt({ text: "Time", fontFamily: r.fontFamily, fontSize: r.fontSize, fill: r.text, wordWrap: !1 });
                    Z.position.set(10, 8 + xt), F_2.addChild(Z);
                    var tt_2 = function (nt) { return Array.from({ length: nt }, function (Q, it) { return Yt(it); }).join("\n"); }, Y = k.inputKey, j = "".concat(Y, ":time-h"), v = "".concat(Y, ":time-m"), f = "".concat(Y, ":time-s"), I = he(m, j, It(E.hour, 0, 23)), O = he(m, v, It(E.minute, 0, 59)), C = he(m, f, It(E.second, 0, 59));
                    I.selectedIndex = It(E.hour, 0, 23), O.selectedIndex = It(E.minute, 0, 59), C.selectedIndex = It(E.second, 0, 59);
                    var y_1 = 96, S_2 = 36, D_1 = 32, R = 8, B = function (nt, Q, it) { var st = new _t; st.position.set(Q, D_1), F_2.addChild(st); var dt = new wt; st.addChild(dt), nn({ node: { key: nt, attrs: { "data-options": tt_2(it), "data-selected-index": String(he(m, nt, 0).selectedIndex) } }, container: st, graphics: dt, w: y_1, h: S_2, absX: h_1 + Q, absY: P_1 + D_1, theme: r, selectStates: m, uiState: T, getPointerId: g, getCursorColor: c, requestPaint: b, popupSink: x }); };
                    B(j, 10, 24), B(v, 10 + y_1 + R, 60), B(f, 10 + (y_1 + R) * 2, 60);
                    var X = It((M = (w = m.get(j)) == null ? void 0 : w.selectedIndex) != null ? M : E.hour, 0, 23), z = It((A = ($ = m.get(v)) == null ? void 0 : $.selectedIndex) != null ? A : E.minute, 0, 59), K = It((N = (H = m.get(f)) == null ? void 0 : H.selectedIndex) != null ? N : E.second, 0, 59);
                    E.hour = X, E.minute = z, E.second = K, s.set(k.inputKey, E);
                    var V = l(k.inputKey, { type: "text" });
                    E.kind === "time" ? V.value = Fe(E) : V.value = Ge(E), F_2.eventMode = "static", F_2.hitArea = new gt(0, 0, 330, 80), F_2.on("pointerdown", function (nt) { var Q; (Q = nt.stopPropagation) == null || Q.call(nt); });
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
            var E = s.get(k.inputKey);
            E && (k.kind === "month-grid" && bo({ stage: n, theme: r, popup: k, st: E, viewportW: i, viewportH: o, getPointerId: g, requestPaint: b, onPick: function (W) { E.month = W, E.openMonthGrid = !1, s.set(k.inputKey, E); var U = l(k.inputKey, { type: "text" }); E.kind === "month" ? U.value = rn(E) : E.kind === "week" ? U.value = on(E) : E.kind === "date" ? U.value = Se(E) : U.value = Ge(E); } }), k.kind === "year-panel" && xo({ stage: n, theme: r, popup: k, st: E, viewportW: i, viewportH: o, sliders: a, sliderBounds: d, sliderDrags: p, getPointerId: g, requestPaint: b, onChange: function (W) { E.year2 = W, s.set(k.inputKey, E); var U = l(k.inputKey, { type: "text" }); E.kind === "month" ? U.value = rn(E) : E.kind === "week" ? U.value = on(E) : E.kind === "date" ? U.value = Se(E) : E.kind === "time" ? U.value = Fe(E) : U.value = Ge(E); } }));
        };
        try {
            for (var _1 = __values(_), _1_1 = _1.next(); !_1_1.done; _1_1 = _1.next()) {
                var k = _1_1.value;
                _loop_2(k);
            }
        }
        catch (e_21_1) { e_21 = { error: e_21_1 }; }
        finally {
            try {
                if (_1_1 && !_1_1.done && (_b = _1.return)) _b.call(_1);
            }
            finally { if (e_21) throw e_21.error; }
        }
    }
    function ii(t) {
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
    var oi = 5e4, Be = new WeakMap, ai = new Map, wo = 1, li = 0, _o = 0, si = !1, we = [], En = null;
    function He(t) { return t instanceof wt ? "Graphics" : t instanceof Qt ? "Text" : t instanceof _t ? "Container" : "Object"; }
    function To(t) { var e = t && typeof t == "object" ? t.label : void 0, n = t && typeof t == "object" ? He(t) : "Object"; return e ? "".concat(n, ":").concat(String(e).slice(0, 80)) : n; }
    function me(t) { var e = Be.get(t); return e || (e = wo++, Be.set(t, e)), ai.set(e, t), e; }
    function an(t) { var e, n, r, i, o, s; if (t == null || typeof t == "number" || typeof t == "string" || typeof t == "boolean")
        return t; if (Array.isArray(t))
        return t.slice(0, 16).map(an); if (typeof t == "object") {
        var l = t;
        return "color" in l || "alpha" in l || "width" in l && !("x" in l) && !("y" in l) && !("height" in l) ? { color: l.color, alpha: l.alpha, width: l.width } : "x" in l || "y" in l || "width" in l || "height" in l ? { x: Number((e = l.x) != null ? e : 0), y: Number((n = l.y) != null ? n : 0), w: Number((i = (r = l.width) != null ? r : l.w) != null ? i : 0), h: Number((s = (o = l.height) != null ? o : l.h) != null ? s : 0) } : He(l);
    } return String(t); }
    function Sn(t) { if (t != null)
        return typeof t == "symbol" ? t.toString() : String(t); }
    function ci(t) { if (t != null)
        return typeof t == "function" ? { type: "function", name: t.name || void 0, arity: t.length } : typeof t == "object" ? { id: me(t), type: He(t) } : { type: typeof t }; }
    function Mo(t) { if (t != null)
        return typeof t == "object" ? { id: me(t), type: He(t) } : typeof t == "function" ? { type: "function" } : { type: typeof t }; }
    function Eo(t) { var e = { event: Sn(t[0]), listener: ci(t[1]) }; return t.length > 2 && (e.context = Mo(t[2])), [e]; }
    function ko(t) { return String(t != null ? t : "").slice(0, 240); }
    function So(t) {
        var e_25, _a;
        var r, i;
        if (!t || typeof t != "object")
            return an(t);
        var e = t, n = { type: (i = (r = t.constructor) == null ? void 0 : r.name) != null ? i : "object" };
        try {
            for (var _b = __values(["fontFamily", "fontSize", "fontStyle", "fontWeight", "fill", "align", "lineHeight", "letterSpacing", "wordWrap", "wordWrapWidth", "padding"]), _c = _b.next(); !_c.done; _c = _b.next()) {
                var o = _c.value;
                var s = e[o];
                s !== void 0 && (n[o] = an(s));
            }
        }
        catch (e_25_1) { e_25 = { error: e_25_1 }; }
        finally {
            try {
                if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
            }
            finally { if (e_25) throw e_25.error; }
        }
        return n;
    }
    function Po(t) { var s, l, a, d, p, m; if (!t || typeof t != "object")
        return; var e = t, n = Number((s = e.x) != null ? s : 0), r = Number((l = e.y) != null ? l : 0), i = Number((d = (a = e.width) != null ? a : e.w) != null ? d : 0), o = Number((m = (p = e.height) != null ? p : e.h) != null ? m : 0); if (!(!Number.isFinite(n) || !Number.isFinite(r) || !Number.isFinite(i) || !Number.isFinite(o)) && !(i <= 0 || o <= 0))
        return { x: n, y: r, w: i, h: o }; }
    function Io(t, e) { if (e) {
        if (t === "addChild" || t === "removeChild")
            return e.map(function (n) { return n && typeof n == "object" ? me(n) : 0; });
        if (t === "mask") {
            var n = e[0];
            return [n && typeof n == "object" ? me(n) : 0];
        }
        if (t === "addChildAt" || t === "setChildIndex") {
            var n = e[0];
            return [n && typeof n == "object" ? me(n) : 0, Number(e[1]) || 0];
        }
        return t === "on" ? Eo(e) : t === "snapshot" ? e : t === "text.text.set" ? e.length ? [ko(e[0])] : [] : t === "text.style.set" ? e.length ? [So(e[0])] : [] : e.map(an);
    } }
    function ln(t, e, n) { var r, i; try {
        window.__TRUEOS_PIXI_CAPTURE_STEP__ = "record:".concat(e, ":begin");
        var o = window.__pixiCapture;
        if (!(o != null && o.enabled))
            return;
        o.counts[e] = ((r = o.counts[e]) != null ? r : 0) + 1;
        var s = { frame: li, seq: ++_o, op: e, id: t && typeof t == "object" ? me(t) : void 0, target: To(t), event: e === "on" && (n != null && n.length) ? Sn(n[0]) : void 0, listener: e === "on" && (n != null && n.length) ? ci(n[1]) : void 0, args: Io(e, n) };
        window.__TRUEOS_PIXI_CAPTURE_STEP__ = "record:".concat(e, ":push"), o.commands.push(s), o.persist && Co(s), o.commands.length > oi && o.commands.splice(0, o.commands.length - oi), window.__TRUEOS_PIXI_CAPTURE_STEP__ = "record:".concat(e, ":done");
    }
    catch (o) {
        try {
            window.__TRUEOS_PIXI_CAPTURE_ERROR__ = "record:".concat(e, ":").concat(String((i = o == null ? void 0 : o.message) != null ? i : o));
        }
        catch (s) { }
    } }
    function Co(t) { if (we.push(t), t.op === "snapshot") {
        We();
        return;
    } if (we.length >= 512) {
        We();
        return;
    } En == null && (En = window.setTimeout(function () { En = null, We(); }, 50)); }
    function We() {
        if (we.length === 0)
            return;
        var t = we;
        we = [];
        var e = t.map(function (n) { return JSON.stringify(n); }).join("\n") + "\n";
        navigator.sendBeacon && navigator.sendBeacon("/__pixi_capture", new Blob([e], { type: "application/x-ndjson" })) || fetch("/__pixi_capture", { method: "POST", headers: { "Content-Type": "application/x-ndjson" }, body: e, keepalive: !0 }).catch(function () { we = t.concat(we); });
    }
    function Oo(t, e, n) {
        var e_26, _a, e_27, _b, e_28, _c;
        var r, i;
        if (e === "on") {
            var o = Sn(n[0]), s = n[1];
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
                            var l = s.parent.children.indexOf(s);
                            l >= 0 && s.parent.children.splice(l, 1);
                        }
                        s.parent = t, t.children.push(s);
                    }
                }
            }
            catch (e_26_1) { e_26 = { error: e_26_1 }; }
            finally {
                try {
                    if (o_1_1 && !o_1_1.done && (_a = o_1.return)) _a.call(o_1);
                }
                finally { if (e_26) throw e_26.error; }
            }
            return o[0];
        }
        if (e === "addChildAt") {
            var o = n[0];
            if (!o)
                return o;
            if (Array.isArray(t.children) || (t.children = []), o.parent && Array.isArray(o.parent.children)) {
                var l = o.parent.children.indexOf(o);
                l >= 0 && o.parent.children.splice(l, 1);
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
                    var l = t.children.indexOf(s);
                    l >= 0 && t.children.splice(l, 1), s && (s.parent = null);
                }
            }
            catch (e_27_1) { e_27 = { error: e_27_1 }; }
            finally {
                try {
                    if (o_2_1 && !o_2_1.done && (_b = o_2.return)) _b.call(o_2);
                }
                finally { if (e_27) throw e_27.error; }
            }
            return o[0];
        }
        if (e === "removeChildren") {
            Array.isArray(t.children) || (t.children = []);
            var o = Math.max(0, Number((r = n[0]) != null ? r : 0) | 0), s = Array.isArray(t.children) ? t.children.length : o, l = Math.max(o, Math.min(Number((i = n[1]) != null ? i : s) | 0, s)), a = t.children.splice(o, l - o);
            try {
                for (var a_1 = __values(a), a_1_1 = a_1.next(); !a_1_1.done; a_1_1 = a_1.next()) {
                    var d = a_1_1.value;
                    d.parent = null;
                }
            }
            catch (e_28_1) { e_28 = { error: e_28_1 }; }
            finally {
                try {
                    if (a_1_1 && !a_1_1.done && (_c = a_1.return)) _c.call(a_1);
                }
                finally { if (e_28) throw e_28.error; }
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
            var l = Math.max(0, Math.min(Number(n[1]) | 0, t.children.length));
            t.children.splice(l, 0, o);
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
    function Ro() { window.__TRUEOS_DISPATCH_PIXI_POINTER__ = function (t, e, n, r, i, o, s) {
        var e_29, _a;
        if (s === void 0) { s = 0; }
        var T, g, b, _, w, M, $, A, H, N, k, E, W, U;
        var l = function (G) { try {
            window.__TRUEOS_PIXI_POINTER_DISPATCH_STEP__ = G, typeof console != "undefined" && typeof console.log == "function" && console.log("[trueos pointer dispatch] ".concat(G));
        }
        catch (h) { } };
        l("start node=".concat(Number(t) || 0, " event=").concat(String(e || "")));
        var a = window.__TRUEOS_PIXI_APP;
        if (String(e || "") === "wheel") {
            var G = a == null ? void 0 : a.canvas;
            if (!G || typeof G.dispatchEvent != "function")
                return l("wheel-canvas-missing"), { handled: 0, listenerCount: 0, painted: 0, targetFound: 0 };
            var h = (b = (g = (T = window.__pixiCapture) == null ? void 0 : T.commands) == null ? void 0 : g.length) != null ? b : 0, P = { type: "wheel", deltaX: 0, deltaY: Number(s) || 0, deltaMode: 0, offsetX: Number(n) || 0, offsetY: Number(r) || 0, clientX: Number(n) || 0, clientY: Number(r) || 0, pointerId: Number(i) || 1, buttons: Number(o) || 0, defaultPrevented: !1, propagationStopped: !1, preventDefault: function () { this.defaultPrevented = !0; }, stopPropagation: function () { this.propagationStopped = !0; } };
            l("wheel-dispatch deltaY=".concat(P.deltaY)), G.dispatchEvent(P);
            var F = 0;
            if (window.__TRUEOS_CAPTURE_ONLY__) {
                var j = window.__TRUEOS_REPAINT_NOW__;
                window.__TRUEOS_PIXI_DIRTY__ && typeof j == "function" && (l("wheel-repaint-call"), j(), l("wheel-repaint-return"), F = 1);
            }
            else
                (_ = a == null ? void 0 : a.renderer) != null && _.render && (a != null && a.stage) && (a.renderer.render(a.stage), F = 1);
            var q = ($ = (M = (w = window.__pixiCapture) == null ? void 0 : w.commands) == null ? void 0 : M.length) != null ? $ : h, Z = (A = G.listeners) == null ? void 0 : A.wheel, tt = Array.isArray(Z) ? Z.length : typeof Z == "function" ? 1 : 0, Y = P.defaultPrevented || tt > 0 ? 1 : 0;
            return l("wheel-done handled=".concat(Y, " listeners=").concat(tt, " painted=").concat(F)), { handled: Y, listenerCount: tt, painted: q > h || F ? 1 : 0, targetFound: 1 };
        }
        var d = ai.get(Number(t) || 0), p = 0, m = 0, x = 0;
        if (!d)
            return l("target-missing"), { handled: p, listenerCount: m, painted: x, targetFound: 0 };
        var c = { type: String(e || ""), button: Number(o) & 2 ? 2 : 0, buttons: Number(o) || 0, pointerId: Number(i) || 1, pointerType: "mouse", global: { x: Number(n) || 0, y: Number(r) || 0 }, data: { pointerId: Number(i) || 1, pointerType: "mouse", global: { x: Number(n) || 0, y: Number(r) || 0 } }, target: d, currentTarget: d, defaultPrevented: !1, propagationStopped: !1, preventDefault: function () { this.defaultPrevented = !0; }, stopPropagation: function () { this.propagationStopped = !0; } };
        l("target-found label=".concat(String((H = d.label) != null ? H : "")));
        for (var G = d; G; G = G.parent) {
            c.currentTarget = G;
            var h = (N = G.listeners) == null ? void 0 : N[c.type];
            if (!(!Array.isArray(h) || h.length === 0)) {
                m += h.length, l("listeners node=".concat((k = Be.get(G)) != null ? k : 0, " count=").concat(h.length));
                try {
                    for (var _b = (e_29 = void 0, __values(h.slice())), _c = _b.next(); !_c.done; _c = _b.next()) {
                        var P = _c.value;
                        if (typeof P == "function" && (p = 1, l("listener-call node=".concat((E = Be.get(G)) != null ? E : 0)), P.call(G, c), l("listener-return node=".concat((W = Be.get(G)) != null ? W : 0)), c.propagationStopped))
                            break;
                    }
                }
                catch (e_29_1) { e_29 = { error: e_29_1 }; }
                finally {
                    try {
                        if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
                    }
                    finally { if (e_29) throw e_29.error; }
                }
                if (c.propagationStopped)
                    break;
            }
        }
        if (window.__TRUEOS_CAPTURE_ONLY__) {
            var G = window.__TRUEOS_REPAINT_NOW__;
            window.__TRUEOS_PIXI_DIRTY__ && typeof G == "function" && (l("capture-repaint-call"), G(), l("capture-repaint-return"), x = 1);
        }
        else
            (U = a == null ? void 0 : a.renderer) != null && U.render && (a != null && a.stage) && (l("paint-call"), a.renderer.render(a.stage), l("paint-return"), x = 1);
        return l("done handled=".concat(p, " listeners=").concat(m, " painted=").concat(x)), { handled: p, listenerCount: m, painted: x, targetFound: 1 };
    }; }
    function kn(t, e, n) {
        if (n === void 0) { n = e; }
        var r = t == null ? void 0 : t[e];
        if (typeof r != "function" || r.__pixiCapturePatched)
            return;
        var i = function () {
            var s = [];
            for (var _a = 0; _a < arguments.length; _a++) {
                s[_a] = arguments[_a];
            }
            var l;
            if (ln(this, n, s), !window.__TRUEOS_CAPTURE_ONLY__)
                return r.apply(this, s);
            try {
                window.__TRUEOS_PIXI_CAPTURE_STEP__ = "invoke:".concat(n, ":begin");
                var a = Oo(this, n, s);
                return window.__TRUEOS_PIXI_CAPTURE_STEP__ = "invoke:".concat(n, ":done"), a;
            }
            catch (a) {
                try {
                    window.__TRUEOS_PIXI_CAPTURE_ERROR__ = "invoke:".concat(n, ":").concat(String((l = a == null ? void 0 : a.message) != null ? l : a));
                }
                catch (d) { }
                return n === "addChild" || n === "addChildAt" || n === "removeChild" ? s[0] : this;
            }
        };
        i.__pixiCapturePatched = !0, t[e] = i;
    }
    function Do(t, e) { var n = t; for (; n;) {
        var r = Object.getOwnPropertyDescriptor(n, e);
        if (r)
            return r;
        n = Object.getPrototypeOf(n);
    } }
    function Pe(t, e, n) { var o, s; if (!(t != null && t.constructor) || t.constructor["__pixiCapturePatched_".concat(n)])
        return; var r = Do(t, e); if ((r == null ? void 0 : r.configurable) === !1 || r && !r.set && !r.writable)
        return; var i = typeof Symbol == "function" ? Symbol("pixiCapture:".concat(n)) : "__pixiCaptureValue_".concat(n); Object.defineProperty(t, e, { configurable: (o = r == null ? void 0 : r.configurable) != null ? o : !0, enumerable: (s = r == null ? void 0 : r.enumerable) != null ? s : !0, get: r != null && r.get ? function () { var a; return (a = r.get) == null ? void 0 : a.call(this); } : function () { var a = this; return Object.prototype.hasOwnProperty.call(a, i) ? a[i] : r && "value" in r ? r.value : void 0; }, set: function (a) { if (ln(this, n, [a]), !window.__TRUEOS_CAPTURE_ONLY__) {
            r != null && r.set ? r.set.call(this, a) : Object.defineProperty(this, i, { configurable: !0, enumerable: !1, writable: !0, value: a });
            return;
        } var d = this; n === "text.text.set" ? d._text = String(a != null ? a : "") : n === "text.style.set" ? d._style = a != null ? a : {} : n === "text.resolution.set" ? d._resolution = Math.max(1, Number(a) || 1) : Object.defineProperty(d, i, { configurable: !0, enumerable: !1, writable: !0, value: a }); } }), t.constructor["__pixiCapturePatched_".concat(n)] = !0; }
    function ui(t, e) {
        if (e === void 0) { e = 0; }
        var s, l, a, d, p, m, x, c, T;
        if (!t || e > 64)
            return null;
        var n, r;
        try {
            var g = typeof t.getGlobalPosition == "function" ? t.getGlobalPosition() : null;
            g && Number.isFinite(Number(g.x)) && Number.isFinite(Number(g.y)) && (n = Number(g.x), r = Number(g.y));
        }
        catch (g) { }
        var i = { id: me(t), type: He(t), label: (s = t.label) != null ? s : void 0, x: (d = (a = (l = t.position) == null ? void 0 : l.x) != null ? a : t.x) != null ? d : 0, y: (x = (m = (p = t.position) == null ? void 0 : p.y) != null ? m : t.y) != null ? x : 0, globalX: n, globalY: r, scaleX: Number.isFinite(Number((c = t.scale) == null ? void 0 : c.x)) ? Number(t.scale.x) : 1, scaleY: Number.isFinite(Number((T = t.scale) == null ? void 0 : T.y)) ? Number(t.scale.y) : 1, visible: t.visible, alpha: Number.isFinite(Number(t.alpha)) ? Number(t.alpha) : 1, maskId: t.mask ? me(t.mask) : 0, zIndex: Number(t.zIndex) || 0, sortableChildren: t.sortableChildren === !0 }, o = Po(t.hitArea);
        return o && (i.hitArea = o), typeof t.text == "string" && (i.text = t.text.slice(0, 120)), Array.isArray(t.children) && t.children.length && (i.children = t.children.map(function (g) { return ui(g, e + 1); })), i;
    }
    function di() {
        var e_30, _a, e_31, _b;
        if (window.__pixiCapture)
            return window.__pixiCapture;
        var t = { enabled: !0, persist: !window.__TRUEOS_CAPTURE_ONLY__, commands: [], counts: Object.create(null), clear: function () { this.commands.length = 0, this.counts = Object.create(null); }, dump: function (e) {
                if (e === void 0) { e = 200; }
                return this.commands.slice(-e);
            }, flush: function () { We(); }, summary: function () { return re({}, this.counts); } };
        if (window.__pixiCapture = t, Ro(), window.addEventListener("beforeunload", function () { return We(); }), !si) {
            si = !0, typeof wt.prototype.image != "function" && (wt.prototype.image = function () { return this; });
            try {
                for (var _c = __values(["clear", "rect", "roundRect", "circle", "ellipse", "moveTo", "lineTo", "closePath", "poly", "fill", "stroke", "image", "svg"]), _d = _c.next(); !_d.done; _d = _c.next()) {
                    var e = _d.value;
                    kn(wt.prototype, e);
                }
            }
            catch (e_30_1) { e_30 = { error: e_30_1 }; }
            finally {
                try {
                    if (_d && !_d.done && (_a = _c.return)) _a.call(_c);
                }
                finally { if (e_30) throw e_30.error; }
            }
            try {
                for (var _f = __values(["addChild", "addChildAt", "removeChild", "removeChildren", "setChildIndex", "on", "removeAllListeners"]), _g = _f.next(); !_g.done; _g = _f.next()) {
                    var e = _g.value;
                    kn(_t.prototype, e);
                }
            }
            catch (e_31_1) { e_31 = { error: e_31_1 }; }
            finally {
                try {
                    if (_g && !_g.done && (_b = _f.return)) _b.call(_f);
                }
                finally { if (e_31) throw e_31.error; }
            }
            Pe(Qt.prototype, "text", "text.text.set"), Pe(Qt.prototype, "style", "text.style.set"), Pe(Qt.prototype, "resolution", "text.resolution.set"), kn(Qt.prototype, "setSize", "text.setSize"), Pe(_t.prototype, "visible", "visible"), Pe(_t.prototype, "alpha", "alpha"), Pe(_t.prototype, "mask", "mask");
        }
        return t;
    }
    function hi(t) { var e = t.renderer, n = e == null ? void 0 : e.render; if (typeof n != "function" || n.__pixiCapturePatched)
        return; var r = function (o) { var s = o && typeof o == "object" && "container" in o ? o.container : o || t.stage; return li++, ln(s, "render", []), ln(s, "snapshot", [ui(s)]), window.__TRUEOS_CAPTURE_ONLY__ ? s : n.call(this, o); }; r.__pixiCapturePatched = !0, e.render = r; }
    di();
    var rt = null, Cn = 6, Ie = 10, Wt = 1, Ht = 3, Ut = 4, Ce = 512, xi = new Map;
    var u = { focusedKeyByPointer: new Map, keyboardOwnerPointerId: 1, inputs: new Map, sliders: new Map, sliderDrags: new Map, sliderBounds: new Map, dialogs: new Map, dialogDrags: new Map, dialogSelectedBy: new Map, dialogZ: new Map, dialogZCounter: 1, numbers: new Map, numberHolds: new Map, selects: new Map, temporals: new Map, temporalYearOwners: new Map, color: { rgb: { r: 255, g: 0, b: 0 }, a: 255, pick: null, draggingPointerId: null, bounds: null }, cursorColors: new Map, primaryMousePointerId: 1, harness: { enabled: !0, activeUserPointerId: Wt, periodMs: 3e3 }, userCursorPos: new Map, lastMouse: { x: 0, y: 0, has: !1 }, scroll: { y: 0, contentHeight: 0, viewportHeight: 0, draggingPointerId: null, dragOffsetY: 0, track: { x: 0, y: 0, w: Ie, h: 0 }, thumb: { x: 0, y: 0, w: Ie, h: 0 } }, iframeScroll: new Map, iframeRects: [], hoverRects: [], hoverHandlers: new Map, hoveredKeyByPointer: new Map, hoveredCursorByPointer: new Map, virtualCursor: { enabled: !1, x: 0, y: 0, t: 0, radius: 120, speed: .9 }, textDrags: new Map, fieldBounds: new Map, dialogDragBounds: new Map, detailsOpen: new Map, contextMenus: new Map, clipboards: new Map }, cn = null, On = 0;
    function vo(t) { if (!cn) {
        var n = document.createElement("canvas").getContext("2d");
        if (!n)
            throw new Error("2D canvas not available");
        cn = n;
    } return cn.font = "".concat(t.fontSize, "px ").concat(t.fontFamily), function (e) { return (On += 1, cn.measureText(e).width); }; }
    function Pn(t, e) {
        if (e === void 0) { e = 16; }
        return Object.entries(t).sort(function (n, r) { return r[1] - n[1] || (n[0] < r[0] ? -1 : n[0] > r[0] ? 1 : 0); }).slice(0, e).map(function (_a) {
            var _b = __read(_a, 2), n = _b[0], r = _b[1];
            return "".concat(n, ":").concat(r);
        }).join(",");
    }
    function Ue(t) { var e = String(t != null ? t : ""); return e.indexOf("<truesurfer-") >= 0 && (e = e.replace(/<truesurfer-[A-Za-z0-9._-]+>/g, "")), e; }
    function No(t, e) { if (e >= t.length)
        return !0; var n = t.charCodeAt(e); return n === 95 || n === 40 || n === 91 || n === 123 || n === 34 || n === 39 || n >= 48 && n <= 57 || n >= 65 && n <= 90; }
    function yi(t) { var e = t, n = !0; for (; n;) {
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
        r >= 2 && No(e, r) && (e = e.slice(r), n = !0);
    } return e; }
    function Go(t) { var e = Ue(t), n = e.indexOf("__trueos") >= 0 || e.indexOf("tsNu") >= 0 || e.indexOf("tsNum") >= 0; return e.indexOf("__TRUEOS_HOST_READY__") >= 0 && (e = e.replace(/__TRUEOS_HOST_READY__/g, "")), e.indexOf("__trueos") >= 0 && (e = Lo(e), e = e.replace(/__trueosNumberValue/g, "").replace(/__trueosHostNum/g, "").replace(/__trueosNum/g, "").replace(/__trueosNu/g, "").replace(/__trueos/g, "")), (e.indexOf("tsNu") >= 0 || e.indexOf("tsNum") >= 0) && (e = e.replace(/tsNum/g, "").replace(/tsNutsNutsNutsNu/g, "").replace(/tsNutsNutsNu/g, "").replace(/tsNutsNu/g, "").replace(/tsNu/g, "")), n && (e = yi(e.trimStart())), e; }
    function Lo(t) { var e = "__trueosN", n = t, r = 0; for (; r < n.length;) {
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
    function wi(t) { return Go(t); }
    function Rn(t) { return yi(wi(t).trimStart()); }
    function Fo(t) { var e = le(Rn(t)); return !(e.length === 0 || e === "true" || e === "false" || e === "N" || e === "Nu" || e === "Num" || e.startsWith("<truesurfer-") || e.startsWith("__trueo")); }
    function _i(t, e) { var r; var n = Ue(e) || "block"; t[n] = ((r = t[n]) != null ? r : 0) + 1; }
    function Bo(t) {
        var e_32, _a;
        var e = { nodes: 0, blocks: 0, text: 0, maxDepth: 0, tags: {} }, n = function (r, i) {
            var e_33, _a;
            if (e.nodes += 1, e.maxDepth = Math.max(e.maxDepth, i), r.kind === "text") {
                e.text += 1;
                return;
            }
            e.blocks += 1, _i(e.tags, r.tagName);
            try {
                for (var _b = __values(r.children), _c = _b.next(); !_c.done; _c = _b.next()) {
                    var o = _c.value;
                    n(o, i + 1);
                }
            }
            catch (e_33_1) { e_33 = { error: e_33_1 }; }
            finally {
                try {
                    if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
                }
                finally { if (e_33) throw e_33.error; }
            }
        };
        try {
            for (var t_1 = __values(t), t_1_1 = t_1.next(); !t_1_1.done; t_1_1 = t_1.next()) {
                var r = t_1_1.value;
                n(r, 1);
            }
        }
        catch (e_32_1) { e_32 = { error: e_32_1 }; }
        finally {
            try {
                if (t_1_1 && !t_1_1.done && (_a = t_1.return)) _a.call(t_1);
            }
            finally { if (e_32) throw e_32.error; }
        }
        return e;
    }
    function Wo(t) { var e = { nodes: 0, blocks: 0, text: 0, maxDepth: 0, tags: {} }, n = function (r, i) {
        var e_34, _a;
        var o;
        e.nodes += 1, e.maxDepth = Math.max(e.maxDepth, i), r.kind === "text" ? e.text += 1 : (e.blocks += 1, _i(e.tags, (o = r.tagName) != null ? o : "block"));
        try {
            for (var _b = __values(r.children), _c = _b.next(); !_c.done; _c = _b.next()) {
                var s = _c.value;
                n(s, i + 1);
            }
        }
        catch (e_34_1) { e_34 = { error: e_34_1 }; }
        finally {
            try {
                if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
            }
            finally { if (e_34) throw e_34.error; }
        }
    }; return n(t, 1), e; }
    function Dn(t, e) {
        if (e === void 0) { e = 64; }
        var n = le(wi(t)), r = "";
        for (var i = 0; i < n.length && r.length < e; i += 1) {
            var o = n.charAt(i);
            r += o === "|" || o === '"' || o === "\\" ? "_" : o;
        }
        return r;
    }
    function Ti(t, e) {
        if (e === void 0) { e = 120; }
        var n = "";
        for (var r = 0; r < t.length && n.length < e; r += 1) {
            var i = t.charAt(r);
            n += i === "\r" || i === "\n" || i === "	" || i === "|" || i === '"' || i === "\\" ? "_" : i;
        }
        return n;
    }
    function Ho(t, e) {
        var e_35, _a;
        if (e === void 0) { e = 12; }
        var n = [], r = function (i, o, s) {
            var e_36, _a;
            if (n.length >= e)
                return;
            if (i.kind === "text") {
                n.push("#".concat(n.length, "@").concat(o, ":").concat(s, " chars=").concat(i.text.length, " sample=\"").concat(Dn(i.text), "\""));
                return;
            }
            var l = Ue(i.tagName || "block") || "block", a = i.key || "";
            try {
                for (var _b = __values(i.children), _c = _b.next(); !_c.done; _c = _b.next()) {
                    var d = _c.value;
                    r(d, l, a);
                }
            }
            catch (e_36_1) { e_36 = { error: e_36_1 }; }
            finally {
                try {
                    if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
                }
                finally { if (e_36) throw e_36.error; }
            }
        };
        try {
            for (var t_2 = __values(t), t_2_1 = t_2.next(); !t_2_1.done; t_2_1 = t_2.next()) {
                var i = t_2_1.value;
                r(i, "root", "");
            }
        }
        catch (e_35_1) { e_35 = { error: e_35_1 }; }
        finally {
            try {
                if (t_2_1 && !t_2_1.done && (_a = t_2.return)) _a.call(t_2);
            }
            finally { if (e_35) throw e_35.error; }
        }
        return n.join("|");
    }
    function $o(t, e) {
        if (e === void 0) { e = 12; }
        var n = [], r = function (i, o, s) {
            var e_37, _a;
            var d;
            if (n.length >= e)
                return;
            if (i.kind === "text") {
                var p = (d = i.text) != null ? d : "";
                n.push("#".concat(n.length, "@").concat(o, ":").concat(s, " chars=").concat(p.length, " box=").concat(Math.round(i.x), ",").concat(Math.round(i.y), ",").concat(Math.round(i.width), ",").concat(Math.round(i.height), " sample=\"").concat(Dn(p), "\""));
                return;
            }
            var l = Ue(i.tagName || "block") || "block", a = i.key || "";
            try {
                for (var _b = __values(i.children), _c = _b.next(); !_c.done; _c = _b.next()) {
                    var p = _c.value;
                    r(p, l, a);
                }
            }
            catch (e_37_1) { e_37 = { error: e_37_1 }; }
            finally {
                try {
                    if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
                }
                finally { if (e_37) throw e_37.error; }
            }
        };
        return r(t, "root", ""), n.join("|");
    }
    function An(t) { return String(t != null ? t : "").replace(/&quot;/g, '"').replace(/&#34;/g, '"').replace(/&#39;/g, "'").replace(/&apos;/g, "'").replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&amp;/g, "&"); }
    function In(t) { return le(An(String(t != null ? t : "").replace(/<[^>]*>/g, " "))); }
    function Mi(t) { var a, d; var e = String(t != null ? t : "").replace(/<script\b[\s\S]*?<\/script>/gi, " ").replace(/<style\b[\s\S]*?<\/style>/gi, " "), n = [], r = new Set(["h1", "h2", "h3", "h4", "h5", "h6", "summary", "p", "button", "label", "legend", "option"]), i = function (p) { var m = In(p); m.length !== 0 && (m.startsWith("<truesurfer-") || m.startsWith("__trueo") || n.push(m)); }, o = [], s = /<\/?([a-zA-Z0-9:-]+)\b[^>]*>|([^<]+)/g, l; for (; (l = s.exec(e)) && n.length < Ce;) {
        var p = l[2];
        if (p != null) {
            var b = An(p);
            if (!b)
                continue;
            for (var _ = o.length - 1; _ >= 0; _ -= 1)
                if (o[_].wanted) {
                    o[_].text += " ".concat(b);
                    break;
                }
            continue;
        }
        var m = (a = l[0]) != null ? a : "", x = String((d = l[1]) != null ? d : "").toLowerCase();
        if (m.charAt(1) === "/") {
            for (var b = o.length - 1; b >= 0; b -= 1) {
                var _ = o.pop();
                if (_ != null && _.wanted && i(_.text), (_ == null ? void 0 : _.tag) === x)
                    break;
            }
            continue;
        }
        if (x === "input") {
            var b = pi(m, "type").toLowerCase();
            (b === "button" || b === "submit" || b === "reset") && i(pi(m, "value"));
        }
        var T = m.length - 1;
        for (; T >= 0 && m.charCodeAt(T) <= 32;)
            T -= 1;
        T >= 1 && m.charAt(T) === ">" && m.charAt(T - 1) === "/" || x === "input" || x === "br" || x === "hr" || x === "img" || o.push({ tag: x, wanted: r.has(x), text: "" });
    } for (; o.length && n.length < Ce;) {
        var p = o.pop();
        p != null && p.wanted && i(p.text);
    } if (n.length === 0) {
        var p = e.toLowerCase(), m = p.indexOf("<body");
        if (m >= 0) {
            var b = e.indexOf(">", m);
            m = b >= 0 ? b + 1 : m;
        }
        else
            m = 0;
        var x = p.indexOf("</body>", m), c = x >= 0 ? x : e.length, T = !1, g = "";
        for (var b = m; b < c && n.length < Ce; b += 1) {
            var _ = e.charAt(b);
            if (_ === "<") {
                i(g), g = "", T = !0;
                continue;
            }
            if (_ === ">") {
                T = !1;
                continue;
            }
            T || (g += _);
        }
        i(g);
    } return n; }
    function Uo(t, e) {
        if (e === void 0) { e = 8; }
        var n = [];
        for (var r = 0; r < t.length && n.length < e; r += 1)
            n.push("#".concat(r, "=\"").concat(Ti(t[r], 48), "\""));
        return n.join("|");
    }
    function pi(t, e) { var i, o, s; var r = new RegExp("".concat(e, "[ \\t\\r\\n\\f]*=[ \\t\\r\\n\\f]*(\"([^\"]*)\"|'([^']*)'|([^ \\t\\r\\n\\f>]+))"), "i").exec(t); return An((s = (o = (i = r == null ? void 0 : r[2]) != null ? i : r == null ? void 0 : r[3]) != null ? o : r == null ? void 0 : r[4]) != null ? s : ""); }
    function Oe(t) {
        var e_38, _a;
        var e = [];
        try {
            for (var t_3 = __values(t), t_3_1 = t_3.next(); !t_3_1.done; t_3_1 = t_3.next()) {
                var n = t_3_1.value;
                var r = le(String(n != null ? n : ""));
                r.length !== 0 && (e.includes(r) || e.push(r));
            }
        }
        catch (e_38_1) { e_38 = { error: e_38_1 }; }
        finally {
            try {
                if (t_3_1 && !t_3_1.done && (_a = t_3.return)) _a.call(t_3);
            }
            finally { if (e_38) throw e_38.error; }
        }
        return e;
    }
    function Xo(t) { var o, s, l, a; var e = [], n = String(t != null ? t : "").replace(/<script[^]*?<\/script>/gi, " ").replace(/<style[^]*?<\/style>/gi, " "), r = /<(h[1-6]|p|label|button)\b[^>]*>([^]*?)<\/\1>|<input\b[^>]*>/gi, i; for (; (i = r.exec(n)) && e.length < Ce;) {
        var d = (o = i[0]) != null ? o : "", p = String((s = i[1]) != null ? s : "").toLowerCase();
        if (d.toLowerCase().startsWith("<input"))
            continue;
        var m = In(p === "p" || p === "label" ? (l = i[2]) != null ? l : "" : (a = i[2]) != null ? a : "");
        m.length > 0 && e.push(m);
    } return e; }
    function Yo(t) { var e = Xo(t), n = Oe(e); return Oe(n); }
    function Ko(t, e, n, r) {
        var e_39, _a;
        var a, d, p, m, x, c;
        var i = Oe((d = xi.get(String((a = t.key) != null ? a : ""))) != null ? d : []), o = Oe(String((m = (p = t.attrs) == null ? void 0 : p["data-trueos-srcdoc-text"]) != null ? m : "").split("\n").map(function (T) { return le(T); })), s = i.length > 0 ? i : o.length > 0 ? o : Yo(String((c = (x = t.attrs) == null ? void 0 : x.srcdoc) != null ? c : "")), l = n + 48;
        try {
            for (var s_2 = __values(s), s_2_1 = s_2.next(); !s_2_1.done; s_2_1 = s_2.next()) {
                var T = s_2_1.value;
                if (r.length >= Ce)
                    return;
                r.push({ x: e + 16, y: l, text: T }), l += 32;
            }
        }
        catch (e_39_1) { e_39 = { error: e_39_1 }; }
        finally {
            try {
                if (s_2_1 && !s_2_1.done && (_a = s_2.return)) _a.call(s_2);
            }
            finally { if (e_39) throw e_39.error; }
        }
    }
    function vn(t) { var e; return t.kind === "text" ? (e = t.text) != null ? e : "" : t.children.map(vn).join(" "); }
    function zo(t) { var e = [], n = function (r, i, o, s) {
        var e_40, _a;
        var g, b, _;
        if (e.length >= Ce)
            return;
        var l = i + r.x, a = o + r.y, d = r.kind === "block" && r.tagName === "iframe" && String((b = (g = r.attrs) == null ? void 0 : g["data-root"]) != null ? b : "") !== "1", p = s + (d ? 1 : 0), m = r.kind === "block" && r.tagName === "button", x = r.kind === "text" ? (_ = r.text) != null ? _ : "" : m ? vn(r) : "", c = le(Rn(x)), T = e.length;
        if (Fo(c)) {
            var w = m ? l + 8 : l, M = m ? a + Math.max(0, Math.floor((r.height - be.fontSize * 1.25) / 2)) : a;
            e.push({ x: w, y: M, text: c });
        }
        if (!m) {
            try {
                for (var _b = __values(r.children), _c = _b.next(); !_c.done; _c = _b.next()) {
                    var w = _c.value;
                    n(w, l, a, p);
                }
            }
            catch (e_40_1) { e_40 = { error: e_40_1 }; }
            finally {
                try {
                    if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
                }
                finally { if (e_40) throw e_40.error; }
            }
            d && e.length === T && Ko(r, l, a, e);
        }
    }; return n(t, 0, 0, 0), e; }
    function jo(t, e) {
        if (e === void 0) { e = 8; }
        var n = [];
        for (var r = 0; r < t.length && n.length < e; r += 1) {
            var i = t[r];
            n.push("#".concat(n.length, " x=").concat(Math.round(i.x), " y=").concat(Math.round(i.y), " text=\"").concat(Dn(i.text), "\""));
        }
        return n.join("|");
    }
    function Vo() {
        var e_41, _a;
        var i, o, s, l;
        var t = (o = (i = window.__pixiCapture) == null ? void 0 : i.commands) != null ? o : [], e = {}, n = {}, r = new Set(["addChild", "addChildAt", "setChildIndex", "removeChild", "removeChildren", "removeAllListeners", "on", "clear", "rect", "roundRect", "circle", "ellipse", "moveTo", "lineTo", "closePath", "poly", "fill", "stroke", "image", "visible", "alpha", "scale", "mask", "text.text.set", "text.style.set", "text.resolution.set", "text.setSize", "render", "snapshot"]);
        try {
            for (var t_4 = __values(t), t_4_1 = t_4.next(); !t_4_1.done; t_4_1 = t_4.next()) {
                var a = t_4_1.value;
                var d = Ue(a == null ? void 0 : a.op);
                d && (e[d] = ((s = e[d]) != null ? s : 0) + 1, r.has(d) || (n[d] = ((l = n[d]) != null ? l : 0) + 1));
            }
        }
        catch (e_41_1) { e_41 = { error: e_41_1 }; }
        finally {
            try {
                if (t_4_1 && !t_4_1.done && (_a = t_4.return)) _a.call(t_4);
            }
            finally { if (e_41) throw e_41.error; }
        }
        return { total: t.length, ops: Pn(e, 24), unsupported: Pn(n, 24) };
    }
    function Jo(t, e, n, r) { if (!Vt())
        return; var i = Vo(); window.__TRUEOS_PIXI_BRIDGE_STATS__ = { renderNodes: t.nodes, renderBlocks: t.blocks, renderText: t.text, renderTags: Pn(t.tags, 24), renderTextSamples: n, layoutBoxes: e.nodes, layoutBlocks: e.blocks, layoutText: e.text, layoutMaxDepth: e.maxDepth, layoutTextSamples: r, measureTextCalls: On, scrollbarVisible: u.scroll.track.h > 0 ? 1 : 0, scrollbarTrack: "".concat(Math.round(u.scroll.track.x), ",").concat(Math.round(u.scroll.track.y), ",").concat(Math.round(u.scroll.track.w), ",").concat(Math.round(u.scroll.track.h)), scrollbarThumb: "".concat(Math.round(u.scroll.thumb.x), ",").concat(Math.round(u.scroll.thumb.y), ",").concat(Math.round(u.scroll.thumb.w), ",").concat(Math.round(u.scroll.thumb.h)), pixiCommands: i.total, pixiOps: i.ops, pixiUnsupported: i.unsupported }; }
    var gi = new WeakMap;
    function Nn(t, e) { var n = t; for (; n;) {
        if (n === e)
            return !0;
        n = n.parent;
    } return !1; }
    function Ei(t) { return Array.isArray(t.children) || (t.children = []), t.children; }
    function oe(t, e, n) { var r = Number(e) || 0, i = Number(n) || 0; (!t.position || typeof t.position != "object") && (t.position = { x: 0, y: 0 }), t.position.x = r, t.position.y = i; }
    function $e(t, e, n) { if (e === t || Nn(t, e))
        return; var r = Ei(t); if (e.parent !== t) {
        var l = Math.max(0, Math.min(n, r.length));
        t.addChildAt(e, l);
        return;
    } var i = Math.max(0, r.length - 1), o = Math.max(0, Math.min(n, i)); t.getChildIndex(e) !== o && t.setChildIndex(e, o); }
    function bi(t, e, n) { if (e === t || Nn(t, e))
        return; var r = Ei(t); if (e.parent !== t) {
        var l = Math.max(0, Math.min(n, r.length));
        t.addChildAt(e, l);
        return;
    } var i = Math.max(0, r.length - 1), o = Math.max(0, Math.min(n, i)); t.getChildIndex(e) !== o && t.setChildIndex(e, o); }
    var dn = null, ot = null;
    function jt(t) { var e = u.cursorColors.get(t); if (e != null)
        return e; var n = [1118481, 2450411, 1483594, 14427686, 8141549, 959977, 16096779], r = Math.abs(Number(t) || 0) % n.length, i = n[r]; return u.cursorColors.set(t, i), i; }
    function Ft(t) { var i, o, s, l, a, d; var e = Number((s = (o = t == null ? void 0 : t.pointerId) != null ? o : (i = t == null ? void 0 : t.data) == null ? void 0 : i.pointerId) != null ? s : 0), r = String((d = (a = t == null ? void 0 : t.pointerType) != null ? a : (l = t == null ? void 0 : t.data) == null ? void 0 : l.pointerType) != null ? d : "").toLowerCase() === "mouse" || e === 1 || e === u.primaryMousePointerId; return u.harness.enabled && r ? u.harness.activeUserPointerId : e; }
    function Vt() { return !!globalThis.__TRUEOS_CAPTURE_ONLY__; }
    function kt(t) { Vt() && (window.__TRUEOS_PIXI_APP_PHASE__ = t); }
    function L(t) { Vt() && (window.__TRUEOS_PIXI_LAYOUT_STEP__ = t); }
    function ki(t) { var l, a, d, p, m; var e = (l = window.__TRUEOS_PIXI_APP_PHASE__) != null ? l : "unknown", n = (a = window.__TRUEOS_PIXI_LAYOUT_STEP__) != null ? a : "", r = t, i = String((d = r == null ? void 0 : r.name) != null ? d : "Error"), o = String((p = r == null ? void 0 : r.message) != null ? p : t), s = String((m = r == null ? void 0 : r.stack) != null ? m : ""); return "phase=".concat(e, " layout=").concat(n, " name=").concat(i, " message=").concat(o, " stack=").concat(s); }
    function Zo() { var t = Math.max(1, Number(window.innerWidth || 1920) | 0), e = Math.max(1, Number(window.innerHeight || 1080) | 0), n = new gt(0, 0, t, e), r = document.createElement("canvas"), i = { width: t, height: e, screen: n, render: function (o) { return o; }, resize: function (o, s) { var l = Math.max(1, Number(o || t) | 0), a = Math.max(1, Number(s || e) | 0); this.width = l, this.height = a, n.width = l, n.height = a; } }; return { stage: new _t, screen: n, canvas: r, renderer: i, ticker: { stop: function () { }, add: function () { }, remove: function () { } } }; }
    function Qo() { var l = /** @class */ (function () {
        function l() {
            et(this, "children");
            et(this, "measureFunc");
            et(this, "paddingLeft");
            et(this, "paddingTop");
            et(this, "paddingRight");
            et(this, "paddingBottom");
            et(this, "marginLeft");
            et(this, "marginTop");
            et(this, "marginRight");
            et(this, "marginBottom");
            et(this, "width");
            et(this, "height");
            et(this, "minWidth");
            et(this, "minHeight");
            et(this, "flexDirection");
            et(this, "computed");
            this.children = [], this.measureFunc = null, this.paddingLeft = 0, this.paddingTop = 0, this.paddingRight = 0, this.paddingBottom = 0, this.marginLeft = 0, this.marginTop = 0, this.marginRight = 0, this.marginBottom = 0, this.width = 0, this.height = 0, this.minWidth = 0, this.minHeight = 0, this.flexDirection = 0, this.computed = { left: 0, top: 0, width: 0, height: 0 };
        }
        l.create = function () { return new l; };
        l.prototype.setMeasureFunc = function (d) { this.measureFunc = d; };
        l.prototype.setMargin = function (d, p) { var m = Number(p) || 0; d === 0 ? this.marginLeft = m : d === 1 ? this.marginTop = m : d === 2 ? this.marginRight = m : d === 3 && (this.marginBottom = m); };
        l.prototype.setPadding = function (d, p) { var m = Number(p) || 0; d === 0 ? this.paddingLeft = m : d === 1 ? this.paddingTop = m : d === 2 ? this.paddingRight = m : d === 3 && (this.paddingBottom = m); };
        l.prototype.setFlexDirection = function (d) { this.flexDirection = d; };
        l.prototype.setAlignItems = function (d) { };
        l.prototype.setJustifyContent = function (d) { };
        l.prototype.setFlexWrap = function (d) { };
        l.prototype.setFlexGrow = function (d) { };
        l.prototype.setFlexShrink = function (d) { };
        l.prototype.setAlignSelf = function (d) { };
        l.prototype.setPositionType = function (d) { };
        l.prototype.setPosition = function (d, p) { };
        l.prototype.setWidth = function (d) { this.width = Math.max(0, Number(d) || 0); };
        l.prototype.setHeight = function (d) { this.height = Math.max(0, Number(d) || 0); };
        l.prototype.setMinWidth = function (d) { this.minWidth = Math.max(0, Number(d) || 0); };
        l.prototype.setMinHeight = function (d) { this.minHeight = Math.max(0, Number(d) || 0); };
        l.prototype.insertChild = function (d, p) { this.children.splice(Math.max(0, Math.min(p, this.children.length)), 0, d); };
        l.prototype.getChildCount = function () { return this.children.length; };
        l.prototype.getComputedLeft = function () { return this.computed.left; };
        l.prototype.getComputedTop = function () { return this.computed.top; };
        l.prototype.getComputedWidth = function () { return this.computed.width; };
        l.prototype.getComputedHeight = function () { return this.computed.height; };
        l.prototype.freeRecursive = function () { };
        l.prototype.calculateLayout = function (d, p) {
            if (d === void 0) { d = this.width; }
            if (p === void 0) { p = this.height; }
            this.layout(0, 0, Math.max(1, Number(d) || this.width || 1), Math.max(1, Number(p) || this.height || 1));
        };
        l.prototype.layout = function (d, p, m, x) {
            var e_42, _a, e_43, _b;
            var c = this.paddingLeft + this.paddingRight, T = this.paddingTop + this.paddingBottom, g = Math.max(this.minWidth, this.width || m), b = Math.max(this.minHeight, this.height || 0);
            if (this.computed.left = d, this.computed.top = p, this.computed.width = g, this.measureFunc) {
                var _ = this.measureFunc(Math.max(0, g - c), 0);
                b = Math.max(b, Math.ceil(Number(_.height) || 0) + T), this.computed.height = b;
                return;
            }
            if (this.flexDirection === 1) {
                var _ = this.paddingLeft, w = 0;
                try {
                    for (var _c = __values(this.children), _d = _c.next(); !_d.done; _d = _c.next()) {
                        var M = _d.value;
                        var $ = M.width || M.minWidth || Math.max(24, (g - c) / Math.max(1, this.children.length));
                        M.layout(_ + M.marginLeft, this.paddingTop + M.marginTop, $, x), _ += M.computed.width + M.marginLeft + M.marginRight, w = Math.max(w, M.computed.height + M.marginTop + M.marginBottom);
                    }
                }
                catch (e_42_1) { e_42 = { error: e_42_1 }; }
                finally {
                    try {
                        if (_d && !_d.done && (_a = _c.return)) _a.call(_c);
                    }
                    finally { if (e_42) throw e_42.error; }
                }
                b = Math.max(b, w + T);
            }
            else {
                var _ = this.paddingTop;
                try {
                    for (var _f = __values(this.children), _g = _f.next(); !_g.done; _g = _f.next()) {
                        var w = _g.value;
                        var M = Math.max(0, g - c - w.marginLeft - w.marginRight);
                        w.layout(this.paddingLeft + w.marginLeft, _ + w.marginTop, M, x), _ += w.computed.height + w.marginTop + w.marginBottom;
                    }
                }
                catch (e_43_1) { e_43 = { error: e_43_1 }; }
                finally {
                    try {
                        if (_g && !_g.done && (_b = _f.return)) _b.call(_f);
                    }
                    finally { if (e_43) throw e_43.error; }
                }
                b = Math.max(b, _ + this.paddingBottom);
            }
            this.computed.height = Math.max(this.minHeight, b);
        };
        return l;
    }()); return { Node: l, EDGE_LEFT: 0, EDGE_TOP: 1, EDGE_RIGHT: 2, EDGE_BOTTOM: 3, FLEX_DIRECTION_COLUMN: 0, FLEX_DIRECTION_ROW: 1, FLEX_DIRECTION_ROW_REVERSE: 1, ALIGN_STRETCH: 0, ALIGN_CENTER: 1, ALIGN_FLEX_START: 2, JUSTIFY_CENTER: 0, JUSTIFY_FLEX_START: 1, JUSTIFY_SPACE_BETWEEN: 2, WRAP_WRAP: 1, WRAP_NO_WRAP: 0, POSITION_TYPE_ABSOLUTE: 1, DIRECTION_LTR: 0, MEASURE_MODE_UNDEFINED: 0 }; }
    function qo(t) {
        var e_44, _a;
        var r;
        var e = 0, n = function (i, o, s) {
            var e_45, _a;
            var d;
            var l = o + i.x, a = s + i.y;
            if (!(i.kind === "block" && i.tagName === "dialog")) {
                e = Math.max(e, a + i.height);
                try {
                    for (var _b = __values((d = i.children) != null ? d : []), _c = _b.next(); !_c.done; _c = _b.next()) {
                        var p = _c.value;
                        n(p, l, a);
                    }
                }
                catch (e_45_1) { e_45 = { error: e_45_1 }; }
                finally {
                    try {
                        if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
                    }
                    finally { if (e_45) throw e_45.error; }
                }
            }
        };
        try {
            for (var _b = __values((r = t.children) != null ? r : []), _c = _b.next(); !_c.done; _c = _b.next()) {
                var i = _c.value;
                n(i, 0, 0);
            }
        }
        catch (e_44_1) { e_44 = { error: e_44_1 }; }
        finally {
            try {
                if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
            }
            finally { if (e_44) throw e_44.error; }
        }
        return e;
    }
    function un(t, e) { var o, s, l, a; var n = u.inputs.get(t); if (n)
        return n; var r = {}, i = ((o = e == null ? void 0 : e.type) != null ? o : "text").toLowerCase(); if (i === "checkbox" || i === "radio") {
        if (r.checked = e ? Object.prototype.hasOwnProperty.call(e, "checked") : !1, i === "checkbox") {
            var d = ((s = e == null ? void 0 : e["aria-checked"]) != null ? s : "").toLowerCase(), p = ((l = e == null ? void 0 : e["data-indeterminate"]) != null ? l : "").toLowerCase();
            r.indeterminate = (e ? Object.prototype.hasOwnProperty.call(e, "indeterminate") : !1) || d === "mixed" || p === "true" || p === "1" || p === "yes";
        }
    }
    else
        r.value = (a = e == null ? void 0 : e.value) != null ? a : ""; return u.inputs.set(t, r), r; }
    function ts(t) { var e = new Map; function n(r) {
        var e_46, _a;
        var i, o, s, l, a;
        if (r.kind === "block" && r.tagName === "input" && ((o = (i = r.attrs) == null ? void 0 : i.type) != null ? o : "text").toLowerCase() === "radio") {
            var m = "radio:".concat((l = (s = r.attrs) == null ? void 0 : s.name) != null ? l : "__default__"), x = r.key;
            if (x) {
                var c = (a = e.get(m)) != null ? a : [];
                c.push(x), e.set(m, c);
            }
        }
        try {
            for (var _b = __values(r.children), _c = _b.next(); !_c.done; _c = _b.next()) {
                var d = _c.value;
                n(d);
            }
        }
        catch (e_46_1) { e_46 = { error: e_46_1 }; }
        finally {
            try {
                if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
            }
            finally { if (e_46) throw e_46.error; }
        }
    } return n(t), e; }
    function le(t) { var e = "", n = !1, r = String(t != null ? t : ""); for (var i = 0; i < r.length; i += 1) {
        var o = r.charCodeAt(i);
        if (o === 32 || o === 9 || o === 10 || o === 13 || o === 12) {
            n = !0;
            continue;
        }
        n && e.length > 0 && (e += " "), e += r.charAt(i), n = !1;
    } return e; }
    function es(t) {
        var e_47, _a;
        if (!t || typeof t != "object")
            return;
        var e = {};
        try {
            for (var _b = __values(Object.entries(t)), _c = _b.next(); !_c.done; _c = _b.next()) {
                var _d = __read(_c.value, 2), n = _d[0], r = _d[1];
                typeof n != "string" || n.length === 0 || (e[n] = String(r != null ? r : ""));
            }
        }
        catch (e_47_1) { e_47 = { error: e_47_1 }; }
        finally {
            try {
                if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
            }
            finally { if (e_47) throw e_47.error; }
        }
        return Object.keys(e).length > 0 ? e : void 0;
    }
    function Si(t, e, n) { var d, p, m, x, c, T; if (!t || typeof t != "object")
        return null; var r = t, i = String((d = r.kind) != null ? d : ""); if (i === "text") {
        var g = String((p = r.text) != null ? p : ""), b = le(Rn(g));
        if (g.indexOf("<truesurfer-") >= 0 || g.indexOf("__trueo") >= 0 || b.startsWith("<truesurfer-") || b.startsWith("__trueo")) {
            var w = (m = n == null ? void 0 : n.rows[n.index]) != null ? m : "";
            n && (n.index += 1), b = w;
        }
        else if (b.length === 0) {
            var w = (x = n == null ? void 0 : n.rows[n.index]) != null ? x : "";
            n && w && (n.index += 1), w && (b = w);
        }
        return b.length > 0 ? { kind: "text", text: b } : null;
    } if (i !== "block")
        return null; var o = String((c = r.tagName) != null ? c : "").toLowerCase(); if (o.length === 0)
        return null; var s = String((T = r.key) != null ? T : "".concat(e, ":").concat(o)), l = [], a = Array.isArray(r.children) ? r.children : []; for (var g = 0; g < a.length; g += 1) {
        var b = Si(a[g], "".concat(e, ".").concat(g), n);
        b && l.push(b);
    } return { kind: "block", key: s, tagName: o, attrs: es(r.attrs), children: l }; }
    function ns(t, e) { var n = Array.isArray(t) ? t : t && typeof t == "object" && Array.isArray(t.widgetRenderTree) ? t.widgetRenderTree : [], i = { rows: Array.isArray(e) ? Oe(e) : Mi(e), index: 0 }, o = []; for (var s = 0; s < n.length; s += 1) {
        var l = Si(n[s], "0.".concat(s), i);
        l && o.push(l);
    } return o; }
    function rs(t) { var n = document.createElement("canvas").getContext("2d"); if (!n)
        throw new Error("2D canvas not available"); n.font = t; var r = t.indexOf("px"), i = r; for (; i > 0;) {
        var l = t.charCodeAt(i - 1);
        if (l < 48 || l > 57)
            break;
        i -= 1;
    } var o = r > i ? Number(t.slice(i, r)) : 16, s = Math.ceil(o * 1.25); return { measure: function (l, a) {
            var e_48, _a;
            On += 1;
            var d = le(l).split(" ").filter(Boolean);
            if (d.length === 0)
                return { width: 0, height: s, lines: [""] };
            var p = [], m = "";
            try {
                for (var d_1 = __values(d), d_1_1 = d_1.next(); !d_1_1.done; d_1_1 = d_1.next()) {
                    var T = d_1_1.value;
                    var g = m ? "".concat(m, " ").concat(T) : T, b = n.measureText(g).width, _ = a != null ? a : Number.POSITIVE_INFINITY;
                    b <= _ || !m ? m = g : (p.push(m), m = T);
                }
            }
            catch (e_48_1) { e_48 = { error: e_48_1 }; }
            finally {
                try {
                    if (d_1_1 && !d_1_1.done && (_a = d_1.return)) _a.call(d_1);
                }
                finally { if (e_48) throw e_48.error; }
            }
            m && p.push(m);
            var x = Math.min(Math.max.apply(Math, __spreadArray([], __read(p.map(function (T) { return n.measureText(T).width; })), false)), a != null ? a : Number.POSITIVE_INFINITY), c = p.length * s;
            return { width: Math.ceil(x), height: Math.ceil(c), lines: p };
        }, lineHeight: s, font: t }; }
    function is(t, e, n) { var x; L("build:start nodes=".concat(t.length, " viewport=").concat(e, "x").concat(n)); var r = 12, i = 8, o = be; L("build:measurer"); var s = rs("".concat(o.fontSize, "px ").concat(o.fontFamily)); function l(c) { return c.kind !== "block" || c.tagName === "hr" || c.tagName === "tr" || c.tagName === "td" || c.tagName === "th" ? 0 : i; } function a(c) { var T = c.kind === "text" ? "text:".concat(c.text.slice(0, 24)) : "".concat(c.tagName, ":").concat(c.key); if (L("node:".concat(T, ":start")), c.kind === "text") {
        var w_1 = rt.Node.create();
        return L("node:".concat(T, ":measure-func")), w_1.setMeasureFunc(function (M, $) { L("node:".concat(T, ":measure-call")); var A = $ === rt.MEASURE_MODE_UNDEFINED ? void 0 : Math.max(0, M), H = s.measure(c.text, A); return { width: H.width, height: H.height }; }), w_1.setMargin(rt.EDGE_RIGHT, 6), w_1.setMargin(rt.EDGE_BOTTOM, 0), { yogaNode: w_1, buildBox: function () { return ({ kind: "text", text: c.text, x: w_1.getComputedLeft(), y: w_1.getComputedTop(), width: w_1.getComputedWidth(), height: w_1.getComputedHeight(), children: [] }); } };
    } if (c.tagName === "sliderlabel")
        return L("node:".concat(c.tagName, ":").concat(c.key, ":sliderlabel")), tr({ node: c, Yoga: rt, measurer: s }); L("node:".concat(c.tagName, ":").concat(c.key, ":create")); var g = rt.Node.create(); if (L("node:".concat(c.tagName, ":").concat(c.key, ":base-defaults")), g.setFlexDirection(rt.FLEX_DIRECTION_COLUMN), g.setAlignItems(rt.ALIGN_STRETCH), g.setPadding(rt.EDGE_LEFT, r), g.setPadding(rt.EDGE_RIGHT, r), g.setPadding(rt.EDGE_TOP, r), g.setPadding(rt.EDGE_BOTTOM, r), g.setMargin(rt.EDGE_BOTTOM, 0), wn(c.tagName) && (L("node:".concat(c.tagName, ":").concat(c.key, ":heading-defaults")), pr(g, rt)), c.tagName === "hr" && (L("node:".concat(c.tagName, ":").concat(c.key, ":hr-defaults")), ar(g, rt)), (c.tagName === "p" || c.tagName === "label") && (L("node:".concat(c.tagName, ":").concat(c.key, ":inline-scan")), c.children.some(function (M) { return M.kind === "block" && (M.tagName === "input" || M.tagName === "button" || M.tagName === "select" || M.tagName === "textarea" || M.tagName === "timeinput" || M.tagName === "dateinput" || M.tagName === "monthinput" || M.tagName === "weekinput" || M.tagName === "datetimelocalinput" || M.tagName === "progress" || M.tagName === "meter" || M.tagName === "slider" || M.tagName === "number" || M.tagName === "color"); }) && (g.setFlexDirection(rt.FLEX_DIRECTION_ROW), g.setFlexWrap(rt.WRAP_WRAP), g.setAlignItems(rt.ALIGN_CENTER)), g.setPadding(rt.EDGE_TOP, 4), g.setPadding(rt.EDGE_BOTTOM, 4), g.setPadding(rt.EDGE_LEFT, 4), g.setPadding(rt.EDGE_RIGHT, 4)), c.tagName === "table" && (L("node:".concat(c.tagName, ":").concat(c.key, ":table-defaults")), hr(g, rt)), c.tagName === "tr" && (L("node:".concat(c.tagName, ":").concat(c.key, ":tr-defaults")), mr(g, rt)), (c.tagName === "td" || c.tagName === "th") && (L("node:".concat(c.tagName, ":").concat(c.key, ":cell-defaults")), fr(g, rt)), c.tagName === "input" && (L("node:".concat(c.tagName, ":").concat(c.key, ":input-defaults")), Gr(g, c, rt)), c.tagName === "textarea" && (L("node:".concat(c.tagName, ":").concat(c.key, ":textarea-defaults")), Fr(g, rt)), c.tagName === "select" && (L("node:".concat(c.tagName, ":").concat(c.key, ":select-defaults")), Qr(g, rt)), c.tagName === "timeinput" || c.tagName === "dateinput" || c.tagName === "monthinput" || c.tagName === "weekinput" || c.tagName === "datetimelocalinput") {
        var w = c.tagName === "timeinput" ? "time" : c.tagName === "monthinput" ? "month" : c.tagName === "weekinput" ? "week" : c.tagName === "dateinput" ? "date" : "datetime-local";
        L("node:".concat(c.tagName, ":").concat(c.key, ":temporal-defaults")), ei(g, rt, w);
    } c.tagName === "img" && (L("node:".concat(c.tagName, ":").concat(c.key, ":img-defaults")), Er(g, c, rt)), c.tagName === "svg" && (L("node:".concat(c.tagName, ":").concat(c.key, ":svg-defaults")), Or(g, c, rt)), c.tagName === "canvas" && (L("node:".concat(c.tagName, ":").concat(c.key, ":canvas-defaults")), Dr(g, c, rt)), c.tagName === "iframe" && (L("node:".concat(c.tagName, ":").concat(c.key, ":iframe-defaults")), vr(g, c, rt)), c.tagName === "button" && (L("node:".concat(c.tagName, ":").concat(c.key, ":button-defaults")), cr(g, rt)), c.tagName === "dialog" && (L("node:".concat(c.tagName, ":").concat(c.key, ":dialog-defaults")), Xr(g, rt)), c.tagName === "number" && (L("node:".concat(c.tagName, ":").concat(c.key, ":number-defaults")), Kr(g, rt)), c.tagName === "color" && (L("node:".concat(c.tagName, ":").concat(c.key, ":color-defaults")), Vr(g, c, rt)), c.tagName === "searchrow" && (L("node:".concat(c.tagName, ":").concat(c.key, ":searchrow-defaults")), Hr(g, rt)), c.tagName === "searchbutton" && (L("node:".concat(c.tagName, ":").concat(c.key, ":searchbutton-defaults")), $r(g, rt)), c.tagName === "summary" && (L("node:".concat(c.tagName, ":").concat(c.key, ":summary-defaults")), rr(g, rt)), c.tagName === "details" && (L("node:".concat(c.tagName, ":").concat(c.key, ":details-defaults")), ir(g, rt)), c.tagName === "barrow" && (L("node:".concat(c.tagName, ":").concat(c.key, ":barrow-defaults")), Wr(g, rt)), (c.tagName === "progress" || c.tagName === "meter") && (L("node:".concat(c.tagName, ":").concat(c.key, ":progress-defaults")), Qn(g, rt)), c.tagName === "slider" && (L("node:".concat(c.tagName, ":").concat(c.key, ":slider-defaults")), qn(g, rt)), L("node:".concat(c.tagName, ":").concat(c.key, ":children-effective")); var b = or(c, u.detailsOpen); L("node:".concat(c.tagName, ":").concat(c.key, ":children-map count=").concat(b.length)); var _ = b.map(a); L("node:".concat(c.tagName, ":").concat(c.key, ":children-insert")); for (var w = 0; w < _.length; w++) {
        var M = b[w], $ = _[w];
        if (M && M.kind === "block") {
            var A = w === _.length - 1 ? 0 : l(M);
            $.yogaNode.setMargin(rt.EDGE_BOTTOM, A);
        }
        g.insertChild($.yogaNode, g.getChildCount());
    } return { yogaNode: g, buildBox: function () { return ({ kind: "block", key: c.key, tagName: c.tagName, attrs: c.attrs, x: g.getComputedLeft(), y: g.getComputedTop(), width: g.getComputedWidth(), height: g.getComputedHeight(), children: _.map(function (w) { return w.buildBox(); }) }); } }; } var d = rt.Node.create(); L("root:flex-direction"), d.setFlexDirection(rt.FLEX_DIRECTION_COLUMN), L("root:align-items"), d.setAlignItems(rt.ALIGN_STRETCH), L("root:width"), d.setWidth(e), L("root:height"), d.setHeight(n), L("root:padding-left"), d.setPadding(rt.EDGE_LEFT, 16), L("root:padding-top"), d.setPadding(rt.EDGE_TOP, 16), L("root:padding-right"), d.setPadding(rt.EDGE_RIGHT, 16 + Cn), L("root:padding-bottom"), d.setPadding(rt.EDGE_BOTTOM, 16), L("root:children-map count=".concat(t.length)); var p = t.map(a); L("root:children-insert"); for (var c = 0; c < p.length; c++) {
        var T = t[c], g = p[c];
        if (T && T.kind === "block") {
            var b = c === p.length - 1 ? 0 : l(T);
            g.yogaNode.setMargin(rt.EDGE_BOTTOM, b);
        }
        d.insertChild(g.yogaNode, d.getChildCount());
    } L("root:calculate"), d.calculateLayout(e, n, rt.DIRECTION_LTR), L("root:build-box"); var m = { kind: "block", tagName: "root", x: 0, y: 0, width: d.getComputedWidth(), height: d.getComputedHeight(), children: p.map(function (c) { return c.buildBox(); }) }; return L("root:free"), (x = d.freeRecursive) == null || x.call(d), L("build:done"), m; }
    function os(t, e, n) {
        var e_49, _a, e_50, _b, e_51, _c, e_52, _d, e_53, _f;
        var U, G;
        L("render:start");
        var r = be, i = n != null ? n : t.stage;
        L("render:get-background");
        var o = St(i, "__background");
        L("render:get-content-root");
        var s = xe(i, "__contentRoot");
        L("render:get-dialog-root");
        var l = xe(i, "__dialogRoot");
        L("render:get-overlay-root");
        var a = xe(i, "__overlayRoot");
        L("render:ensure-background"), bi(i, o, 0), L("render:ensure-content-root"), $e(i, s, 1), L("render:ensure-dialog-root"), $e(i, l, 2), L("render:ensure-overlay-root"), $e(i, a, 3), L("render:overlay-remove-children"), a.removeChildren(), L("render:overlay-removed");
        var d = [], p = [], m = ts(e);
        L("render:clear-ui-state"), u.fieldBounds.clear(), u.sliderBounds.clear(), u.dialogDragBounds.clear(), u.hoverRects.length = 0, u.hoverHandlers.clear(), u.iframeRects.length = 0, L("render:node-cache");
        var x = (U = gi.get(i)) != null ? U : new Map;
        gi.set(i, x);
        var c = new Set, T = function (h) {
            var e_54, _a;
            var q;
            var P = 0, F = function (Z, tt, Y) {
                var e_55, _a;
                var f;
                if (Z.kind === "block" && Z.tagName === "dialog")
                    return;
                var j = tt + Z.x, v = Y + Z.y;
                P = Math.max(P, v + Z.height);
                try {
                    for (var _b = __values((f = Z.children) != null ? f : []), _c = _b.next(); !_c.done; _c = _b.next()) {
                        var I = _c.value;
                        F(I, j, v);
                    }
                }
                catch (e_55_1) { e_55 = { error: e_55_1 }; }
                finally {
                    try {
                        if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
                    }
                    finally { if (e_55) throw e_55.error; }
                }
            };
            try {
                for (var _b = __values((q = h.children) != null ? q : []), _c = _b.next(); !_c.done; _c = _b.next()) {
                    var Z = _c.value;
                    F(Z, 0, 0);
                }
            }
            catch (e_54_1) { e_54 = { error: e_54_1 }; }
            finally {
                try {
                    if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
                }
                finally { if (e_54) throw e_54.error; }
            }
            return P;
        }, g = new Set;
        try {
            for (var _g = __values(u.textDrags.values()), _h = _g.next(); !_h.done; _h = _g.next()) {
                var h = _h.value;
                g.add(h.key);
            }
        }
        catch (e_49_1) { e_49 = { error: e_49_1 }; }
        finally {
            try {
                if (_h && !_h.done && (_a = _g.return)) _a.call(_g);
            }
            finally { if (e_49) throw e_49.error; }
        }
        L("render:measure");
        var b = vo(r);
        function _(h, P, F) { return Math.max(P, Math.min(F, h)); }
        var w = function (h) {
            var e_56, _a;
            try {
                for (var _b = __values(u.textDrags.entries()), _c = _b.next(); !_c.done; _c = _b.next()) {
                    var _d = __read(_c.value, 2), P = _d[0], F = _d[1];
                    if (F.key === h)
                        return P;
                }
            }
            catch (e_56_1) { e_56 = { error: e_56_1 }; }
            finally {
                try {
                    if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
                }
                finally { if (e_56) throw e_56.error; }
            }
            return null;
        }, M = function (h) {
            var e_57, _a;
            var P = u.keyboardOwnerPointerId;
            if (u.focusedKeyByPointer.get(P) === h)
                return P;
            try {
                for (var _b = __values(u.focusedKeyByPointer.entries()), _c = _b.next(); !_c.done; _c = _b.next()) {
                    var _d = __read(_c.value, 2), F = _d[0], q = _d[1];
                    if (q === h)
                        return F;
                }
            }
            catch (e_57_1) { e_57 = { error: e_57_1 }; }
            finally {
                try {
                    if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
                }
                finally { if (e_57) throw e_57.error; }
            }
            return null;
        };
        L("render:background-clear"), Et(o), L("render:background-rect"), o.rect(0, 0, t.renderer.width, t.renderer.height), L("render:background-fill"), o.fill(r.background), L("render:content-position");
        {
            var h = u.scroll, P = h && Number(h.y || 0) || 0;
            if (P !== 0) {
                var F = s.position;
                F && (F.x = 0, F.y = -P);
            }
        }
        L("render:content-position-done");
        function $(h, P, F, q, Z, tt, Y, j, v) {
            var e_58, _a;
            if (q === void 0) { q = 0; }
            if (Z === void 0) { Z = 0; }
            var D, R, B, X, z, K, V, nt, Q, it, st, dt, yt, Rt, Gt, at, Mt, Dt, Ct, vt;
            L("render:draw:".concat(j, ":").concat(h.kind, ":").concat(h.kind === "block" ? h.tagName : "text", ":start"));
            var f = h.kind === "block" ? h.key && h.key.length > 0 ? h.key : "".concat(j, ":").concat((D = h.tagName) != null ? D : "block") : "", I = h.kind === "block" ? "b:".concat(f) : "t:".concat(j);
            L("render:draw:".concat(j, ":cache"));
            var O = x.get(I);
            (!O || Nn(P, O)) && (L("render:draw:".concat(j, ":new-container")), O = new _t, O.label = I, x.set(I, O)), L("render:draw:".concat(j, ":ensure-child")), c.add(I), $e(P, O, v), L("render:draw:".concat(j, ":children-root"));
            var C = xe(O, "__children");
            if (L("render:draw:".concat(j, ":ensure-children-root")), $e(O, C, 1), L("render:draw:".concat(j, ":position")), oe(O, h.x, h.y), h.kind === "block" && h.tagName === "hr" && oe(O, Math.round(h.x), Math.round(h.y)), h.kind === "block" && h.tagName === "dialog" && h.key) {
                var mt = Qe(u.dialogs, h.key), ft = Math.max(0, h.width), ut = Math.max(0, h.height), lt = Y.x, Xt = Y.y, Nt = Math.max(lt, Y.x + Y.w - ft), Bt = Math.max(Xt, Y.y + Y.h - ut);
                if (u.dialogDragBounds.set(h.key, { minX: lt, minY: Xt, maxX: Nt, maxY: Bt }), Vt() && !mt.__trueosInitialPositionSeeded) {
                    var se = Y.w <= 760 && Y.h <= 800, ct = lt + Math.max(12, Math.floor((Y.w - ft) / 2)), pt = Xt + Math.max(se ? 190 : 40, Math.floor((Y.h - ut) / 2));
                    mt.x = Math.max(lt, Math.min(Nt, ct)), mt.y = Math.max(Xt, Math.min(Bt, pt)), mt.__trueosInitialPositionSeeded = !0;
                }
                mt.x = Math.max(lt, Math.min(Nt, mt.x)), mt.y = Math.max(Xt, Math.min(Bt, mt.y)), oe(O, mt.x, mt.y);
            }
            var y = q + O.position.x, S = Z + O.position.y;
            if (h.kind === "block") {
                L("render:draw:".concat(j, ":block:").concat(h.tagName, ":begin"));
                var mt = F;
                (h.tagName === "h1" || h.tagName === "h2" || h.tagName === "h3" || h.tagName === "summary" || h.tagName === "th") && (mt = { bold: !0 }), L("render:draw:".concat(j, ":graphics"));
                var ft = St(O, "__g");
                L("render:draw:".concat(j, ":graphics-clear")), Et(ft), L("render:draw:".concat(j, ":graphics-ensure")), bi(O, ft, 0), ft.zIndex = -10;
                var ut = Math.max(0, h.width), lt = Math.max(0, h.height), Xt = null;
                if ((h.tagName === "h1" || h.tagName === "h2" || h.tagName === "h3") && (oe(O, Math.round(h.x), Math.round(h.y)), ut = Math.round(ut), lt = Math.round(lt)), L("render:draw:".concat(j, ":widget:").concat(h.tagName)), h.tagName === "hr")
                    sr({ graphics: ft, w: ut, theme: r });
                else if (h.tagName !== "barrow") {
                    if (h.tagName !== "searchrow") {
                        if (h.tagName === "searchbutton")
                            Ur({ node: h, container: O, graphics: ft, w: ut, h: lt, theme: r, uiState: u, getPointerId: Ft, focusInputKey: (R = h.attrs) == null ? void 0 : R["data-focus-key"], requestPaint: ot });
                        else if (h.tagName === "progress" || h.tagName === "meter")
                            Zn({ node: h, graphics: ft, w: ut, h: lt, theme: r });
                        else if (h.tagName === "sliderlabel")
                            er({ node: h, container: O, theme: r, sliderStates: u.sliders });
                        else if (h.tagName === "slider")
                            Ze({ node: h, container: O, graphics: ft, w: ut, h: lt, absX: y, absY: S, theme: r, sliderStates: u.sliders, sliderBounds: u.sliderBounds, sliderDrags: u.sliderDrags, requestPaint: ot, getPointerId: Ft });
                        else if (h.tagName === "timeinput" || h.tagName === "dateinput" || h.tagName === "monthinput" || h.tagName === "weekinput" || h.tagName === "datetimelocalinput")
                            ni({ node: h, container: O, graphics: ft, w: ut, h: lt, absX: y, absY: S, theme: r, uiState: u, getPointerId: Ft, getCursorColor: jt, temporalStates: u.temporals, yearSliderOwners: u.temporalYearOwners, getOrInitInputValue: function (J, bt) { return un(J, bt); }, requestPaint: ot, popupSink: p });
                        else if (h.tagName === "input") {
                            var J = h.key, bt = J != null ? M(J) : null, ee = J != null && u.focusedKeyByPointer.get(u.keyboardOwnerPointerId) === J, Ot = J == null ? null : ee ? u.keyboardOwnerPointerId : g.has(J) ? w(J) : null, ne = Ot != null, Jt = bt != null ? jt(bt) : null;
                            Lr({ node: h, container: O, graphics: ft, w: ut, h: lt, absX: y, absY: S, theme: r, textMeasure: b, uiState: u, getOrInitInputState: un, clamp: _, radioGroups: m, textDrags: u.textDrags, requestPaint: ot, showCaret: ne, caretPointerId: Ot, focusColor: Jt != null ? Jt : void 0, getCursorColor: jt, getPointerId: Ft });
                        }
                        else if (h.tagName === "textarea") {
                            var J = h.key, bt = J != null ? M(J) : null, ee = J != null && u.focusedKeyByPointer.get(u.keyboardOwnerPointerId) === J, Ot = J == null ? null : ee ? u.keyboardOwnerPointerId : g.has(J) ? w(J) : null, ne = Ot != null, Jt = bt != null ? jt(bt) : null;
                            Br({ node: h, container: O, graphics: ft, w: ut, h: lt, absX: y, absY: S, theme: r, textMeasure: b, uiState: u, getOrInitInputState: un, clamp: _, textDrags: u.textDrags, requestPaint: ot, showCaret: ne, caretPointerId: Ot, focusColor: Jt != null ? Jt : void 0, getCursorColor: jt, getPointerId: Ft });
                        }
                        else if (h.tagName === "select") {
                            if (h.key) {
                                var J = Number((X = (B = h.attrs) == null ? void 0 : B["data-selected-index"]) != null ? X : "0");
                                he(u.selects, h.key, Number.isFinite(J) ? J : 0);
                            }
                            nn({ node: h, container: O, graphics: ft, w: ut, h: lt, absX: y, absY: S, theme: r, selectStates: u.selects, uiState: u, getPointerId: Ft, getCursorColor: jt, requestPaint: ot, popupSink: d });
                        }
                        else if (h.tagName === "summary")
                            h.key && u.hoverRects.push({ key: h.key, kind: "summary", cursor: "pointer", x: y, y: S, w: ut, h: lt }), nr({ node: h, container: O, w: ut, h: lt, theme: r, detailsOpen: u.detailsOpen, requestRerender: dn });
                        else if (h.tagName === "dialog")
                            Yr({ node: h, container: O, w: ut, h: lt, theme: r, selectedBy: u.dialogSelectedBy, getCursorColor: jt, dialogStates: u.dialogs, dialogDrags: u.dialogDrags, bringToFront: function (J) { u.dialogZ.set(J, u.dialogZCounter++); }, requestPaint: ot, getPointerId: Ft });
                        else if (h.tagName === "img")
                            Mr({ node: h, container: O, graphics: ft, w: ut, h: lt, theme: r, requestRerender: dn });
                        else if (h.tagName === "svg") {
                            var J = (K = (z = h.attrs) == null ? void 0 : z["data-svg"]) != null ? K : "";
                            Rr({ svgMarkup: J, container: O, w: ut, h: lt, requestRerender: dn });
                        }
                        else if (h.tagName === "canvas")
                            Ar({ node: h, container: O, graphics: ft, w: ut, h: lt, theme: r });
                        else if (h.tagName === "iframe")
                            Nr({ node: h, container: O, graphics: ft, w: ut, h: lt, theme: r });
                        else if (h.tagName === "color")
                            u.color.bounds = { x: y, y: S, w: Math.max(0, ut), h: Math.max(0, lt) }, Zr({ node: h, container: O, graphics: ft, w: ut, h: lt, theme: r, rgb: u.color.rgb, setRgb: function (J) { u.color.rgb = J; }, alpha: u.color.a, setAlpha: function (J) { u.color.a = Math.max(0, Math.min(255, Math.round(J))); }, pick: u.color.pick, setPick: function (J) { u.color.pick = J; }, requestPaint: ot, getPointerId: Ft, setDraggingPointerId: function (J) { u.color.draggingPointerId = J; } });
                        else if (h.tagName === "number") {
                            var J_1 = h.key, bt_1 = String((nt = (V = h.attrs) == null ? void 0 : V.channel) != null ? nt : "").toLowerCase(), ee_1 = bt_1 === "r" || bt_1 === "g" || bt_1 === "b" || bt_1 === "a";
                            J_1 && zr({ node: h, container: O, graphics: ft, w: ut, h: lt, theme: r, getValue: function () { var Ot, ne; return ee_1 ? bt_1 === "a" ? (Ot = u.color.a) != null ? Ot : 255 : (ne = u.color.rgb[bt_1]) != null ? ne : 0 : Tn(u.numbers, J_1, h.attrs).value; }, setValue: function (Ot) { ee_1 ? bt_1 === "a" ? u.color.a = Math.max(0, Math.min(255, Math.round(Ot))) : u.color.rgb[bt_1] = Math.max(0, Math.min(255, Math.round(Ot))) : Tn(u.numbers, J_1, h.attrs).value = Ot; }, requestPaint: ot, numberHolds: u.numberHolds, getPointerId: Ft });
                        }
                        else if (h.tagName === "button")
                            h.key && u.hoverRects.push({ key: h.key, kind: "button", cursor: "pointer", x: y, y: S, w: ut, h: lt }), lr({ container: O, graphics: ft, w: ut, h: lt, label: le(vn(h)), theme: r, registerHoverHandlers: h.key ? function (J) { u.hoverHandlers.set(h.key, J); } : void 0 });
                        else if (!wn(h.tagName))
                            if (h.tagName === "table")
                                ur({ graphics: ft, w: ut, h: lt, boxBorder: r.boxBorder });
                            else if (h.tagName === "td" || h.tagName === "th")
                                dr({ nodeTag: h.tagName, graphics: ft, w: ut, h: lt, theme: r });
                            else {
                                var J = Math.max(0, Math.round(ut)), bt = Math.max(0, Math.round(lt));
                                ft.rect(0, 0, J, bt), ft.stroke({ width: 1, color: r.boxBorder, alignment: 0 });
                            }
                    }
                }
                L("render:draw:".concat(j, ":overlay-label")), Xt && O.addChild(Xt);
                var Nt = null, Bt = null, se = h.tagName === "iframe" && String((it = (Q = h.attrs) == null ? void 0 : Q["data-root"]) != null ? it : "") === "1";
                if (h.tagName === "iframe" && !se) {
                    h.key && u.iframeRects.push({ key: h.key, x: y, y: S, w: Math.max(0, ut), h: Math.max(0, lt) }), Nt = xe(O, "__iframeContentRoot"), oe(Nt, 0, 0);
                    var Ot = St(O, "__iframeContentMask");
                    Et(Ot);
                    var ne = 0, Jt = 34, Pi = Math.max(0, ut), Ii = Math.max(0, lt - 34);
                    Ot.rect(ne, Jt, Pi, Ii), Ot.fill(16777215), Ot.alpha = 0, Nt.mask = Ot;
                    var Xe_1 = (st = h.key) != null ? st : "", ht_1 = (dt = u.iframeScroll.get(Xe_1)) != null ? dt : { y: 0, contentHeight: 0, viewportHeight: 0, draggingPointerId: null, dragOffsetY: 0, track: { x: 0, y: 0, w: Ie, h: 0 }, thumb: { x: 0, y: 0, w: Ie, h: 0 }, rect: { x: y, y: S, w: Math.max(0, ut), h: Math.max(0, lt) } };
                    ht_1.rect = { x: y, y: S, w: Math.max(0, ut), h: Math.max(0, lt) }, ht_1.contentHeight = T(h), ht_1.viewportHeight = Math.max(0, lt - 34 - 8);
                    var _e_1 = Math.max(0, ht_1.contentHeight - ht_1.viewportHeight);
                    ht_1.y = Math.max(0, Math.min(ht_1.y, _e_1)), Bt = xe(Nt, "__iframeScrollRoot"), oe(Bt, 0, -ht_1.y);
                    var ce = St(O, "__iframeScrollbar");
                    Et(ce), ce.eventMode = "static";
                    var hn = Cn, fe = Ie, Ye = Math.max(0, ut - fe - hn), mn = 34 + hn, Re = Math.max(0, lt - 34 - hn * 2), Gn = _e_1 > .5 && Re > 1;
                    if (ce.visible = Gn, Gn) {
                        var fn = Math.max(24, (ht_1.viewportHeight || 1) / Math.max(1, ht_1.contentHeight) * Re), Ci = Math.max(1, Re - fn), Oi = _e_1 <= 0 ? 0 : ht_1.y / _e_1, Ln = mn + Ci * Oi;
                        ht_1.track = { x: y + Ye, y: S + mn, w: fe, h: Re }, ht_1.thumb = { x: y + Ye, y: S + Ln, w: fe, h: fn }, ce.rect(Ye, mn, fe, Re), ce.fill({ color: 0, alpha: .06 }), ce.rect(Ye, Ln, fe, fn), ce.fill({ color: 0, alpha: .25 }), ce.on("pointerdown", function (Zt) { var Wn, Hn, $n, Un, Xn, Yn; if ((Zt == null ? void 0 : Zt.button) === 2)
                            return; var pn = Ft(Zt); if (pn <= 0)
                            return; var Ke = (Hn = (Wn = Zt.global) == null ? void 0 : Wn.x) != null ? Hn : 0, pe = (Un = ($n = Zt.global) == null ? void 0 : $n.y) != null ? Un : 0; if (!(Ke >= ht_1.track.x && Ke <= ht_1.track.x + ht_1.track.w && pe >= ht_1.track.y && pe <= ht_1.track.y + ht_1.track.h))
                            return; if (Ke >= ht_1.thumb.x && Ke <= ht_1.thumb.x + ht_1.thumb.w && pe >= ht_1.thumb.y && pe <= ht_1.thumb.y + ht_1.thumb.h) {
                            ht_1.draggingPointerId = pn, ht_1.dragOffsetY = pe - ht_1.thumb.y, u.iframeScroll.set(Xe_1, ht_1), (Xn = Zt.stopPropagation) == null || Xn.call(Zt);
                            return;
                        } var Fn = Math.max(1, ht_1.track.h - ht_1.thumb.h), Bn = Math.max(ht_1.track.y, Math.min(ht_1.track.y + Fn, pe - ht_1.thumb.h / 2)), Ri = (Bn - ht_1.track.y) / Fn; ht_1.y = Math.max(0, Math.min(_e_1, Ri * _e_1)), ht_1.draggingPointerId = pn, ht_1.dragOffsetY = pe - Bn, u.iframeScroll.set(Xe_1, ht_1), ot == null || ot(), (Yn = Zt.stopPropagation) == null || Yn.call(Zt); });
                    }
                    else
                        ht_1.track = { x: 0, y: 0, w: fe, h: 0 }, ht_1.thumb = { x: 0, y: 0, w: fe, h: 0 };
                    u.iframeScroll.set(Xe_1, ht_1);
                }
                var ct = [], pt = h.tagName === "dialog" || h.tagName === "iframe" && !se ? ct : tt, Tt = Y;
                if (h.tagName === "dialog")
                    Tt = { x: 0, y: 0, w: Math.max(0, ut), h: Math.max(0, lt) };
                else if (h.tagName === "iframe" && !se) {
                    var J = (yt = h.key) != null ? yt : "", bt = u.iframeScroll.get(J), ee = bt ? bt.y : 0, Ot = 34;
                    Tt = { x: 0, y: Ot + ee, w: Math.max(0, ut), h: Math.max(0, lt - Ot) };
                }
                var At = (Rt = Bt != null ? Bt : Nt) != null ? Rt : C, Lt = y + ((Gt = Nt == null ? void 0 : Nt.position.x) != null ? Gt : 0), Kt = S + ((at = Nt == null ? void 0 : Nt.position.y) != null ? at : 0) + ((Mt = Bt == null ? void 0 : Bt.position.y) != null ? Mt : 0);
                L("render:draw:".concat(j, ":children"));
                var te = 0;
                for (var J = 0; J < ((Dt = h.children) != null ? Dt : []).length; J++) {
                    var bt = ((Ct = h.children) != null ? Ct : [])[J];
                    if (bt.kind === "block" && bt.tagName === "dialog")
                        pt.push(bt);
                    else {
                        if (h.tagName === "button" && bt.kind === "text")
                            continue;
                        $(bt, At, mt, Lt, Kt, pt, Tt, "".concat(j, ".").concat(J), te++);
                    }
                }
                if ((h.tagName === "dialog" || h.tagName === "iframe" && !se) && ct.length > 0) {
                    ct.sort(function (J, bt) { var ne, Jt; var ee = J.key && (ne = u.dialogZ.get(J.key)) != null ? ne : 0, Ot = bt.key && (Jt = u.dialogZ.get(bt.key)) != null ? Jt : 0; return ee - Ot; });
                    try {
                        for (var ct_1 = __values(ct), ct_1_1 = ct_1.next(); !ct_1_1.done; ct_1_1 = ct_1.next()) {
                            var J = ct_1_1.value;
                            var bt = J.key && J.key.length > 0 ? J.key : "".concat(j, ".dlg.").concat(te);
                            $(J, At, mt, Lt, Kt, ct, Tt, "".concat(j, ".dlg.").concat(bt), te++);
                        }
                    }
                    catch (e_58_1) { e_58 = { error: e_58_1 }; }
                    finally {
                        try {
                            if (ct_1_1 && !ct_1_1.done && (_a = ct_1.return)) _a.call(ct_1);
                        }
                        finally { if (e_58) throw e_58.error; }
                    }
                }
            }
            else {
                L("render:draw:".concat(j, ":text:begin"));
                var mt = Pt(O, "__text", function (ft) { ft.style = { fontFamily: r.fontFamily, fontSize: r.fontSize, fill: r.text, fontWeight: F.bold ? "700" : "400", wordWrap: !0, wordWrapWidth: 0 }; });
                mt.text = (vt = h.text) != null ? vt : "", mt.style.fontFamily = r.fontFamily, mt.style.fontSize = r.fontSize, mt.style.fill = r.text, mt.style.fontWeight = F.bold ? "700" : "400", mt.style.wordWrap = !0, mt.style.wordWrapWidth = Math.max(0, Math.ceil(h.width) + Me), oe(mt, 0, xt), L("render:draw:".concat(j, ":text:done"));
            }
        }
        L("render:root-loop");
        var A = { bold: !1 }, H = { x: 0, y: 0, w: t.renderer.width, h: t.renderer.height }, N = [], k = s.position, E = k && Number(k.y || 0) || 0, W = 0;
        for (var h = 0; h < e.children.length; h++) {
            L("render:root-loop:".concat(h));
            var P = e.children[h];
            P && (P.kind === "block" && P.tagName === "dialog" ? N.push(P) : (L("render:root-loop:".concat(h, ":dispatch")), $(P, s, A, 0, E, N, H, "root.".concat(h), W++)));
        }
        if (L("render:root-dialogs"), N.length > 0) {
            N.sort(function (P, F) { var tt, Y; var q = P.key && (tt = u.dialogZ.get(P.key)) != null ? tt : 0, Z = F.key && (Y = u.dialogZ.get(F.key)) != null ? Y : 0; return q - Z; });
            var h = 0;
            try {
                for (var N_1 = __values(N), N_1_1 = N_1.next(); !N_1_1.done; N_1_1 = N_1.next()) {
                    var P = N_1_1.value;
                    var F = P.key && P.key.length > 0 ? P.key : "rootdlg.".concat(h);
                    $(P, l, A, 0, 0, N, H, "dlg.".concat(F), h++);
                }
            }
            catch (e_50_1) { e_50 = { error: e_50_1 }; }
            finally {
                try {
                    if (N_1_1 && !N_1_1.done && (_b = N_1.return)) _b.call(N_1);
                }
                finally { if (e_50) throw e_50.error; }
            }
        }
        if (L("render:temporal-popups"), p.length > 0 && ri({ popups: p, stage: a, theme: r, viewportW: t.renderer.width, viewportH: t.renderer.height, temporalStates: u.temporals, getOrInitInputValue: function (h, P) { return un(h, P); }, sliders: u.sliders, sliderBounds: u.sliderBounds, sliderDrags: u.sliderDrags, selects: u.selects, selectPopups: d, uiFocus: u, getPointerId: Ft, getCursorColor: jt, requestPaint: ot }), L("render:select-popups"), d.length > 0)
            try {
                for (var d_2 = __values(d), d_2_1 = d_2.next(); !d_2_1.done; d_2_1 = d_2.next()) {
                    var h = d_2_1.value;
                    qr({ popup: h, stage: a, theme: r, selectStates: u.selects, uiState: u, getPointerId: Ft, requestPaint: ot, viewportW: t.renderer.width, viewportH: t.renderer.height });
                }
            }
            catch (e_51_1) { e_51 = { error: e_51_1 }; }
            finally {
                try {
                    if (d_2_1 && !d_2_1.done && (_c = d_2.return)) _c.call(d_2);
                }
                finally { if (e_51) throw e_51.error; }
            }
        L("render:context-menus");
        var _loop_3 = function (h, P) {
            if (!(P != null && P.open))
                return "continue";
            var F = new _t;
            F.eventMode = "static", F.cursor = "default", oe(F, P.x, P.y);
            var q = 140, Z = 28, tt = 6, Y = ["Copy", "Paste", "Close"], j = new wt;
            j.rect(0, 0, q + tt * 2, Y.length * Z + tt * 2), j.fill(16777215);
            var v = 1;
            j.rect(v, v, q + tt * 2 - v * 2, Y.length * Z + tt * 2 - v * 2), j.stroke({ width: 2, color: jt(h), alignment: 0 }), F.addChild(j), Y.forEach(function (f, I) { var O = tt + I * Z, C = new _t; C.eventMode = "static", C.cursor = "pointer", oe(C, tt, O); var y = new wt; y.rect(0, 0, q, Z), y.fill(16777215), C.addChild(y); var S = zt({ text: f, fontFamily: r.fontFamily, fontSize: r.fontSize, fill: r.text }); oe(S, 8, Math.max(0, (Z - S.height) / 2) + xt), C.addChild(S); var D = function (R) { return Ft(R) === h; }; C.on("pointerover", function (R) { D(R) && (y.clear(), y.rect(0, 0, q, Z), y.fill(15921906)); }), C.on("pointerout", function (R) { D(R) && (y.clear(), y.rect(0, 0, q, Z), y.fill(16777215)); }), C.on("pointerdown", function (R) { var V, nt, Q, it, st, dt, yt, Rt, Gt, at, Mt; if (!D(R))
                return; (V = R.stopPropagation) == null || V.call(R); var B = (nt = u.focusedKeyByPointer.get(h)) != null ? nt : null, X = B ? u.inputs.get(B) : null, z = B != null && u.fieldBounds.has(B) && X != null && typeof X.value == "string"; if (f === "Copy" && z) {
                var Dt = X, Ct = (Q = Dt.value) != null ? Q : "", vt = (st = (it = Dt.selections) == null ? void 0 : it.get(h)) != null ? st : null, mt = vt ? Math.max(0, Math.min(Ct.length, (dt = vt.start) != null ? dt : 0)) : 0, ft = vt ? Math.max(0, Math.min(Ct.length, (yt = vt.end) != null ? yt : mt)) : mt, ut = Math.min(mt, ft), lt = Math.max(mt, ft), Xt = ut !== lt ? Ct.slice(ut, lt) : Ct;
                u.clipboards.set(h, Xt);
            }
            else if (f === "Paste" && z) {
                var Dt = (Rt = u.clipboards.get(h)) != null ? Rt : "";
                if (Dt.length > 0) {
                    var Ct = X, vt = (Gt = Ct.value) != null ? Gt : "";
                    if (Ct.selections || (Ct.selections = new Map), !Ct.selections.has(h)) {
                        var Bt = vt.length;
                        Ct.selections.set(h, { start: Bt, end: Bt });
                    }
                    var mt = Ct.selections.get(h), ft = Math.max(0, Math.min(vt.length, (at = mt.start) != null ? at : vt.length)), ut = Math.max(0, Math.min(vt.length, (Mt = mt.end) != null ? Mt : ft)), lt = Math.min(ft, ut), Xt = Math.max(ft, ut);
                    Ct.value = vt.slice(0, lt) + Dt + vt.slice(Xt);
                    var Nt = lt + Dt.length;
                    mt.start = Nt, mt.end = Nt;
                }
            } var K = u.contextMenus.get(h); K && (K.open = !1, u.contextMenus.set(h, K)), ot == null || ot(); }), F.addChild(C); }), a.addChild(F);
        };
        try {
            for (var _j = __values(u.contextMenus.entries()), _k = _j.next(); !_k.done; _k = _j.next()) {
                var _l = __read(_k.value, 2), h = _l[0], P = _l[1];
                _loop_3(h, P);
            }
        }
        catch (e_52_1) { e_52 = { error: e_52_1 }; }
        finally {
            try {
                if (_k && !_k.done && (_d = _j.return)) _d.call(_j);
            }
            finally { if (e_52) throw e_52.error; }
        }
        L("render:prune-cache");
        try {
            for (var _m = __values(x.entries()), _p = _m.next(); !_p.done; _p = _m.next()) {
                var _q = __read(_p.value, 2), h = _q[0], P = _q[1];
                if (!c.has(h)) {
                    try {
                        P.removeFromParent(), (G = P.destroy) == null || G.call(P, { children: !0 });
                    }
                    catch (F) { }
                    x.delete(h);
                }
            }
        }
        catch (e_53_1) { e_53 = { error: e_53_1 }; }
        finally {
            try {
                if (_p && !_p.done && (_f = _m.return)) _f.call(_m);
            }
            finally { if (e_53) throw e_53.error; }
        }
        L("render:done");
    }
    function ss() {
        return ze(this, null, function () {
            var t, e, n, r, _a, i_1, _b, o_3, s, l, a_2, d, p_1, m_1, x_1, c_1, g_1, b_1, _2, w, _c, M, $, A_1, H_1, N_2, k_1, E_1, W_1, U_1, G_1, h_2, P_2, F_3, q_1, Z, tt, Y_1, j_1, f, I, O, v_4, n_3, r;
            return __generator(this, function (_d) {
                switch (_d.label) {
                    case 0:
                        _d.trys.push([0, 9, , 10]);
                        kt("main:start");
                        n = (t = document.getElementById("app")) != null ? t : document.body, r = !0;
                        kt("main:yoga");
                        if (!r) return [3 /*break*/, 1];
                        _a = Qo();
                        return [3 /*break*/, 3];
                    case 1: return [4 /*yield*/, Promise.resolve().then(function () { return (fi(), mi); })];
                    case 2:
                        _a = (_d.sent()).default;
                        _d.label = 3;
                    case 3:
                        rt = _a, kt("main:create-app");
                        i_1 = r ? Zo() : new Je;
                        _b = r;
                        if (_b) return [3 /*break*/, 5];
                        return [4 /*yield*/, i_1.init({ background: "#ffffff", resizeTo: window, antialias: !1, preference: "webgl" })];
                    case 4:
                        _b = (_d.sent());
                        _d.label = 5;
                    case 5:
                        _b, kt("main:attach-capture"), hi(i_1), window.__TRUEOS_PIXI_APP = i_1, kt("main:append-canvas"), n.appendChild(i_1.canvas), i_1.ticker.stop(), kt("main:capture-flags"), r && (u.harness.enabled = !1, u.virtualCursor.enabled = !1, window.__pixiCapture && (window.__pixiCapture.persist = !1)), kt("main:canvas-listeners"), i_1.canvas.addEventListener("contextmenu", function (f) { return f.preventDefault(); }), i_1.canvas.addEventListener("wheel", function (f) { var S, D; var I = (S = f.offsetX) != null ? S : 0, O = (D = f.offsetY) != null ? D : 0, C = null; for (var R = u.iframeRects.length - 1; R >= 0; R--) {
                            var B = u.iframeRects[R];
                            if (I >= B.x && I <= B.x + B.w && O >= B.y && O <= B.y + B.h) {
                                C = B.key;
                                break;
                            }
                        } if (C) {
                            var R = u.iframeScroll.get(C);
                            if (R) {
                                var B = Math.max(0, R.contentHeight - R.viewportHeight);
                                B > 0 && (R.y = Math.max(0, Math.min(B, R.y + f.deltaY)), u.iframeScroll.set(C, R), ot == null || ot(), f.preventDefault());
                                return;
                            }
                        } var y = Math.max(0, u.scroll.contentHeight - u.scroll.viewportHeight); y <= 0 || (u.scroll.y = Math.max(0, Math.min(y, u.scroll.y + f.deltaY)), ot == null || ot(), f.preventDefault()); }, { passive: !1 }), kt("main:stage:eventMode"), i_1.stage.eventMode = "static", kt("main:stage:hitArea"), i_1.stage.hitArea = i_1.screen, kt("main:stage:on:pointerdown"), i_1.stage.on("pointerdown", function (f) {
                            var e_59, _a;
                            var I, O, C, y, S, D;
                            if ((f == null ? void 0 : f.button) === 2) {
                                var R = Ft(f);
                                if (R > 0) {
                                    var B = (I = u.contextMenus.get(R)) != null ? I : { open: !1, x: 0, y: 0 };
                                    B.open = !0, B.x = (C = (O = f.global) == null ? void 0 : O.x) != null ? C : 0, B.y = (S = (y = f.global) == null ? void 0 : y.y) != null ? S : 0, u.contextMenus.set(R, B);
                                }
                                ot == null || ot(), (D = f.preventDefault) == null || D.call(f);
                                return;
                            }
                            if ((f == null ? void 0 : f.button) !== 2) {
                                var R = Ft(f), B = R > 0 ? u.contextMenus.get(R) : null;
                                B && B.open && (B.open = !1, u.contextMenus.set(R, B), ot == null || ot());
                            }
                            if ((f == null ? void 0 : f.button) !== 2) {
                                var R = !1;
                                try {
                                    for (var _b = __values(u.selects.values()), _c = _b.next(); !_c.done; _c = _b.next()) {
                                        var B = _c.value;
                                        B.open && (B.open = !1, R = !0);
                                    }
                                }
                                catch (e_59_1) { e_59 = { error: e_59_1 }; }
                                finally {
                                    try {
                                        if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
                                    }
                                    finally { if (e_59) throw e_59.error; }
                                }
                                R && (ot == null || ot());
                            }
                            (f == null ? void 0 : f.button) !== 2 && ii(u.temporals) && (ot == null || ot()), q_1();
                        }), kt("main:stage:done"), kt("main:roots");
                        o_3 = new _t, s = new _t;
                        s.eventMode = "static";
                        l = new _t;
                        l.eventMode = "none", i_1.stage.addChild(o_3), i_1.stage.addChild(s), i_1.stage.addChild(l);
                        a_2 = new wt;
                        a_2.label = "__trueosGlobalScrollbar", a_2.eventMode = "static", s.addChild(a_2);
                        d = function (f, I) { f.clear(); var O = I.half, C = I.strokeWidth, y = I.color; f.moveTo(-O, 0), f.lineTo(O, 0), f.stroke({ width: C, color: y }), f.moveTo(0, -O), f.lineTo(0, O), f.stroke({ width: C, color: y }); }, p_1 = new wt;
                        p_1.eventMode = "none", p_1.visible = !1, l.addChild(p_1);
                        m_1 = new wt;
                        m_1.eventMode = "none", m_1.visible = !1, l.addChild(m_1);
                        x_1 = new wt;
                        x_1.eventMode = "none", x_1.visible = !1, l.addChild(x_1);
                        c_1 = new wt;
                        c_1.eventMode = "none", l.addChild(c_1), kt("main:text-measure");
                        g_1 = document.createElement("canvas").getContext("2d");
                        if (!g_1)
                            throw new Error("2D canvas not available");
                        g_1.font = "".concat(be.fontSize, "px ").concat(be.fontFamily);
                        b_1 = function (f) { return g_1.measureText(f).width; }, _2 = be.fontSize * 1.25;
                        kt("main:html");
                        if (!(typeof window.__TRUEOS_INPUT_HTML__ == "string")) return [3 /*break*/, 6];
                        _c = window.__TRUEOS_INPUT_HTML__;
                        return [3 /*break*/, 8];
                    case 6: return [4 /*yield*/, fetch("/input.html").then(function (f) { return f.text(); })];
                    case 7:
                        _c = _d.sent();
                        _d.label = 8;
                    case 8:
                        w = _c;
                        Vt() && console.log("[trueos pixi widgets] input-html chars=".concat(w.length, " sample=\"").concat(Ti(w), "\"")), kt("main:render-tree"), xi.clear();
                        M = Array.isArray(window.__TRUEOS_WIDGET_TEXT_ROWS__) ? Oe(window.__TRUEOS_WIDGET_TEXT_ROWS__) : [], $ = M.length > 0 ? M : Mi(w), A_1 = ns(window.__TRUEOS_WIDGET_RENDER_TREE__, $);
                        if (Vt() && (console.log("[trueos pixi widgets] text-fallback source=".concat(M.length > 0 ? "truesurfer" : "html", " rows=").concat($.length, " samples=").concat(Uo($))), console.log("[trueos pixi widgets] render-tree source=truesurfer nodes=".concat(A_1.length))), A_1.length === 0)
                            throw new Error("TrueSurfer widget render tree is missing");
                        H_1 = Bo(A_1), N_2 = null, k_1 = { nodes: 0, blocks: 0, text: 0, maxDepth: 0, tags: {} }, E_1 = 0, W_1 = function () { var f = Math.max(0, u.scroll.contentHeight - u.scroll.viewportHeight); u.scroll.y = Math.max(0, Math.min(u.scroll.y, f)); }, U_1 = function () { var f = i_1.renderer.width, I = i_1.renderer.height; u.scroll.viewportHeight = I; var O = u.scroll.contentHeight, C = Math.max(0, O - I), y = C > .5; if (a_2.clear(), a_2.visible = y, !y) {
                            u.scroll.track = { x: 0, y: 0, w: u.scroll.track.w, h: 0 }, u.scroll.thumb = { x: 0, y: 0, w: u.scroll.thumb.w, h: 0 };
                            return;
                        } var S = Cn, D = Ie, R = Math.max(0, f - D - S), B = S, X = Math.max(0, I - S * 2), K = Math.max(24, I / Math.max(I, O) * X), V = Math.max(1, X - K), nt = C <= 0 ? 0 : u.scroll.y / C, Q = B + V * nt; u.scroll.track = { x: R, y: B, w: D, h: X }, u.scroll.thumb = { x: R, y: Q, w: D, h: K }, a_2.rect(R, B, D, X), a_2.fill({ color: 0, alpha: .06 }), a_2.rect(R, Q, D, K), a_2.fill({ color: 0, alpha: .25 }); }, G_1 = function () { if (N_2) {
                            if (kt("main:paint:clamp"), W_1(), kt("main:paint:render-to-pixi"), os(i_1, N_2, o_3), kt("main:paint:scrollbar"), U_1(), kt("main:paint:renderer-render"), i_1.renderer.render(i_1.stage), Jo(H_1, k_1, Ho(A_1), $o(N_2)), Vt()) {
                                var f = zo(N_2);
                                window.__TRUEOS_PIXI_LAYOUT_TEXT_OVERLAYS__ = f, E_1 < 4 && (E_1 += 1, console.log("[trueos pixi widgets] layout-text-overlays count=".concat(f.length, " samples=").concat(jo(f))));
                            }
                            kt("main:paint:done");
                        } };
                        Vt() && (window.__TRUEOS_REPAINT_NOW__ = function () { window.__TRUEOS_PIXI_DIRTY__ = !1, G_1(); });
                        h_2 = function () { kt("main:layout-build"); var f = is(A_1, window.innerWidth, window.innerHeight); kt("main:layout-commit"), N_2 = f, Vt() && (window.__TRUEOS_PIXI_LAST_LAYOUT__ = f, window.__TRUEOS_PIXI_LAYOUT_TEXT_OVERLAYS__ = []), k_1 = Wo(f), u.scroll.contentHeight = qo(f), u.scroll.viewportHeight = window.innerHeight, G_1(); };
                        dn = function () { h_2(); };
                        P_2 = !1, F_3 = !1, q_1 = function () { if (Vt()) {
                            window.__TRUEOS_PIXI_DIRTY__ = !0;
                            return;
                        } F_3 || P_2 || (F_3 = !0, requestAnimationFrame(function () { F_3 = !1, i_1.renderer.render(i_1.stage); })); };
                        ot = function () { if (!P_2) {
                            if (Vt()) {
                                window.__TRUEOS_PIXI_DIRTY__ = !0;
                                return;
                            }
                            P_2 = !0, requestAnimationFrame(function () { P_2 = !1, G_1(); });
                        } }, kt("main:first-rerender"), h_2(), kt("main:cursor-setup");
                        Z = 2, tt = 10, Y_1 = Vt();
                        d(p_1, { half: tt, strokeWidth: Z, color: jt(Wt) }), d(m_1, { half: tt, strokeWidth: Z, color: jt(Ht) }), d(x_1, { half: tt, strokeWidth: Z, color: jt(Ut) });
                        j_1 = 2;
                        if (d(c_1, { half: tt, strokeWidth: Z, color: jt(j_1) }), u.userCursorPos.set(Wt, { x: i_1.renderer.width * .25, y: i_1.renderer.height * .5 }), u.userCursorPos.set(Ht, { x: i_1.renderer.width * .25 + 40, y: i_1.renderer.height * .5 + 20 }), u.userCursorPos.set(Ut, { x: i_1.renderer.width * .25 + 80, y: i_1.renderer.height * .5 + 40 }), p_1.visible = !Y_1, m_1.visible = !Y_1, x_1.visible = !Y_1, !Y_1) {
                            f = u.userCursorPos.get(Wt), I = u.userCursorPos.get(Ht), O = u.userCursorPos.get(Ut);
                            p_1.position.set(f.x, f.y), m_1.position.set(I.x, I.y), x_1.position.set(O.x, O.y);
                        }
                        c_1.visible = !Y_1 && u.virtualCursor.enabled;
                        v_4 = function () { if (Y_1) {
                            p_1.visible = !1, m_1.visible = !1, x_1.visible = !1, c_1.visible = !1;
                            return;
                        } var f = u.userCursorPos.get(Wt), I = u.userCursorPos.get(Ht), O = u.userCursorPos.get(Ut); f && (p_1.visible = !0, p_1.position.set(f.x, f.y)), I && (m_1.visible = !0, m_1.position.set(I.x, I.y)), O && (x_1.visible = !0, x_1.position.set(O.x, O.y)); var C = function (y, S) { var D = null, R = null; for (var B = u.hoverRects.length - 1; B >= 0; B--) {
                            var X = u.hoverRects[B];
                            if (y >= X.x && y <= X.x + X.w && S >= X.y && S <= X.y + X.h) {
                                D = X.key, R = X.cursor;
                                break;
                            }
                        } return { hitKey: D, hitCursor: R }; }; if (f) {
                            var _a = C(f.x, f.y), y = _a.hitKey, S = _a.hitCursor;
                            u.hoveredKeyByPointer.set(Wt, y), u.hoveredCursorByPointer.set(Wt, S);
                            var D = u.textDrags.has(Wt) || u.sliderDrags.has(Wt) || u.dialogDrags.has(Wt);
                            p_1.rotation = S != null || D ? Math.PI / 4 : 0;
                        } if (I) {
                            var _b = C(I.x, I.y), y = _b.hitKey, S = _b.hitCursor;
                            u.hoveredKeyByPointer.set(Ht, y), u.hoveredCursorByPointer.set(Ht, S);
                            var D = u.textDrags.has(Ht) || u.sliderDrags.has(Ht) || u.dialogDrags.has(Ht);
                            m_1.rotation = S != null || D ? Math.PI / 4 : 0;
                        } if (O) {
                            var _c = C(O.x, O.y), y = _c.hitKey, S = _c.hitCursor;
                            u.hoveredKeyByPointer.set(Ut, y), u.hoveredCursorByPointer.set(Ut, S);
                            var D = u.textDrags.has(Ut) || u.sliderDrags.has(Ut) || u.dialogDrags.has(Ut);
                            x_1.rotation = S != null || D ? Math.PI / 4 : 0;
                        } q_1(); };
                        u.harness.enabled && setInterval(function () {
                            var e_60, _a, e_61, _b;
                            var f = u.harness.activeUserPointerId, I = f === Wt ? Ht : f === Ht ? Ut : Wt;
                            if (u.harness.activeUserPointerId = I, u.lastMouse.has) {
                                var X = u.userCursorPos.get(f), z = u.userCursorPos.get(I);
                                u.userCursorPos.set(I, { x: u.lastMouse.x, y: u.lastMouse.y }), z ? u.userCursorPos.set(f, { x: z.x, y: z.y }) : X && u.userCursorPos.set(f, { x: X.x, y: X.y });
                            }
                            var O = u.textDrags.size > 0, C = u.sliderDrags.size > 0, y = u.dialogDrags.size > 0, S = u.scroll.draggingPointerId != null, D = u.color.draggingPointerId != null, R = !1;
                            try {
                                for (var _c = __values(u.iframeScroll.values()), _d = _c.next(); !_d.done; _d = _c.next()) {
                                    var X = _d.value;
                                    if (X.draggingPointerId != null) {
                                        R = !0;
                                        break;
                                    }
                                }
                            }
                            catch (e_60_1) { e_60 = { error: e_60_1 }; }
                            finally {
                                try {
                                    if (_d && !_d.done && (_a = _c.return)) _a.call(_c);
                                }
                                finally { if (e_60) throw e_60.error; }
                            }
                            var B = O || C || y || S || D || R;
                            u.textDrags.delete(Wt), u.textDrags.delete(Ht), u.textDrags.delete(Ut), u.sliderDrags.delete(Wt), u.sliderDrags.delete(Ht), u.sliderDrags.delete(Ut), u.dialogDrags.delete(Wt), u.dialogDrags.delete(Ht), u.dialogDrags.delete(Ut);
                            try {
                                for (var _f = __values([Wt, Ht, Ut]), _g = _f.next(); !_g.done; _g = _f.next()) {
                                    var X = _g.value;
                                    var z = u.numberHolds.get(X);
                                    z && (z.timeoutId != null && window.clearTimeout(z.timeoutId), z.intervalId != null && window.clearInterval(z.intervalId), u.numberHolds.delete(X));
                                }
                            }
                            catch (e_61_1) { e_61 = { error: e_61_1 }; }
                            finally {
                                try {
                                    if (_g && !_g.done && (_b = _f.return)) _b.call(_f);
                                }
                                finally { if (e_61) throw e_61.error; }
                            }
                            (u.scroll.draggingPointerId === Wt || u.scroll.draggingPointerId === Ht || u.scroll.draggingPointerId === Ut) && (u.scroll.draggingPointerId = null), (u.color.draggingPointerId === Wt || u.color.draggingPointerId === Ht || u.color.draggingPointerId === Ut) && (u.color.draggingPointerId = null), v_4(), B && (ot == null || ot());
                        }, u.harness.periodMs), !Y_1 && u.virtualCursor.enabled && i_1.ticker.add(function () { var S, D, R, B, X; var f = Math.max(0, i_1.ticker.deltaMS) / 1e3; c_1.visible = !0, u.virtualCursor.t += f; var I = i_1.renderer.width * .75, O = i_1.renderer.height * .25, C = u.virtualCursor.t * u.virtualCursor.speed, y = u.virtualCursor.radius; u.virtualCursor.x = I + Math.cos(C) * y, u.virtualCursor.y = O + Math.sin(C) * y, c_1.position.set(u.virtualCursor.x, u.virtualCursor.y); {
                            var z = j_1, K = u.virtualCursor.x, V = u.virtualCursor.y, nt = null, Q = null;
                            for (var dt = u.hoverRects.length - 1; dt >= 0; dt--) {
                                var yt = u.hoverRects[dt];
                                if (K >= yt.x && K <= yt.x + yt.w && V >= yt.y && V <= yt.y + yt.h) {
                                    nt = yt.key, Q = yt.cursor;
                                    break;
                                }
                            }
                            var it = (S = u.hoveredKeyByPointer.get(z)) != null ? S : null;
                            it !== nt && (it && ((R = (D = u.hoverHandlers.get(it)) == null ? void 0 : D.out) == null || R.call(D)), nt && ((X = (B = u.hoverHandlers.get(nt)) == null ? void 0 : B.over) == null || X.call(B)), u.hoveredKeyByPointer.set(z, nt)), u.hoveredCursorByPointer.set(z, Q);
                            var st = u.textDrags.has(z) || u.sliderDrags.has(z) || u.dialogDrags.has(z);
                            c_1.rotation = Q != null || st ? Math.PI / 4 : 0;
                        } }), u.virtualCursor.x = i_1.renderer.width * .75 + u.virtualCursor.radius, u.virtualCursor.y = i_1.renderer.height * .25, c_1.position.set(u.virtualCursor.x, u.virtualCursor.y), Vt() && G_1(), i_1.stage.on("pointerup", function (f) {
                            var e_62, _a;
                            var C, y, S;
                            var I = Ft(f), O = (y = (C = u.sliderDrags.get(I)) == null ? void 0 : C.key) != null ? y : null;
                            u.textDrags.delete(I), u.sliderDrags.delete(I), u.dialogDrags.delete(I), u.scroll.draggingPointerId === I && (u.scroll.draggingPointerId = null), u.color.draggingPointerId === I && (u.color.draggingPointerId = null);
                            try {
                                for (var _b = __values(u.iframeScroll.values()), _c = _b.next(); !_c.done; _c = _b.next()) {
                                    var D = _c.value;
                                    D.draggingPointerId === I && (D.draggingPointerId = null);
                                }
                            }
                            catch (e_62_1) { e_62 = { error: e_62_1 }; }
                            finally {
                                try {
                                    if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
                                }
                                finally { if (e_62) throw e_62.error; }
                            }
                            {
                                var D = u.numberHolds.get(I);
                                D && (D.timeoutId != null && window.clearTimeout(D.timeoutId), D.intervalId != null && window.clearInterval(D.intervalId), u.numberHolds.delete(I));
                            }
                            if (O) {
                                var D = (S = u.temporalYearOwners.get(O)) != null ? S : null;
                                if (D) {
                                    var R = u.temporals.get(D);
                                    R && R.openYear && (R.openYear = !1, u.temporals.set(D, R), ot == null || ot());
                                }
                            }
                            q_1();
                        }), i_1.stage.on("pointerupoutside", function (f) {
                            var e_63, _a;
                            var C, y, S;
                            var I = Ft(f), O = (y = (C = u.sliderDrags.get(I)) == null ? void 0 : C.key) != null ? y : null;
                            u.textDrags.delete(I), u.sliderDrags.delete(I), u.dialogDrags.delete(I), u.scroll.draggingPointerId === I && (u.scroll.draggingPointerId = null), u.color.draggingPointerId === I && (u.color.draggingPointerId = null);
                            try {
                                for (var _b = __values(u.iframeScroll.values()), _c = _b.next(); !_c.done; _c = _b.next()) {
                                    var D = _c.value;
                                    D.draggingPointerId === I && (D.draggingPointerId = null);
                                }
                            }
                            catch (e_63_1) { e_63 = { error: e_63_1 }; }
                            finally {
                                try {
                                    if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
                                }
                                finally { if (e_63) throw e_63.error; }
                            }
                            {
                                var D = u.numberHolds.get(I);
                                D && (D.timeoutId != null && window.clearTimeout(D.timeoutId), D.intervalId != null && window.clearInterval(D.intervalId), u.numberHolds.delete(I));
                            }
                            if (O) {
                                var D = (S = u.temporalYearOwners.get(O)) != null ? S : null;
                                if (D) {
                                    var R = u.temporals.get(D);
                                    R && R.openYear && (R.openYear = !1, u.temporals.set(D, R), ot == null || ot());
                                }
                            }
                            q_1();
                        }), a_2.on("pointerdown", function (f) { var V, nt, Q, it, st, dt; if ((f == null ? void 0 : f.button) === 2)
                            return; var I = Ft(f); if (I <= 0)
                            return; var O = (nt = (V = f.global) == null ? void 0 : V.x) != null ? nt : 0, C = (it = (Q = f.global) == null ? void 0 : Q.y) != null ? it : 0, y = u.scroll.track, S = u.scroll.thumb; if (!(O >= y.x && O <= y.x + y.w && C >= y.y && C <= y.y + y.h))
                            return; var R = Math.max(0, u.scroll.contentHeight - u.scroll.viewportHeight); if (R <= .5)
                            return; if (O >= S.x && O <= S.x + S.w && C >= S.y && C <= S.y + S.h) {
                            u.scroll.draggingPointerId = I, u.scroll.dragOffsetY = C - S.y, (st = f.stopPropagation) == null || st.call(f);
                            return;
                        } var X = Math.max(1, y.h - S.h), z = Math.max(y.y, Math.min(y.y + X, C - S.h / 2)), K = (z - y.y) / X; u.scroll.y = Math.max(0, Math.min(R, K * R)), u.scroll.draggingPointerId = I, u.scroll.dragOffsetY = C - z, ot == null || ot(), (dt = f.stopPropagation) == null || dt.call(f); }), i_1.stage.on("pointermove", function (f) {
                            var e_64, _a;
                            var D, R, B, X, z, K, V, nt, Q, it, st, dt, yt, Rt, Gt, at, Mt, Dt, Ct, vt, mt, ft, ut, lt, Xt, Nt, Bt, se;
                            var I = Number((B = (R = f == null ? void 0 : f.pointerId) != null ? R : (D = f == null ? void 0 : f.data) == null ? void 0 : D.pointerId) != null ? B : 1);
                            if (String((K = (z = f == null ? void 0 : f.pointerType) != null ? z : (X = f == null ? void 0 : f.data) == null ? void 0 : X.pointerType) != null ? K : "").toLowerCase() === "mouse" || I === 1) {
                                var ct = (nt = (V = f.global) == null ? void 0 : V.x) != null ? nt : 0, pt = (it = (Q = f.global) == null ? void 0 : Q.y) != null ? it : 0;
                                u.lastMouse.x = ct, u.lastMouse.y = pt, u.lastMouse.has = !0, u.primaryMousePointerId = I;
                                var Tt = u.harness.enabled ? u.harness.activeUserPointerId : I;
                                u.userCursorPos.set(Tt, { x: ct, y: pt }), v_4();
                            }
                            var y = Ft(f);
                            if (y <= 0)
                                return;
                            var S = !1;
                            {
                                var ct = u.textDrags.get(y);
                                if (ct) {
                                    var pt = ct.key, Tt = u.fieldBounds.get(pt), At = u.inputs.get(pt);
                                    if (Tt && At && typeof At.value == "string") {
                                        var Lt = Tt.isPassword ? "\u2022".repeat(At.value.length) : At.value, Kt = de(ue(Lt, Math.max(0, Tt.innerWidth), b_1), Tt.maxLines), te = ((dt = (st = f.global) == null ? void 0 : st.x) != null ? dt : 0) - Tt.x - Tt.innerLeft, J = ((Rt = (yt = f.global) == null ? void 0 : yt.y) != null ? Rt : 0) - Tt.y - Tt.innerTop, bt = Ee({ fullText: Lt, lines: Kt, localX: te, localY: J, lineHeight: _2, measure: b_1 });
                                        At.selections || (At.selections = new Map), At.selections.set(y, { start: ct.anchor, end: bt }), S = !0;
                                    }
                                }
                            }
                            {
                                var ct = u.sliderDrags.get(y);
                                if (ct) {
                                    var pt = ct.key, Tt = u.sliderBounds.get(pt);
                                    if (Tt) {
                                        var Lt = ((at = (Gt = f.global) == null ? void 0 : Gt.x) != null ? at : 0) - Tt.x, Kt = Math.max(1, Tt.w - Tt.innerPad * 2), te = (Lt - Tt.innerPad) / Kt, J = ye(u.sliders, pt, void 0);
                                        J.value = Math.max(0, Math.min(1, te)), S = !0;
                                    }
                                }
                            }
                            {
                                var ct = u.color.draggingPointerId;
                                if (ct != null && ct === y) {
                                    var pt = u.color.bounds;
                                    if (pt) {
                                        var Tt = (Dt = (Mt = f.global) == null ? void 0 : Mt.x) != null ? Dt : 0, At = (vt = (Ct = f.global) == null ? void 0 : Ct.y) != null ? vt : 0, Lt = Tt - pt.x, Kt = At - pt.y, te = Mn({ lx: Lt, ly: Kt, w: pt.w, h: pt.h });
                                        te && (u.color.rgb = te, u.color.pick = { x: Lt, y: Kt }, S = !0);
                                    }
                                }
                            }
                            {
                                var ct = u.scroll.draggingPointerId;
                                if (ct != null && ct === y) {
                                    var pt = u.scroll.track, Tt = u.scroll.thumb, At = Math.max(0, u.scroll.contentHeight - u.scroll.viewportHeight);
                                    if (At > .5 && pt.h > 0 && Tt.h > 0) {
                                        var Lt = (ft = (mt = f.global) == null ? void 0 : mt.y) != null ? ft : 0, Kt = Math.max(1, pt.h - Tt.h), J = (Math.max(pt.y, Math.min(pt.y + Kt, Lt - u.scroll.dragOffsetY)) - pt.y) / Kt;
                                        u.scroll.y = Math.max(0, Math.min(At, J * At)), S = !0;
                                    }
                                }
                            }
                            try {
                                for (var _b = __values(u.iframeScroll.values()), _c = _b.next(); !_c.done; _c = _b.next()) {
                                    var ct = _c.value;
                                    if (ct.draggingPointerId == null || ct.draggingPointerId !== y)
                                        continue;
                                    var pt = Math.max(0, ct.contentHeight - ct.viewportHeight);
                                    if (pt <= .5 || ct.track.h <= 0 || ct.thumb.h <= 0)
                                        continue;
                                    var Tt = (lt = (ut = f.global) == null ? void 0 : ut.y) != null ? lt : 0, At = Math.max(1, ct.track.h - ct.thumb.h), Kt = (Math.max(ct.track.y, Math.min(ct.track.y + At, Tt - ct.dragOffsetY)) - ct.track.y) / At;
                                    ct.y = Math.max(0, Math.min(pt, Kt * pt)), S = !0;
                                }
                            }
                            catch (e_64_1) { e_64 = { error: e_64_1 }; }
                            finally {
                                try {
                                    if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
                                }
                                finally { if (e_64) throw e_64.error; }
                            }
                            {
                                var ct = u.dialogDrags.get(y);
                                if (ct) {
                                    var pt = Qe(u.dialogs, ct.key), Tt = (Nt = (Xt = f.global) == null ? void 0 : Xt.x) != null ? Nt : 0, At = (se = (Bt = f.global) == null ? void 0 : Bt.y) != null ? se : 0;
                                    pt.x = ct.originX + (Tt - ct.startGX), pt.y = ct.originY + (At - ct.startGY);
                                    var Lt = u.dialogDragBounds.get(ct.key);
                                    Lt && (pt.x = Math.max(Lt.minX, Math.min(Lt.maxX, pt.x)), pt.y = Math.max(Lt.minY, Math.min(Lt.maxY, pt.y))), S = !0;
                                }
                            }
                            S && (ot == null || ot());
                        }), kt("main:input-listeners"), window.addEventListener("keydown", function (f) {
                            var Q, it, st, dt, yt, Rt, Gt;
                            var I = u.keyboardOwnerPointerId, O = (Q = u.focusedKeyByPointer.get(I)) != null ? Q : null;
                            if (!O)
                                return;
                            var C = u.inputs.get(O);
                            if (!C || typeof C.value != "string")
                                return;
                            if (C.selections || (C.selections = new Map), !C.selections.has(I)) {
                                var at = C.value.length;
                                C.selections.set(I, { start: at, end: at });
                            }
                            var y = C.selections.get(I), S = C.value.length, D = function (at) { return Math.max(0, Math.min(S, at)); }, R = D((it = y.start) != null ? it : S), B = D((st = y.end) != null ? st : R);
                            y.start = R, y.end = B;
                            var X = Math.min(R, B), z = Math.max(R, B), K = X !== z, V = function (at) { var Mt = Math.max(0, Math.min(C.value.length, at)); y.start = Mt, y.end = Mt; }, nt = function (at, Mt) { y.start = Math.max(0, Math.min(C.value.length, at)), y.end = Math.max(0, Math.min(C.value.length, Mt)); };
                            if (f.key.toLowerCase() === "a" && (f.ctrlKey || f.metaKey)) {
                                nt(0, C.value.length), f.preventDefault(), G_1();
                                return;
                            }
                            if (f.key === "ArrowLeft" || f.key === "ArrowRight") {
                                var at = f.key === "ArrowLeft" ? -1 : 1;
                                if (f.shiftKey) {
                                    var Mt = (dt = y.start) != null ? dt : S, Dt = ((yt = y.end) != null ? yt : Mt) + at;
                                    nt(Mt, Dt);
                                }
                                else
                                    V((K ? X : z) + at);
                                f.preventDefault(), h_2();
                                return;
                            }
                            if (f.key === "Home") {
                                f.shiftKey ? nt((Rt = y.start) != null ? Rt : S, 0) : V(0), f.preventDefault(), h_2();
                                return;
                            }
                            if (f.key === "End") {
                                f.shiftKey ? nt((Gt = y.start) != null ? Gt : 0, C.value.length) : V(C.value.length), f.preventDefault(), h_2();
                                return;
                            }
                            if (f.key === "Backspace") {
                                if (K)
                                    C.value = C.value.slice(0, X) + C.value.slice(z), V(X);
                                else {
                                    var at = z;
                                    at > 0 && (C.value = C.value.slice(0, at - 1) + C.value.slice(at), V(at - 1));
                                }
                                f.preventDefault(), h_2();
                                return;
                            }
                            if (f.key === "Enter") {
                                var at = "\n";
                                if (K)
                                    C.value = C.value.slice(0, X) + at + C.value.slice(z), V(X + at.length);
                                else {
                                    var Mt = z;
                                    C.value = C.value.slice(0, Mt) + at + C.value.slice(Mt), V(Mt + at.length);
                                }
                                f.preventDefault(), h_2();
                                return;
                            }
                            if (f.key === "Delete") {
                                if (K)
                                    C.value = C.value.slice(0, X) + C.value.slice(z), V(X);
                                else {
                                    var at = z;
                                    at < C.value.length && (C.value = C.value.slice(0, at) + C.value.slice(at + 1), V(at));
                                }
                                f.preventDefault(), h_2();
                                return;
                            }
                            if (f.key === "Escape") {
                                u.focusedKeyByPointer.set(I, null), h_2();
                                return;
                            }
                            if (f.key.length === 1 && !f.ctrlKey && !f.metaKey && !f.altKey) {
                                if (K)
                                    C.value = C.value.slice(0, X) + f.key + C.value.slice(z), V(X + 1);
                                else {
                                    var at = z;
                                    C.value = C.value.slice(0, at) + f.key + C.value.slice(at), V(at + 1);
                                }
                                f.preventDefault(), h_2();
                            }
                        }), window.addEventListener("resize", function () { h_2(), c_1.visible = u.virtualCursor.enabled; }), kt("main:done"), r && (window.__TRUEOS_PIXI_APP_READY__ = !0, window.__TRUEOS_PIXI_APP_ERROR__ = "", window.__TRUEOS_PIXI_APP_PHASE__ = "ready");
                        return [3 /*break*/, 10];
                    case 9:
                        n_3 = _d.sent();
                        window.__TRUEOS_PIXI_APP_READY__ = !1, window.__TRUEOS_PIXI_APP_ERROR__ = ki(n_3);
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
    ss().then(function () { window.__TRUEOS_PIXI_APP_ERROR__ || (window.__TRUEOS_PIXI_APP_READY__ = !0, window.__TRUEOS_PIXI_APP_ERROR__ = "", window.__TRUEOS_PIXI_APP_PHASE__ = "ready"); }).catch(function (t) { var n; window.__TRUEOS_PIXI_APP_READY__ = !1, window.__TRUEOS_PIXI_APP_ERROR__ = ki(t), console.error(t); var e = document.createElement("pre"); e.textContent = String((n = t == null ? void 0 : t.stack) != null ? n : t), document.body.appendChild(e); });
})();
