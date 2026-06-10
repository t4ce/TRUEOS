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
    var nr = Object.defineProperty, Yi = Object.defineProperties;
    var Ki = Object.getOwnPropertyDescriptors;
    var er = Object.getOwnPropertySymbols;
    var zi = Object.prototype.hasOwnProperty, ji = Object.prototype.propertyIsEnumerable;
    var kn = function (t, e, n) { return e in t ? nr(t, e, { enumerable: !0, configurable: !0, writable: !0, value: n }) : t[e] = n; }, te = function (t, e) {
        var e_1, _a;
        for (var n in e || (e = {}))
            zi.call(e, n) && kn(t, n, e[n]);
        if (er)
            try {
                for (var _b = __values(er(e)), _c = _b.next(); !_c.done; _c = _b.next()) {
                    var n = _c.value;
                    ji.call(e, n) && kn(t, n, e[n]);
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
    }, _e = function (t, e) { return Yi(t, Ki(e)); };
    var Vi = function (t, e) { return function () { return (t && (e = t(t = 0)), e); }; };
    var Ji = function (t, e) { for (var n in e)
        nr(t, n, { get: e[n], enumerable: !0 }); };
    var pt = function (t, e, n) { return kn(t, typeof e != "symbol" ? e + "" : e, n); };
    var Qe = function (t, e, n) { return new Promise(function (r, i) { var o = function (a) { try {
        u(n.next(a));
    }
    catch (h) {
        i(h);
    } }, s = function (a) { try {
        u(n.throw(a));
    }
    catch (h) {
        i(h);
    } }, u = function (a) { return a.done ? r(a.value) : Promise.resolve(a.value).then(o, s); }; u((n = n.apply(t, e)).next()); }); };
    var Ti = {};
    Ji(Ti, { default: function () { return Ko; } });
    var Ko, Ei = Vi(function () { Ko = {}; });
    var He = /** @class */ (function () {
        function He(e, n) {
            if (e === void 0) { e = 0; }
            if (n === void 0) { n = 0; }
            pt(this, "x");
            pt(this, "y");
            this.x = Number(e) || 0, this.y = Number(n) || 0;
        }
        He.prototype.set = function (e, n) {
            if (e === void 0) { e = 0; }
            if (n === void 0) { n = e; }
            this.x = Number(e) || 0, this.y = Number(n) || 0;
        };
        return He;
    }()), gt = /** @class */ (function () {
        function gt(e, n, r, i) {
            if (e === void 0) { e = 0; }
            if (n === void 0) { n = 0; }
            if (r === void 0) { r = 0; }
            if (i === void 0) { i = 0; }
            pt(this, "x");
            pt(this, "y");
            pt(this, "width");
            pt(this, "height");
            this.x = Number(e) || 0, this.y = Number(n) || 0, this.width = Number(r) || 0, this.height = Number(i) || 0;
        }
        return gt;
    }()), Rn = /** @class */ (function () {
        function Rn() {
            pt(this, "parent");
            pt(this, "children");
            pt(this, "label");
            pt(this, "name");
            pt(this, "position");
            pt(this, "scale");
            pt(this, "pivot");
            pt(this, "visible");
            pt(this, "alpha");
            pt(this, "mask");
            pt(this, "rotation");
            pt(this, "zIndex");
            pt(this, "eventMode");
            pt(this, "cursor");
            pt(this, "hitArea");
            pt(this, "listeners");
            this.parent = null, this.position = new He, this.scale = new He(1, 1), this.pivot = new He, this.visible = !0, this.alpha = 1, this.mask = null, this.rotation = 0, this.zIndex = 0, this.eventMode = null, this.cursor = null, this.hitArea = null, this.listeners = {};
        }
        Object.defineProperty(Rn.prototype, "x", {
            get: function () { return this.position.x; },
            set: function (e) { this.position.x = Number(e) || 0; },
            enumerable: false,
            configurable: true
        });
        Object.defineProperty(Rn.prototype, "y", {
            get: function () { return this.position.y; },
            set: function (e) { this.position.y = Number(e) || 0; },
            enumerable: false,
            configurable: true
        });
        Rn.prototype.on = function (e, n) { return this; };
        Rn.prototype.removeAllListeners = function (e) { return e == null ? this.listeners = {} : delete this.listeners[String(e)], this; };
        Rn.prototype.removeFromParent = function () { var e; return (e = this.parent) == null || e.removeChild(this), this; };
        Rn.prototype.destroy = function (e) { this.removeFromParent(), this.removeAllListeners(); };
        Rn.prototype.toLocal = function (e) { var n = e || {}; return { x: (Number(n.x) || 0) - this.getGlobalX(), y: (Number(n.y) || 0) - this.getGlobalY() }; };
        Rn.prototype.getGlobalPosition = function () { return { x: this.getGlobalX(), y: this.getGlobalY() }; };
        Rn.prototype.getGlobalX = function () { return (this.parent ? this.parent.getGlobalX() : 0) + this.x; };
        Rn.prototype.getGlobalY = function () { return (this.parent ? this.parent.getGlobalY() : 0) + this.y; };
        return Rn;
    }()), xt = /** @class */ (function (_super) {
        __extends(xt, _super);
        function xt() {
            var _this = _super.call(this) || this;
            pt(_this, "children");
            pt(_this, "sortableChildren");
            _this.children = [], _this.sortableChildren = !1;
            return _this;
        }
        xt.prototype.addChild = function () {
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
        xt.prototype.addChildAt = function (n, r) { var o; (o = n.parent) == null || o.removeChild(n), n.parent = this; var i = Math.max(0, Math.min(Number(r) | 0, this.children.length)); return this.children.splice(i, 0, n), n; };
        xt.prototype.removeChild = function () {
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
        xt.prototype.removeChildren = function (n, r) {
            var e_4, _a;
            if (n === void 0) { n = 0; }
            if (r === void 0) { r = this.children.length; }
            var i = Math.max(0, Number(n) | 0), o = Math.max(i, Math.min(Number(r) | 0, this.children.length)), s = this.children.splice(i, o - i);
            try {
                for (var s_1 = __values(s), s_1_1 = s_1.next(); !s_1_1.done; s_1_1 = s_1.next()) {
                    var u = s_1_1.value;
                    u.parent = null;
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
        xt.prototype.setChildIndex = function (n, r) { var i = this.children.indexOf(n); if (i < 0)
            return; this.children.splice(i, 1); var o = Math.max(0, Math.min(Number(r) | 0, this.children.length)); this.children.splice(o, 0, n); };
        xt.prototype.getChildIndex = function (n) { return this.children.indexOf(n); };
        xt.prototype.getChildByLabel = function (n) { for (var r = 0; r < this.children.length; r += 1) {
            var i = this.children[r];
            if (i && i.label === n)
                return i;
        } return null; };
        return xt;
    }(Rn)), bt = /** @class */ (function (_super) {
        __extends(bt, _super);
        function bt() {
            var _this = _super.call(this) || this;
            pt(_this, "commands");
            _this.commands = [];
            return _this;
        }
        bt.prototype.clear = function () { return this.commands.length = 0, this; };
        bt.prototype.rect = function (n, r, i, o) { return this.commands.push(["rect", n, r, i, o]), this; };
        bt.prototype.roundRect = function (n, r, i, o, s) {
            if (s === void 0) { s = 0; }
            return this.commands.push(["roundRect", n, r, i, o, s]), this;
        };
        bt.prototype.circle = function (n, r, i) { return this.commands.push(["circle", n, r, i]), this; };
        bt.prototype.ellipse = function (n, r, i, o) { return this.commands.push(["ellipse", n, r, i, o]), this; };
        bt.prototype.moveTo = function (n, r) { return this.commands.push(["moveTo", n, r]), this; };
        bt.prototype.lineTo = function (n, r) { return this.commands.push(["lineTo", n, r]), this; };
        bt.prototype.closePath = function () { return this.commands.push(["closePath"]), this; };
        bt.prototype.poly = function (n) { return this.commands.push(["poly", n]), this; };
        bt.prototype.fill = function (n) { return this.commands.push(["fill", n]), this; };
        bt.prototype.stroke = function (n) { return this.commands.push(["stroke", n]), this; };
        bt.prototype.image = function (n, r, i, o, s) { return this.commands.push(["image", n, r, i, o, s]), this; };
        bt.prototype.svg = function (n) { return this.commands.push(["svg", n]), this; };
        return bt;
    }(xt)), jt = /** @class */ (function (_super) {
        __extends(jt, _super);
        function jt(n) {
            if (n === void 0) { n = ""; }
            var r, i;
            var _this = _super.call(this) || this;
            pt(_this, "_text");
            pt(_this, "_style");
            pt(_this, "_resolution");
            _this._text = "", _this._style = {}, _this._resolution = 1, typeof n == "string" ? _this._text = n : (_this._text = String((r = n.text) != null ? r : ""), _this._style = te({}, (i = n.style) != null ? i : {}));
            return _this;
        }
        Object.defineProperty(jt.prototype, "text", {
            get: function () { return this._text; },
            set: function (n) { this._text = String(n != null ? n : ""); },
            enumerable: false,
            configurable: true
        });
        Object.defineProperty(jt.prototype, "style", {
            get: function () { return this._style; },
            set: function (n) { this._style = n != null ? n : {}; },
            enumerable: false,
            configurable: true
        });
        Object.defineProperty(jt.prototype, "resolution", {
            get: function () { return this._resolution; },
            set: function (n) { this._resolution = Math.max(1, Number(n) || 1); },
            enumerable: false,
            configurable: true
        });
        Object.defineProperty(jt.prototype, "width", {
            get: function () { var n = Number(this._style.fontSize) || 16; return this._text.length * n * .58; },
            enumerable: false,
            configurable: true
        });
        Object.defineProperty(jt.prototype, "height", {
            get: function () { var n = Number(this._style.fontSize) || 16; return Number(this._style.lineHeight) || n * 1.25; },
            enumerable: false,
            configurable: true
        });
        jt.prototype.setSize = function (n, r) { return this; };
        return jt;
    }(xt)), Ie = /** @class */ (function () {
        function Ie(e) {
            if (e === void 0) { e = {}; }
            pt(this, "options");
            this.options = e;
        }
        Ie.prototype.addAttribute = function (e, n) { return this; };
        Ie.prototype.destroy = function () { };
        return Ie;
    }()), Ze = /** @class */ (function (_super) {
        __extends(Ze, _super);
        function Ze(n) {
            if (n === void 0) { n = {}; }
            var r, i;
            var _this = _super.call(this) || this;
            pt(_this, "geometry");
            pt(_this, "shader");
            _this.geometry = (r = n.geometry) != null ? r : new Ie, _this.shader = (i = n.shader) != null ? i : new $e;
            return _this;
        }
        return Ze;
    }(xt)), qe = /** @class */ (function () {
        function qe(e) {
            if (e === void 0) { e = {}; }
            pt(this, "options");
            this.options = e;
        }
        return qe;
    }()), On = { VERTEX: 1, COPY_DST: 2 }, $e = /** @class */ (function () {
        function $e(e) {
            if (e === void 0) { e = {}; }
            pt(this, "options");
            this.options = e;
        }
        return $e;
    }());
    var rr = "", ir = "", or = "", tn = /** @class */ (function () {
        function tn() {
            var _this = this;
            pt(this, "stage");
            pt(this, "screen");
            pt(this, "canvas");
            pt(this, "renderer");
            pt(this, "ticker");
            var e = Math.max(1, Number(globalThis.innerWidth || 1920) | 0), n = Math.max(1, Number(globalThis.innerHeight || 1080) | 0);
            this.stage = new xt, this.screen = new gt(0, 0, e, n), this.canvas = document.createElement("canvas"), this.ticker = { stop: function () { }, add: function () { }, remove: function () { } }, this.renderer = { width: e, height: n, screen: this.screen, render: function (r) { return r; }, resize: function (r, i) { var o = Math.max(1, Number(r || e) | 0), s = Math.max(1, Number(i || n) | 0); _this.renderer.width = o, _this.renderer.height = s, _this.screen.width = o, _this.screen.height = s; } };
        }
        tn.prototype.init = function (e) { return Qe(this, null, function () { return __generator(this, function (_a) {
            return [2 /*return*/];
        }); }); };
        return tn;
    }());
    var ye = { fontFamily: "system-ui, -apple-system, Segoe UI, Arial", fontSize: 16, background: 16777215, text: 1118481, mutedText: 6710886, boxBorder: 14540253, hr: 13421772, control: { border: 0, focusBorder: 3900150, background: 16777215, accent: 3900150, radius: 0, button: { fill: 15921906, hoverFill: 15395562, activeFill: 14737632, border: 6710886, text: 1118481, radius: 0 }, progress: { border: 10066329, background: 16777215, fill: 6990335 }, table: { border: 10066329, cellBorder: 11579568, headerFill: 16250871 } } };
    var Me = 24, _t = 1;
    function zt(t) { var i, o; var e = t.wrapWidth, n = (i = t.wordWrap) != null ? i : e != null, r = (o = t.wordWrapWidth) != null ? o : e == null ? void 0 : Math.max(0, Math.ceil(e) + Me); return new jt({ text: t.text, style: { fontFamily: t.fontFamily, fontSize: t.fontSize, fill: t.fill, fontWeight: t.bold ? "700" : "400", wordWrap: n, wordWrapWidth: r } }); }
    function Cn(t, e) { var n = t.children; if (!Array.isArray(n))
        return null; for (var r = 0; r < n.length; r += 1) {
        var i = n[r];
        if (i && i.label === e)
            return i;
    } return null; }
    function le(t, e) { var n = Cn(t, e); if (n)
        return n; var r = new xt; return r.label = e, t.addChild(r), r; }
    function At(t, e) { var n = Cn(t, e); if (n)
        return n; var r = new bt; return r.label = e, t.addChild(r), r; }
    function Nt(t, e, n) { var r = Cn(t, e); if (r)
        return r; var i = new jt({ text: "" }); return i.label = e, n == null || n(i), t.addChild(i), i; }
    function kt(t) { t.clear(), t.removeAllListeners(), t.hitArea = null; }
    function Xt(t) { t.removeAllListeners(); }
    function ce(t, e, n) {
        var r = String(t != null ? t : ""), i = [], o = 0;
        for (var s = 0; s <= r.length; s++) {
            if (!(s === r.length || r[s] === "\n"))
                continue;
            var a = o, h = s;
            if (a === h)
                i.push({ start: a, end: h, text: "" });
            else {
                var p = a, d = -1;
                for (var b = p; b < h; b++) {
                    r[b] === " " && (d = b);
                    var I = r.slice(p, b + 1);
                    if (n(I) <= e || b === p)
                        continue;
                    var c = d >= p ? d + 1 : b;
                    c <= p && (c = Math.min(h, p + 1)), i.push({ start: p, end: c, text: r.slice(p, c) }), p = c, b = p - 1, d = -1;
                }
                p <= h && i.push({ start: p, end: h, text: r.slice(p, h) });
            }
            o = s + 1;
        }
        return i;
    }
    function ue(t, e) { return e <= 0 ? [] : t.length <= e ? t : t.slice(0, e); }
    function Se(t) { var e = t.fullText, n = t.lines, r = t.localX, i = t.localY, o = t.lineHeight, s = t.measure; if (n.length === 0)
        return 0; var u = Math.max(0, r), a = Math.max(0, i), h = Math.max(1, o), p = Math.max(0, Math.min(n.length - 1, Math.floor(a / h))), d = n[p], b = d.start, y = Number.POSITIVE_INFINITY; for (var I = d.start; I <= d.end; I++) {
        var c = s(e.slice(d.start, I)), _ = Math.abs(c - u);
        _ < y && (y = _, b = I);
    } return b; }
    function sr(t) { var I, c, _, g; var e = t.node, n = t.graphics, r = t.w, i = t.h, o = t.theme, s = Math.max(0, Math.round(r)), u = Math.max(0, Math.round(i)); n.rect(.5, .5, Math.max(0, s - 1), Math.max(0, u - 1)), n.fill(o.control.progress.background), n.stroke({ width: 1, color: o.control.progress.border }); var a = Number((c = (I = e.attrs) == null ? void 0 : I.value) != null ? c : "0"), h = Number((g = (_ = e.attrs) == null ? void 0 : _.max) != null ? g : "1"), p = h > 0 ? Math.max(0, Math.min(1, a / h)) : 0, d = 3, b = Math.max(0, s - d * 2), y = Math.max(0, u - d * 2); n.rect(d, d, Math.max(0, b * p), y), n.fill(o.control.progress.fill); }
    function ar(t, e) { t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0), t.setHeight(14), t.setMinWidth(240); }
    function xe(t, e, n) { var u; var r = t.get(e); if (r)
        return r; var i = Number((u = n == null ? void 0 : n.value) != null ? u : "0"), o = Number.isFinite(i) ? i : 0, s = { value: Math.max(0, Math.min(1, o)) }; return t.set(e, s), s; }
    function lr(t, e) { t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0), t.setHeight(14), t.setMinWidth(240); }
    function cr(t) { var e = t.node, n = t.Yoga, r = t.measurer, i = n.Node.create(); return i.setPadding(n.EDGE_LEFT, 0), i.setPadding(n.EDGE_RIGHT, 0), i.setPadding(n.EDGE_TOP, 0), i.setPadding(n.EDGE_BOTTOM, 0), i.setMargin(n.EDGE_RIGHT, 6), i.setMeasureFunc(function () { var o = r.measure("100"); return { width: o.width, height: o.height }; }), { yogaNode: i, buildBox: function () { return ({ kind: "block", key: e.key, tagName: e.tagName, attrs: e.attrs, x: i.getComputedLeft(), y: i.getComputedTop(), width: i.getComputedWidth(), height: i.getComputedHeight(), children: [] }); } }; }
    function ur(t) { var h, p; var e = t.node, n = t.container, r = t.theme, i = t.sliderStates, o = (h = e.attrs) == null ? void 0 : h["data-slider-key"], s = null; if (o) {
        var d = i.get(o);
        if (d)
            s = d;
        else {
            var b = (p = e.attrs) == null ? void 0 : p["data-slider-init"];
            s = xe(i, o, b != null ? { value: String(b) } : void 0);
        }
    } var u = s ? Math.round(s.value * 100) : 0, a = Nt(n, "__pct", function (d) { d.style = { fontFamily: r.fontFamily, fontSize: r.fontSize, fill: r.text, fontWeight: "400", wordWrap: !1 }; }); a.text = String(u), a.position.set(0, _t); }
    function en(t) { var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.absX, u = t.absY, a = t.theme, h = t.sliderStates, p = t.sliderBounds, d = t.sliderDrags, b = t.requestPaint, y = e.key, I = y ? xe(h, y, e.attrs) : null, c = Math.max(0, Math.round(i)), _ = Math.max(0, Math.round(o)), g = 3; y && p.set(y, { x: s, y: u, w: c, h: _, innerPad: g }), r.rect(.5, .5, Math.max(0, c - 1), Math.max(0, _ - 1)), r.fill(a.control.progress.background), r.stroke({ width: 1, color: a.control.progress.border }); var M = I ? Math.max(0, Math.min(1, I.value)) : 0, S = Math.max(0, c - g * 2), B = Math.max(0, _ - g * 2); r.rect(g, g, Math.max(0, S * M), B), r.fill(a.control.progress.fill); var $ = g + S * M, w = B / 2; r.moveTo($, g - w), r.lineTo($, g + B + w), r.stroke({ width: 2, color: a.text }), y && (Xt(n), n.eventMode = "static", n.cursor = "pointer", n.hitArea = new gt(0, 0, Math.max(0, c), Math.max(0, _)), n.on("pointerdown", function (R) {
        var e_5, _a;
        var Y, rt, ot, q, K, et;
        if ((R == null ? void 0 : R.button) === 2)
            return;
        var E = t.getPointerId ? t.getPointerId(R) : Number((ot = (rt = R == null ? void 0 : R.pointerId) != null ? rt : (Y = R == null ? void 0 : R.data) == null ? void 0 : Y.pointerId) != null ? ot : 0);
        if (E <= 0)
            return;
        try {
            for (var _b = __values(d.entries()), _c = _b.next(); !_c.done; _c = _b.next()) {
                var _d = __read(_c.value, 2), k = _d[0], H = _d[1];
                H.key === y && k !== E && d.delete(k);
            }
        }
        catch (e_5_1) { e_5 = { error: e_5_1 }; }
        finally {
            try {
                if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
            }
            finally { if (e_5) throw e_5.error; }
        }
        d.set(E, { key: y });
        var x = p.get(y), W = (K = (q = R.global) == null ? void 0 : q.x) != null ? K : 0, j = x ? W - x.x : 0, V = x ? Math.max(1, x.w - x.innerPad * 2) : 1, m = (j - ((et = x == null ? void 0 : x.innerPad) != null ? et : 0)) / V, D = xe(h, y, e.attrs);
        D.value = Math.max(0, Math.min(1, m)), b == null || b();
    })); }
    function dr(t) { var B; var e = t.node, n = t.container, r = t.w, i = t.h, o = t.theme, s = t.detailsOpen, u = t.requestRerender, a = (B = e.attrs) == null ? void 0 : B["data-details-key"], h = e.attrs ? Object.prototype.hasOwnProperty.call(e.attrs, "data-details-open") : !1, p = a && s.has(a) ? s.get(a) === !0 : h, d = function ($) { var E; if (!a || ($ == null ? void 0 : $.button) === 2)
        return; var R = !(s.has(a) ? s.get(a) === !0 : h); s.set(a, R), u == null || u(), (E = $ == null ? void 0 : $.stopPropagation) == null || E.call($); }, b = 16, y = At(n, "__arrow"); kt(y); var I = 2, c = 3, _ = c, g = c, M = b - c, S = b - c; p ? (y.moveTo(_, g), y.lineTo((_ + M) / 2, S), y.lineTo(M, g)) : (y.moveTo(_, g), y.lineTo(M, (g + S) / 2), y.lineTo(_, S)), y.stroke({ width: I, color: o.text }), y.position.set(4, Math.max(0, (i - b) / 2)), y.eventMode = "static", y.cursor = "pointer", y.hitArea = new gt(0, 0, b + 8, b + 8), y.on("pointerdown", d), a && (Xt(n), n.eventMode = "static", n.cursor = "pointer", n.hitArea = new gt(0, 0, Math.max(0, r), Math.max(0, i)), n.on("pointerdown", d)); }
    function hr(t, e) { t.setFlexDirection(e.FLEX_DIRECTION_ROW), t.setAlignItems(e.ALIGN_CENTER), t.setJustifyContent(e.JUSTIFY_SPACE_BETWEEN), t.setPadding(e.EDGE_TOP, 6), t.setPadding(e.EDGE_BOTTOM, 6), t.setPadding(e.EDGE_LEFT, 26), t.setPadding(e.EDGE_RIGHT, 12), t.setMinHeight(36); }
    function mr(t, e) { t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0); }
    function fr(t, e) { var i, o, s; if (!t || t.tagName !== "details" || !t.key)
        return (i = t == null ? void 0 : t.children) != null ? i : []; var n = t.attrs ? Object.prototype.hasOwnProperty.call(t.attrs, "open") : !1; return (e.has(t.key) ? e.get(t.key) === !0 : n) ? (o = t.children) != null ? o : [] : ((s = t.children) != null ? s : []).filter(function (u) { return u && u.kind === "block" && u.tagName === "summary"; }); }
    function pr(t) { var e = t.graphics, n = t.w, r = t.theme; e.rect(0, 0, Math.round(n), 1), e.fill(r.hr); }
    function gr(t, e) { t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0), t.setMargin(e.EDGE_TOP, 2), t.setMargin(e.EDGE_BOTTOM, 2), t.setHeight(1); }
    function br(t) { var _, g; var e = t.container, n = t.graphics, r = t.w, i = t.h, o = t.label, s = t.theme, u = t.registerHoverHandlers, a = function (M) { n.clear(); var S = 1, B = S / 2; s.control.button.radius > 0 ? n.roundRect(B, B, Math.max(0, r - S), Math.max(0, i - S), s.control.button.radius) : n.rect(B, B, Math.max(0, r - S), Math.max(0, i - S)), n.fill(M), n.stroke({ width: S, color: s.control.button.border }); }; a(s.control.button.fill); var h = Nt(e, "__label", function (M) { M.style = { fontFamily: s.fontFamily, fontSize: s.fontSize, fill: s.control.button.text, fontWeight: "400", wordWrap: !1, wordWrapWidth: 0 }; }), p = String(o != null ? o : "").trim(); h.text = p, h.visible = p.length > 0, h.style = _e(te({}, h.style), { fontFamily: s.fontFamily, fontSize: s.fontSize, fill: s.control.button.text, wordWrap: !1, wordWrapWidth: Math.max(0, Math.ceil(r - 16)) }); var d = Number((_ = h.width) != null ? _ : 0), b = Number((g = h.height) != null ? g : 0), y = s.fontSize * 1.25; h.position.set(d > 0 ? Math.max(8, Math.floor((r - d) / 2)) : 8, Math.max(0, Math.floor((i - (b > 0 ? b : y)) / 2)) + _t); var I = function () { return a(s.control.button.hoverFill); }, c = function () { return a(s.control.button.fill); }; u == null || u({ over: I, out: c }), Xt(e), e.eventMode = "static", e.cursor = "pointer", e.on("pointerover", I), e.on("pointerout", c), e.on("pointerdown", function () { return a(s.control.button.activeFill); }), e.on("pointerup", function () { return a(s.control.button.hoverFill); }); }
    function _r(t, e) { t.setFlexDirection(e.FLEX_DIRECTION_ROW), t.setPadding(e.EDGE_TOP, 6), t.setPadding(e.EDGE_BOTTOM, 6), t.setMinHeight(36), t.setMinWidth(100), t.setAlignItems(e.ALIGN_CENTER), t.setJustifyContent(e.JUSTIFY_CENTER); }
    function yr(t) { var e = t.graphics, n = t.w, r = t.h, i = t.boxBorder, o = Math.max(0, Math.round(n)), s = Math.max(0, Math.round(r)); e.rect(0, 0, o, s), e.stroke({ width: 1, color: i, alignment: 0 }); }
    function xr(t) { var e = t.nodeTag, n = t.graphics, r = t.w, i = t.h, o = t.theme; e === "th" && (n.rect(0, 0, r, i), n.fill(o.control.table.headerFill)), n.rect(0, 0, r, i), n.stroke({ width: 1, color: o.control.table.cellBorder }); }
    function wr(t, e) { t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0); }
    function Tr(t, e) { t.setFlexDirection(e.FLEX_DIRECTION_ROW), t.setFlexWrap(e.WRAP_NO_WRAP), t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0), t.setMargin(e.EDGE_BOTTOM, 0); }
    function Er(t, e) { t.setFlexGrow(1), t.setFlexShrink(1), t.setMinWidth(80), t.setPadding(e.EDGE_LEFT, 8), t.setPadding(e.EDGE_RIGHT, 8), t.setPadding(e.EDGE_TOP, 6), t.setPadding(e.EDGE_BOTTOM, 6), t.setMargin(e.EDGE_BOTTOM, 0); }
    function An(t) { var e = String(t != null ? t : "").toLowerCase(); if (e.length !== 2 || e.charAt(0) !== "h")
        return !1; var n = e.charCodeAt(1); return n >= 49 && n <= 54; }
    function Ir(t, e) { t.setPadding(e.EDGE_TOP, 6), t.setPadding(e.EDGE_BOTTOM, 6), t.setMinHeight(36), t.setJustifyContent(e.JUSTIFY_CENTER); }
    function Mr(t, e) {
        var n = Math.max(1, Math.floor(t)), r = Math.max(1, Math.floor(e));
        return "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<svg viewBox=\"0 0 ".concat(n, " ").concat(r, "\" xmlns=\"http://www.w3.org/2000/svg\">\n  <rect x=\"0\" y=\"0\" width=\"").concat(n, "\" height=\"").concat(r, "\" fill=\"#f6f6f6\"/>\n  <rect x=\"0.5\" y=\"0.5\" width=\"").concat(Math.max(0, n - 1), "\" height=\"").concat(Math.max(0, r - 1), "\" fill=\"none\" stroke=\"#999\"/>\n  <path d=\"M2 2 L").concat(Math.max(2, n - 2), " ").concat(Math.max(2, r - 2), "\" stroke=\"#c8c8c8\"/>\n  <path d=\"M").concat(Math.max(2, n - 2), " 2 L2 ").concat(Math.max(2, r - 2), "\" stroke=\"#c8c8c8\"/>\n</svg>");
    }
    function Sr(_a) {
        var _b = _a === void 0 ? {} : _a, _c = _b.ring, t = _c === void 0 ? 34 : _c, _d = _b.core, e = _d === void 0 ? 14 : _d, _f = _b.hueA, n = _f === void 0 ? "#00e5ff" : _f, _g = _b.hueB, r = _g === void 0 ? "#ff2bd6" : _g;
        var i = Math.max(0, t - 10), o = Math.max(0, e * .35);
        return "\n<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 100 100\">\n  <rect width=\"100\" height=\"100\" fill=\"#ffffff\"/>\n  <rect width=\"100\" height=\"100\" fill=\"".concat(n, "\" opacity=\"0.08\"/>\n\n  <circle cx=\"50\" cy=\"50\" r=\"").concat(t, "\" fill=\"none\" stroke=\"").concat(r, "\" stroke-width=\"4\" opacity=\"0.95\"/>\n  <circle cx=\"50\" cy=\"50\" r=\"").concat(i, "\" fill=\"none\" stroke=\"").concat(n, "\" stroke-width=\"1\" opacity=\"0.35\"/>\n\n  <circle cx=\"50\" cy=\"50\" r=\"").concat(e, "\" fill=\"").concat(n, "\" opacity=\"0.9\"/>\n  <circle cx=\"43\" cy=\"43\" r=\"").concat(o, "\" fill=\"#ffffff\" opacity=\"0.55\"/>\n\n  <path d=\"M50 16 L52 22 L58 24 L52 26 L50 32 L48 26 L42 24 L48 22 Z\" fill=\"#ffffff\" opacity=\"0.85\"/>\n  <path d=\"M82 52 L85 56 L90 57 L85 58 L82 62 L79 58 L74 57 L79 56 Z\" fill=\"#ffffff\" opacity=\"0.70\"/>\n  <path d=\"M20 70 L22 74 L27 75 L22 76 L20 80 L18 76 L13 75 L18 74 Z\" fill=\"#ffffff\" opacity=\"0.65\"/>\n</svg>\n");
    }
    var Pr = new Map;
    function We() { var t = globalThis; return !0; }
    function Or(t) { var e = globalThis, n = String(t != null ? t : "").trim(); if (!n)
        return null; var r = e.__TRUEOS_READY_IMAGE_TEXTURE_CACHE__; (!r || typeof r.get != "function" || typeof r.set != "function") && (r = new Map, e.__TRUEOS_READY_IMAGE_TEXTURE_CACHE__ = r); var i = r.get(n); if (i)
        return i; var o = e.__trueosResolveReadyImageTexture; if (typeof o != "function")
        return null; var s = o(n); if (s && typeof s.then == "function") {
        var u = s.then(function (a) { return (r.set(n, a), a); }).catch(function (a) { throw r.delete(n), a; });
        return r.set(n, u), u;
    } return r.set(n, s), s; }
    function Qi(t) { var s; var e = globalThis, n = String(t != null ? t : "").trim(); if (!n)
        return null; var r = e.__trueosPeekReadyImageTexture; if (typeof r != "function")
        return null; var i = r(n); return !i || typeof i.then == "function" ? null : Math.max(0, Number((s = i == null ? void 0 : i.texId) != null ? s : 0) | 0) > 0 ? i : null; }
    function Zi(t, e) { var n = globalThis, r = String(t != null ? t : "").trim(); if (!r || !e)
        return; var i = n.__TRUEOS_READY_IMAGE_TEXTURE_CACHE__; (!i || typeof i.get != "function" || typeof i.set != "function") && (i = new Map, n.__TRUEOS_READY_IMAGE_TEXTURE_CACHE__ = i), i.set(r, e); }
    function Cr(t, e) { var r, i, o, s, u; var n = Math.max(0, Number((r = e == null ? void 0 : e.texId) != null ? r : 0) | 0); if (n <= 0)
        throw new Error("image texture missing"); t.state = "ready", t.texId = n, t.width = Math.max(0, Number((o = (i = e == null ? void 0 : e.pixelWidth) != null ? i : e == null ? void 0 : e.width) != null ? o : 0) | 0), t.height = Math.max(0, Number((u = (s = e == null ? void 0 : e.pixelHeight) != null ? s : e == null ? void 0 : e.height) != null ? u : 0) | 0); }
    function qi(t, e) { var n = Qi(t) || Or(t); return !n || typeof n.then == "function" ? !1 : (Cr(e, n), Zi(t, n), !0); }
    function kr(t, e) { var n = String(t != null ? t : "").trim(); if (!n)
        return null; var r = Pr.get(n); if (r) {
        if (We() && r.state === "loading")
            try {
                qi(n, r);
            }
            catch (u) {
                r.state = "error";
            }
        return r;
    } if (We())
        return null; var i = { state: "loading", texId: 0, width: 0, height: 0 }; Pr.set(n, i); var o = function (u) { Cr(i, u), We() || e == null || e(); }, s = function () { i.state = "error", We() || e == null || e(); }; try {
        var u = Or(n);
        if (!u)
            return i;
        if (u && typeof u.then == "function") {
            if (We())
                return i;
            u.then(o).catch(s);
        }
        else
            o(u);
    }
    catch (u) {
        s();
    } return i; }
    function to(t) { var e = String(t != null ? t : ""); if (!e.startsWith("data:image/svg+xml"))
        return null; var n = e.indexOf(","); if (n === -1)
        return null; var r = e.slice(0, n).toLowerCase(), i = e.slice(n + 1); try {
        return r.includes(";base64") ? atob(i) : decodeURIComponent(i);
    }
    catch (o) {
        return null;
    } }
    function eo(t) { return Rr(Rr(String(t), "tspan"), "text"); }
    function no(t) { return "data:image/svg+xml;charset=utf-8,".concat(encodeURIComponent(t)); }
    function Rr(t, e) { var n = "", r = 0, i = t.toLowerCase(), o = "<" + e, s = "</" + e; for (; r < t.length;) {
        var u = i.indexOf(o, r);
        if (u < 0) {
            n += t.slice(r);
            break;
        }
        n += t.slice(r, u);
        var a = i.indexOf(s, u + o.length);
        if (a < 0)
            break;
        var h = t.indexOf(">", a + s.length);
        r = h < 0 ? t.length : h + 1;
    } return n; }
    function Ar(t) { var B, $, w, R; var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.theme, u = t.requestRerender, a = ($ = (B = e.attrs) == null ? void 0 : B.alt) != null ? $ : "", h = (R = (w = e.attrs) == null ? void 0 : w.src) != null ? R : "", p = h.trim().length > 0, d = a.trim().length > 0 ? a : h.trim().length > 0 ? h : "img", b = r.image, y = p ? kr(h, u) : null; if ((y == null ? void 0 : y.state) === "ready" && y.texId > 0 && typeof b == "function") {
        b.call(r, y.texId, 0, 0, Math.max(0, i), Math.max(0, o));
        var E = n.getChildByLabel ? n.getChildByLabel("__label") : n.children.find(function (x) { return (x == null ? void 0 : x.label) === "__label"; });
        E && (E.visible = !1);
        return;
    } var I = p ? to(h) : null, c = eo(I != null ? I : p ? Mr(i, o) : Sr({ ring: 34, core: 14 })), _ = At(n, "__svg"), g = kr(no(c), u); if ((g == null ? void 0 : g.state) === "ready" && g.texId > 0 && typeof _.image == "function") {
        var E = "texture:".concat(g.texId, ":").concat(Math.round(i), "x").concat(Math.round(o));
        if (_.__key !== E && (kt(_), _.image(g.texId, 0, 0, Math.max(0, i), Math.max(0, o)), _.__key = E), _.scale.set(1), _.position.set(0, 0), !p) {
            var x = n.getChildByLabel ? n.getChildByLabel("__label") : n.children.find(function (W) { return (W == null ? void 0 : W.label) === "__label"; });
            x && (x.visible = !1);
            return;
        }
        if (d.trim().length > 0) {
            var x = Nt(n, "__label", function (W) { W.style = { fontFamily: s.fontFamily, fontSize: s.fontSize, fill: s.mutedText, fontWeight: "400", wordWrap: !0, wordWrapWidth: 0 }; });
            x.text = d, x.style.wordWrapWidth = Math.max(0, Math.ceil(i - 16) + Me), x.position.set(8, 8 + _t), x.visible = !0;
        }
        return;
    }
    else
        kt(_); var M = _.svg; if (0 && _.__key !== E)
        try { }
        catch (W) { } r.rect(.5, .5, Math.max(0, i - 1), Math.max(0, o - 1)), r.fill(16185078), r.stroke({ width: 1, color: s.control.border }); var S = Nt(n, "__label", function (E) { E.style = { fontFamily: s.fontFamily, fontSize: s.fontSize, fill: s.mutedText, fontWeight: "400", wordWrap: !0, wordWrapWidth: 0 }; }); S.text = d, S.style.wordWrapWidth = Math.max(0, Math.ceil(i - 16) + Me), S.position.set(8, 8 + _t); }
    function Nr(t, e, n) { var h, p, d, b; t.setPadding(n.EDGE_LEFT, 0), t.setPadding(n.EDGE_RIGHT, 0), t.setPadding(n.EDGE_TOP, 0), t.setPadding(n.EDGE_BOTTOM, 0); var r = Number((p = (h = e.attrs) == null ? void 0 : h.width) != null ? p : "0"), i = Number((b = (d = e.attrs) == null ? void 0 : d.height) != null ? b : "0"), o = Number.isFinite(r) && r > 0, s = Number.isFinite(i) && i > 0, u = o ? r : 240, a = s ? i : 140; (o || s) && (t.setAlignSelf(n.ALIGN_FLEX_START), t.setFlexGrow(0), t.setFlexShrink(0)), t.setWidth(u), t.setHeight(a), t.setMinWidth(120), t.setMinHeight(80); }
    var Dr = new Map;
    function Fe() { var t = globalThis; return !0; }
    function vr(t) { var e = globalThis, n = String(t != null ? t : "").trim(); if (!n)
        return null; var r = e.__TRUEOS_READY_IMAGE_TEXTURE_CACHE__; (!r || typeof r.get != "function" || typeof r.set != "function") && (r = new Map, e.__TRUEOS_READY_IMAGE_TEXTURE_CACHE__ = r); var i = r.get(n); if (i)
        return i; var o = e.__trueosResolveReadyImageTexture; if (typeof o != "function")
        return null; var s = o(n); if (s && typeof s.then == "function") {
        var u = s.then(function (a) { return (r.set(n, a), a); }).catch(function (a) { throw r.delete(n), a; });
        return r.set(n, u), u;
    } return r.set(n, s), s; }
    function ro(t) { var s; var e = globalThis, n = String(t != null ? t : "").trim(); if (!n)
        return null; var r = e.__trueosPeekReadyImageTexture; if (typeof r != "function")
        return null; var i = r(n); return !i || typeof i.then == "function" ? null : Math.max(0, Number((s = i == null ? void 0 : i.texId) != null ? s : 0) | 0) > 0 ? i : null; }
    function io(t, e) { var n = globalThis, r = String(t != null ? t : "").trim(); if (!r || !e)
        return; var i = n.__TRUEOS_READY_IMAGE_TEXTURE_CACHE__; (!i || typeof i.get != "function" || typeof i.set != "function") && (i = new Map, n.__TRUEOS_READY_IMAGE_TEXTURE_CACHE__ = i), i.set(r, e); }
    function Gr(t, e) { var r, i, o, s, u; var n = Math.max(0, Number((r = e == null ? void 0 : e.texId) != null ? r : 0) | 0); if (n <= 0)
        throw new Error("svg texture missing"); t.state = "ready", t.texId = n, t.width = Math.max(0, Number((o = (i = e == null ? void 0 : e.pixelWidth) != null ? i : e == null ? void 0 : e.width) != null ? o : 0) | 0), t.height = Math.max(0, Number((u = (s = e == null ? void 0 : e.pixelHeight) != null ? s : e == null ? void 0 : e.height) != null ? u : 0) | 0); }
    function oo(t, e) { var n = ro(t) || vr(t); return !n || typeof n.then == "function" ? !1 : (Gr(e, n), io(t, n), !0); }
    function so(t) { return Lr(Lr(String(t), "tspan"), "text"); }
    function Lr(t, e) { var n = "", r = 0, i = t.toLowerCase(), o = "<" + e, s = "</" + e; for (; r < t.length;) {
        var u = i.indexOf(o, r);
        if (u < 0) {
            n += t.slice(r);
            break;
        }
        n += t.slice(r, u);
        var a = i.indexOf(s, u + o.length);
        if (a < 0)
            break;
        var h = t.indexOf(">", a + s.length);
        r = h < 0 ? t.length : h + 1;
    } return n; }
    function Hr(t) { var e = String(t), r = e.toLowerCase().indexOf("viewbox"); if (r < 0)
        return null; var i = e.indexOf("=", r + 7); if (i < 0)
        return null; var o = i + 1; for (; o < e.length;) {
        var y = e.charCodeAt(o);
        if (y !== 32 && y !== 9 && y !== 10 && y !== 13 && y !== 12)
            break;
        o += 1;
    } var s = e.charAt(o); if (s !== '"' && s !== "'")
        return null; var u = e.indexOf(s, o + 1); if (u < 0)
        return null; var a = ao(e.slice(o + 1, u)); if (a.length < 4)
        return null; var h = Number(a[0]), p = Number(a[1]), d = Number(a[2]), b = Number(a[3]); return ![h, p, d, b].every(function (y) { return Number.isFinite(y); }) || d <= 0 || b <= 0 ? null : { minX: h, minY: p, w: d, h: b }; }
    function ao(t) { var e = [], n = ""; for (var r = 0; r < t.length; r += 1) {
        var i = t.charCodeAt(r);
        i === 32 || i === 9 || i === 10 || i === 13 || i === 12 ? n.length > 0 && (e.push(n), n = "") : n += t.charAt(r);
    } return n.length > 0 && e.push(n), e; }
    function lo(t, e) { var n = String(t != null ? t : ""); if (!n.trim())
        return null; var r = Dr.get(n), i = "data:image/svg+xml;charset=utf-8,".concat(encodeURIComponent(n)); if (r) {
        if (Fe() && r.state === "loading")
            try {
                oo(i, r);
            }
            catch (a) {
                r.state = "error";
            }
        return r;
    } if (Fe())
        return null; var o = { state: "loading", texId: 0, width: 0, height: 0 }; Dr.set(n, o); var s = function (a) { Gr(o, a), Fe() || e == null || e(); }, u = function () { o.state = "error", Fe() || e == null || e(); }; try {
        var a = vr(i);
        if (!a)
            return o;
        if (a && typeof a.then == "function") {
            if (Fe())
                return o;
            a.then(s).catch(u);
        }
        else
            s(a);
    }
    catch (a) {
        u();
    } return o; }
    function co(t, e, n) { var r = Math.max(0, e), i = Math.max(0, n), o = Hr(t); if (!o || r <= 0 || i <= 0)
        return { x: 0, y: 0, w: r, h: i }; var s = r / o.w, u = i / o.h, a = Math.min(s, u), h = Math.max(0, o.w * a), p = Math.max(0, o.h * a); return { x: Math.max(0, (r - h) / 2), y: Math.max(0, (i - p) / 2), w: h, h: p }; }
    function $r(t, e, n) { var h, p, d, b; t.setPadding(n.EDGE_LEFT, 0), t.setPadding(n.EDGE_RIGHT, 0), t.setPadding(n.EDGE_TOP, 0), t.setPadding(n.EDGE_BOTTOM, 0); var r = Number((p = (h = e.attrs) == null ? void 0 : h.width) != null ? p : "0"), i = Number((b = (d = e.attrs) == null ? void 0 : d.height) != null ? b : "0"), o = Number.isFinite(r) && r > 0, s = Number.isFinite(i) && i > 0, u = o ? r : 300, a = s ? i : 150; (o || s) && (t.setAlignSelf(n.ALIGN_FLEX_START), t.setFlexGrow(0), t.setFlexShrink(0)), t.setWidth(u), t.setHeight(a), t.setMinWidth(Math.min(120, u)), t.setMinHeight(Math.min(80, a)); }
    function Wr(t) { var e = t.svgMarkup, n = t.container, r = t.w, i = t.h, o = t.requestRerender, s = so(e), u = At(n, "__svg"), a = u.__svgString, h = u.__w, p = u.__h, d = a !== s, b = lo(s, o); if (u.scale.set(1), u.position.set(0, 0), (b == null ? void 0 : b.state) === "ready" && b.texId > 0 && typeof u.image == "function") {
        if (d || h !== r || p !== i || u.__texId !== b.texId) {
            var I = co(s, r, i);
            kt(u), u.image(b.texId, I.x, I.y, I.w, I.h), u.__svgString = s, u.__w = r, u.__h = i, u.__texId = b.texId;
        }
        return;
    } kt(u); return; if (typeof y == "function") {
        if (d || h !== r || p !== i) {
            kt(u);
            var c = void 0;
            try {
                c = y.call(u, s);
            }
            catch (_) {
                c = null;
            }
            c && typeof c.then == "function" && c.then(function () { return o == null ? void 0 : o(); }).catch(function () { }), u.__svgString = s, u.__w = r, u.__h = i;
        }
        var I = Hr(s);
        if (I) {
            var c = r / I.w, _ = i / I.h, g = Math.min(c, _), M = I.w * g, S = I.h * g;
            u.scale.set(g), u.position.set(-I.minX * g + (r - M) / 2, -I.minY * g + (i - S) / 2);
        }
        return;
    } }
    function Fr(t, e, n) { var h, p, d, b; t.setPadding(n.EDGE_LEFT, 0), t.setPadding(n.EDGE_RIGHT, 0), t.setPadding(n.EDGE_TOP, 0), t.setPadding(n.EDGE_BOTTOM, 0); var r = Number((p = (h = e.attrs) == null ? void 0 : h.width) != null ? p : "0"), i = Number((b = (d = e.attrs) == null ? void 0 : d.height) != null ? b : "0"), o = Number.isFinite(r) && r > 0, s = Number.isFinite(i) && i > 0, u = o ? r : 300, a = s ? i : 150; (o || s) && (t.setAlignSelf(n.ALIGN_FLEX_START), t.setFlexGrow(0), t.setFlexShrink(0)), t.setWidth(u), t.setHeight(a), t.setMinWidth(Math.min(120, u)), t.setMinHeight(Math.min(80, a)); }
    function Br(t) { var e = t.graphics, n = t.container, r = t.w, i = t.h, o = t.theme, s = 1, u = s / 2; e.rect(u, u, Math.max(0, r - s), Math.max(0, i - s)), e.fill(16777215), e.stroke({ width: s, color: o.control.border, alignment: 0 }), e.moveTo(6, i - 6), e.lineTo(r - 6, 6), e.stroke({ width: 1, color: 0, alpha: .1 }); var a = zt({ text: "canvas", fontFamily: o.fontFamily, fontSize: Math.max(10, Math.floor(o.fontSize * .85)), fill: o.mutedText, wordWrap: !1 }); a.position.set(8, 8 + _t), n.addChild(a); }
    function Ur(t, e, n) { var p, d, b, y, I, c; var r = String((d = (p = e.attrs) == null ? void 0 : p["data-root"]) != null ? d : "") === "1"; if (t.setFlexDirection(n.FLEX_DIRECTION_COLUMN), t.setAlignItems(n.ALIGN_STRETCH), r) {
        t.setPadding(n.EDGE_LEFT, 0), t.setPadding(n.EDGE_RIGHT, 0), t.setPadding(n.EDGE_TOP, 0), t.setPadding(n.EDGE_BOTTOM, 0), t.setAlignSelf(n.ALIGN_STRETCH), t.setFlexGrow(1), t.setFlexShrink(1), t.setMinWidth(0), t.setMinHeight(0);
        return;
    } t.setPadding(n.EDGE_LEFT, 8), t.setPadding(n.EDGE_RIGHT, 8), t.setPadding(n.EDGE_BOTTOM, 8), t.setPadding(n.EDGE_TOP, 34); var i = Number((y = (b = e.attrs) == null ? void 0 : b.width) != null ? y : "0"), o = Number((c = (I = e.attrs) == null ? void 0 : I.height) != null ? c : "0"), s = Number.isFinite(i) && i > 0, u = Number.isFinite(o) && o > 0, a = s ? i : 420, h = u ? o : 240; (s || u) && (t.setAlignSelf(n.ALIGN_FLEX_START), t.setFlexGrow(0), t.setFlexShrink(0)), t.setWidth(a), t.setHeight(h), t.setMinWidth(Math.min(200, a)), t.setMinHeight(Math.min(160, h)); }
    function Xr(t) { var y, I, c, _; var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.theme; if (String((I = (y = e.attrs) == null ? void 0 : y["data-root"]) != null ? I : "") === "1")
        return; var a = 1, h = a / 2; r.rect(h, h, Math.max(0, i - a), Math.max(0, o - a)), r.fill(16777215), r.stroke({ width: a, color: s.control.border, alignment: 0 }), r.rect(h, h, Math.max(0, i - a), 26), r.fill({ color: 0, alpha: .04 }); var d = String((_ = (c = e.attrs) == null ? void 0 : c.srcdoc) != null ? _ : "").trim().length > 0 ? "srcdoc" : "empty", b = Nt(n, "__title", function (g) { g.style = { fontFamily: s.fontFamily, fontSize: Math.max(10, Math.floor(s.fontSize * .85)), fill: s.mutedText, fontWeight: "400", wordWrap: !1 }; }); b.text = "iframe (".concat(d, ")"), b.position.set(8, 6 + _t), n.eventMode = "static", n.cursor = "default", n.hitArea = new gt(0, 0, Math.max(0, i), Math.max(0, o)); }
    function Yr(t, e, n) { var i, o; var r = ((o = (i = e.attrs) == null ? void 0 : i.type) != null ? o : "text").toLowerCase(); r === "checkbox" || r === "radio" ? (t.setPadding(n.EDGE_LEFT, 0), t.setPadding(n.EDGE_RIGHT, 0), t.setPadding(n.EDGE_TOP, 0), t.setPadding(n.EDGE_BOTTOM, 0), t.setWidth(16), t.setHeight(16), t.setMinWidth(16), t.setMargin(n.EDGE_RIGHT, 6)) : (t.setPadding(n.EDGE_TOP, 6), t.setPadding(n.EDGE_BOTTOM, 6), t.setHeight(36), t.setMinHeight(36), t.setMinWidth(220)); }
    function Kr(t) {
        var e_6, _a, e_7, _b;
        var j, V, m, D, Y, rt, ot, q, K, et;
        var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.absX, u = t.absY, a = t.theme, h = t.textMeasure, p = t.uiState, d = t.getOrInitInputState, b = t.clamp, y = t.radioGroups, I = t.textDrags, c = t.requestPaint, _ = ((V = (j = e.attrs) == null ? void 0 : j.type) != null ? V : "text").toLowerCase(), g = e.key, M = g ? d(g, e.attrs) : void 0, S = (m = t.showCaret) != null ? m : !1, B = (D = t.caretPointerId) != null ? D : null, $ = t.focusColor, w = t.getCursorColor;
        n.sortableChildren = !0;
        try {
            for (var _c = __values(n.children), _d = _c.next(); !_d.done; _d = _c.next()) {
                var k = _d.value;
                var H = k.label;
                H && (H.startsWith("__sel:") || H === "__caret") && (k.visible = !1);
            }
        }
        catch (e_6_1) { e_6 = { error: e_6_1 }; }
        finally {
            try {
                if (_d && !_d.done && (_a = _c.return)) _a.call(_c);
            }
            finally { if (e_6) throw e_6.error; }
        }
        var R = 8, E = 6 + _t, x = 5, W = a.fontSize * 1.25;
        if (_ === "checkbox")
            r.rect(.5, .5, Math.max(0, i - 1), Math.max(0, o - 1)), r.fill(a.control.background), r.stroke({ width: 1, color: a.control.border }), M != null && M.indeterminate ? (r.moveTo(4, 4), r.lineTo(Math.max(4, i - 4), Math.max(4, o - 4)), r.stroke({ width: 2, color: a.control.accent }), r.moveTo(Math.max(4, i - 4), 4), r.lineTo(4, Math.max(4, o - 4)), r.stroke({ width: 2, color: a.control.accent })) : M != null && M.checked && (r.rect(3, 3, Math.max(0, i - 3 * 2), Math.max(0, o - 3 * 2)), r.fill(a.control.accent));
        else if (_ === "radio") {
            {
                var Z = Math.max(0, Math.min(i, o) / 2 - .5);
                r.circle(i / 2, o / 2, Z), r.fill(a.control.background), r.stroke({ width: 1, color: a.control.border });
            }
            if (M != null && M.checked) {
                var k = Math.max(0, Math.min(i, o) / 2 - 4.5);
                r.circle(i / 2, o / 2, k), r.fill(a.control.accent);
            }
        }
        else {
            var k = $ != null ? 2 : 1, H = k / 2;
            a.control.radius > 0 ? r.roundRect(H, H, Math.max(0, i - k), Math.max(0, o - k), a.control.radius) : r.rect(H, H, Math.max(0, i - k), Math.max(0, o - k)), r.fill(a.control.background), r.stroke({ width: k, color: $ != null ? $ : a.control.border });
            var Z = _ === "password" ? "\u2022".repeat(((Y = M == null ? void 0 : M.value) != null ? Y : "").length) : (rt = M == null ? void 0 : M.value) != null ? rt : "", G = Math.max(0, i - R * 2);
            g && p.fieldBounds.set(g, { x: s, y: u, w: i, h: o, innerLeft: R, innerTop: E, innerWidth: G, maxLines: x, isPassword: _ === "password" });
            var at = ce(Z, G, h), L = ue(at, x), f = L.length > 0 ? L[L.length - 1].end : 0;
            if (g && M && typeof M.value == "string") {
                var T = M.selections;
                if (T && T.size > 0)
                    try {
                        for (var _f = __values(T.entries()), _g = _f.next(); !_g.done; _g = _f.next()) {
                            var _h = __read(_g.value, 2), C = _h[0], P = _h[1];
                            var A = b((ot = P.start) != null ? ot : 0, 0, Z.length), N = b((q = P.end) != null ? q : A, 0, Z.length), J = b(Math.min(A, N), 0, f), z = b(Math.max(A, N), 0, f);
                            if (J === z)
                                continue;
                            var U = At(n, "__sel:".concat(C));
                            kt(U), U.zIndex = 0, U.visible = !0;
                            for (var Q = 0; Q < L.length; Q++) {
                                var nt = L[Q], ht = Math.max(J, nt.start), yt = Math.min(z, nt.end);
                                if (ht >= yt)
                                    continue;
                                var Tt = R + h(Z.slice(nt.start, ht)), Ot = h(Z.slice(ht, yt));
                                U.rect(Tt, E + Q * W, Ot, W);
                            }
                            U.fill({ color: w(C), alpha: .22 });
                        }
                    }
                    catch (e_7_1) { e_7 = { error: e_7_1 }; }
                    finally {
                        try {
                            if (_g && !_g.done && (_b = _f.return)) _b.call(_f);
                        }
                        finally { if (e_7) throw e_7.error; }
                    }
                if (S && B != null) {
                    var C = (K = M.selections) == null ? void 0 : K.get(B), P = C ? C.end : 0, A = b(P, 0, f), N = Math.max(0, L.length - 1);
                    for (var Q = 0; Q < L.length; Q++) {
                        var nt = L[Q];
                        if (A >= nt.start && A <= nt.end) {
                            N = Q;
                            break;
                        }
                    }
                    var J = (et = L[N]) != null ? et : { start: 0, end: 0, text: "" }, z = R + h(Z.slice(J.start, A)), U = At(n, "__caret");
                    kt(U), U.zIndex = 2, U.visible = !0, U.moveTo(z, E + N * W), U.lineTo(z, E + N * W + W), U.stroke({ width: 1, color: $ != null ? $ : a.control.focusBorder });
                }
            }
            var O = L.map(function (T) { return T.text; }).join("\n"), v = Nt(n, "__valueText", function (T) { T.style = { fontFamily: a.fontFamily, fontSize: a.fontSize, fill: a.text, fontWeight: "400", wordWrap: !1 }, T.zIndex = 1; });
            v.text = O, v.position.set(R, E);
        }
        g && (Xt(n), n.eventMode = "static", n.cursor = "text", n.on("pointerdown", function (k) {
            var e_8, _a, e_9, _b, e_10, _c;
            var Z, G, at, L, f, O, v, T, C, P, A, N, J;
            if ((k == null ? void 0 : k.button) === 2)
                return;
            var H = t.getPointerId ? t.getPointerId(k) : Number((at = (G = k == null ? void 0 : k.pointerId) != null ? G : (Z = k == null ? void 0 : k.data) == null ? void 0 : Z.pointerId) != null ? at : 0);
            if (!(H <= 0)) {
                if (p.focusedKeyByPointer.set(H, g), p.keyboardOwnerPointerId = H, _ === "checkbox") {
                    var z = d(g, e.attrs), U = z.indeterminate === !0, Q = z.checked === !0;
                    !Q && !U ? (z.checked = !0, z.indeterminate = !1) : Q && !U ? (z.checked = !1, z.indeterminate = !0) : (z.checked = !1, z.indeterminate = !1);
                }
                else if (_ === "radio") {
                    var U = "radio:".concat((f = (L = e.attrs) == null ? void 0 : L.name) != null ? f : "__default__"), Q = (O = y.get(U)) != null ? O : [];
                    try {
                        for (var Q_1 = __values(Q), Q_1_1 = Q_1.next(); !Q_1_1.done; Q_1_1 = Q_1.next()) {
                            var nt = Q_1_1.value;
                            var ht = d(nt, void 0);
                            ht.checked = nt === g;
                        }
                    }
                    catch (e_8_1) { e_8 = { error: e_8_1 }; }
                    finally {
                        try {
                            if (Q_1_1 && !Q_1_1.done && (_a = Q_1.return)) _a.call(Q_1);
                        }
                        finally { if (e_8) throw e_8.error; }
                    }
                }
                else {
                    var z = d(g, e.attrs);
                    if (typeof z.value == "string") {
                        try {
                            for (var _d = __values(p.inputs.entries()), _f = _d.next(); !_f.done; _f = _d.next()) {
                                var _g = __read(_f.value, 2), Rt = _g[0], Et = _g[1];
                                Rt !== g && ((v = Et.selections) == null || v.delete(H));
                            }
                        }
                        catch (e_9_1) { e_9 = { error: e_9_1 }; }
                        finally {
                            try {
                                if (_f && !_f.done && (_b = _d.return)) _b.call(_d);
                            }
                            finally { if (e_9) throw e_9.error; }
                        }
                        var U = _ === "password" ? "\u2022".repeat(z.value.length) : z.value, Q = p.fieldBounds.get(g), nt = (T = Q == null ? void 0 : Q.innerWidth) != null ? T : Math.max(0, i - R * 2), ht = ue(ce(U, nt, h), x), yt = ((P = (C = k.global) == null ? void 0 : C.x) != null ? P : 0) - s - R, Tt = ((N = (A = k.global) == null ? void 0 : A.y) != null ? N : 0) - u - E, Ot = Se({ fullText: U, lines: ht, localX: yt, localY: Tt, lineHeight: W, measure: h });
                        z.selections || (z.selections = new Map), z.selections.set(H, { start: Ot, end: Ot });
                        try {
                            for (var _h = __values(I.entries()), _j = _h.next(); !_j.done; _j = _h.next()) {
                                var _k = __read(_j.value, 2), Rt = _k[0], Et = _k[1];
                                Et.key === g && Rt !== H && I.delete(Rt);
                            }
                        }
                        catch (e_10_1) { e_10 = { error: e_10_1 }; }
                        finally {
                            try {
                                if (_j && !_j.done && (_c = _h.return)) _c.call(_h);
                            }
                            finally { if (e_10) throw e_10.error; }
                        }
                        I.set(H, { key: g, anchor: Ot });
                    }
                }
                (_ === "checkbox" || _ === "radio") && ((J = k.stopPropagation) == null || J.call(k)), c == null || c();
            }
        }), (_ === "checkbox" || _ === "radio") && (n.cursor = "pointer"), n.hitArea = new gt(0, 0, Math.max(0, i), Math.max(0, o)));
    }
    function zr(t, e) { t.setPadding(e.EDGE_TOP, 6), t.setPadding(e.EDGE_BOTTOM, 6), t.setHeight(108), t.setMinHeight(108), t.setMinWidth(220); }
    function jr(t) {
        var e_11, _a, e_12, _b;
        var q, K, et, k, H, Z, G, at;
        var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.absX, u = t.absY, a = t.theme, h = t.textMeasure, p = t.uiState, d = t.getOrInitInputState, b = t.clamp, y = t.textDrags, I = t.requestPaint, c = e.key, _ = c ? d(c, _e(te({}, (q = e.attrs) != null ? q : {}), { type: "text" })) : void 0, g = (K = t.showCaret) != null ? K : !1, M = (et = t.caretPointerId) != null ? et : null, S = t.focusColor, B = t.getCursorColor;
        n.sortableChildren = !0;
        try {
            for (var _c = __values(n.children), _d = _c.next(); !_d.done; _d = _c.next()) {
                var L = _d.value;
                var f = L.label;
                f && (f.startsWith("__sel:") || f === "__caret") && (L.visible = !1);
            }
        }
        catch (e_11_1) { e_11 = { error: e_11_1 }; }
        finally {
            try {
                if (_d && !_d.done && (_a = _c.return)) _a.call(_c);
            }
            finally { if (e_11) throw e_11.error; }
        }
        var $ = 8, w = 6 + _t, R = 5, E = a.fontSize * 1.25, x = S != null ? 2 : 1, W = x / 2;
        a.control.radius > 0 ? r.roundRect(W, W, Math.max(0, i - x), Math.max(0, o - x), a.control.radius) : r.rect(W, W, Math.max(0, i - x), Math.max(0, o - x)), r.fill(a.control.background), r.stroke({ width: x, color: S != null ? S : a.control.border });
        var j = (k = _ == null ? void 0 : _.value) != null ? k : "", V = Math.max(0, i - $ * 2);
        c && p.fieldBounds.set(c, { x: s, y: u, w: i, h: o, innerLeft: $, innerTop: w, innerWidth: V, maxLines: R, isPassword: !1 });
        var m = ce(j, V, h), D = ue(m, R), Y = D.length > 0 ? D[D.length - 1].end : 0;
        if (c && _ && typeof _.value == "string") {
            var L = _.selections;
            if (L && L.size > 0)
                try {
                    for (var _f = __values(L.entries()), _g = _f.next(); !_g.done; _g = _f.next()) {
                        var _h = __read(_g.value, 2), f = _h[0], O = _h[1];
                        var v = b((H = O.start) != null ? H : 0, 0, j.length), T = b((Z = O.end) != null ? Z : v, 0, j.length), C = b(Math.min(v, T), 0, Y), P = b(Math.max(v, T), 0, Y);
                        if (C === P)
                            continue;
                        var A = At(n, "__sel:".concat(f));
                        kt(A), A.zIndex = 0, A.visible = !0;
                        for (var N = 0; N < D.length; N++) {
                            var J = D[N], z = Math.max(C, J.start), U = Math.min(P, J.end);
                            if (z >= U)
                                continue;
                            var Q = $ + h(j.slice(J.start, z)), nt = h(j.slice(z, U));
                            A.rect(Q, w + N * E, nt, E);
                        }
                        A.fill({ color: B(f), alpha: .22 });
                    }
                }
                catch (e_12_1) { e_12 = { error: e_12_1 }; }
                finally {
                    try {
                        if (_g && !_g.done && (_b = _f.return)) _b.call(_f);
                    }
                    finally { if (e_12) throw e_12.error; }
                }
            if (g && M != null) {
                var f = (G = _.selections) == null ? void 0 : G.get(M), O = f ? f.end : 0, v = b(O, 0, Y), T = Math.max(0, D.length - 1);
                for (var N = 0; N < D.length; N++) {
                    var J = D[N];
                    if (v >= J.start && v <= J.end) {
                        T = N;
                        break;
                    }
                }
                var C = (at = D[T]) != null ? at : { start: 0, end: 0, text: "" }, P = $ + h(j.slice(C.start, v)), A = At(n, "__caret");
                kt(A), A.zIndex = 2, A.visible = !0, A.moveTo(P, w + T * E), A.lineTo(P, w + T * E + E), A.stroke({ width: 1, color: S != null ? S : a.control.focusBorder });
            }
        }
        var rt = D.map(function (L) { return L.text; }).join("\n"), ot = Nt(n, "__valueText", function (L) { L.style = { fontFamily: a.fontFamily, fontSize: a.fontSize, fill: a.text, fontWeight: "400", wordWrap: !1 }, L.zIndex = 1; });
        ot.text = rt, ot.position.set($, w), c && (Xt(n), n.eventMode = "static", n.cursor = "text", n.hitArea = new gt(0, 0, Math.max(0, i), Math.max(0, o)), n.on("pointerdown", function (L) {
            var e_13, _a, e_14, _b;
            var v, T, C, P, A, N, J, z, U, Q;
            if ((L == null ? void 0 : L.button) === 2)
                return;
            var f = t.getPointerId ? t.getPointerId(L) : Number((C = (T = L == null ? void 0 : L.pointerId) != null ? T : (v = L == null ? void 0 : L.data) == null ? void 0 : v.pointerId) != null ? C : 0);
            if (f <= 0)
                return;
            p.focusedKeyByPointer.set(f, c), p.keyboardOwnerPointerId = f;
            var O = d(c, _e(te({}, (P = e.attrs) != null ? P : {}), { type: "text" }));
            if (typeof O.value == "string") {
                try {
                    for (var _c = __values(p.inputs.entries()), _d = _c.next(); !_d.done; _d = _c.next()) {
                        var _f = __read(_d.value, 2), Ht = _f[0], Ct = _f[1];
                        Ht !== c && ((A = Ct.selections) == null || A.delete(f));
                    }
                }
                catch (e_13_1) { e_13 = { error: e_13_1 }; }
                finally {
                    try {
                        if (_d && !_d.done && (_a = _c.return)) _a.call(_c);
                    }
                    finally { if (e_13) throw e_13.error; }
                }
                var nt = p.fieldBounds.get(c), ht = (N = nt == null ? void 0 : nt.innerWidth) != null ? N : Math.max(0, i - $ * 2), yt = O.value, Tt = ue(ce(yt, ht, h), R), Ot = ((z = (J = L.global) == null ? void 0 : J.x) != null ? z : 0) - s - $, Rt = ((Q = (U = L.global) == null ? void 0 : U.y) != null ? Q : 0) - u - w, Et = Se({ fullText: yt, lines: Tt, localX: Ot, localY: Rt, lineHeight: E, measure: h });
                O.selections || (O.selections = new Map), O.selections.set(f, { start: Et, end: Et });
                try {
                    for (var _g = __values(y.entries()), _h = _g.next(); !_h.done; _h = _g.next()) {
                        var _j = __read(_h.value, 2), Ht = _j[0], Ct = _j[1];
                        Ct.key === c && Ht !== f && y.delete(Ht);
                    }
                }
                catch (e_14_1) { e_14 = { error: e_14_1 }; }
                finally {
                    try {
                        if (_h && !_h.done && (_b = _g.return)) _b.call(_g);
                    }
                    finally { if (e_14) throw e_14.error; }
                }
                y.set(f, { key: c, anchor: Et });
            }
            I == null || I();
        }));
    }
    function Vr(t, e) { t.setFlexDirection(e.FLEX_DIRECTION_ROW), t.setAlignItems(e.ALIGN_CENTER), t.setJustifyContent(e.JUSTIFY_FLEX_START), t.setPadding(e.EDGE_LEFT, 8), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0); }
    function uo(t, e, n, r, i) { t.circle(e, n, r), t.stroke({ width: 2, color: i }); var o = e + r * .65, s = n + r * .65, u = e + r * 1.55, a = n + r * 1.55; t.moveTo(o, s), t.lineTo(u, a), t.stroke({ width: 2, color: i }); }
    function Jr(t, e) { t.setFlexDirection(e.FLEX_DIRECTION_ROW), t.setFlexWrap(e.WRAP_NO_WRAP), t.setAlignItems(e.ALIGN_CENTER), t.setJustifyContent(e.JUSTIFY_FLEX_START), t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0); }
    function Qr(t, e) { t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0), t.setWidth(36), t.setHeight(36), t.setMinWidth(36), t.setMinHeight(36), t.setFlexGrow(0), t.setFlexShrink(0), t.setMargin(e.EDGE_RIGHT, 6); }
    function Zr(t) { var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.theme, u = t.uiState, a = t.getPointerId, h = t.focusInputKey, p = t.requestPaint, d = function (y) { r.clear(); var I = 1, c = I / 2; s.control.button.radius > 0 ? r.roundRect(c, c, Math.max(0, i - I), Math.max(0, o - I), s.control.button.radius) : r.rect(c, c, Math.max(0, i - I), Math.max(0, o - I)), r.fill(y), r.stroke({ width: I, color: s.control.button.border }); var _ = i / 2 - 2, g = o / 2 - 2, M = Math.max(5, Math.min(7, Math.min(i, o) * .22)); uo(r, _, g, M, s.text); }; d(s.control.button.fill), Xt(n), n.eventMode = "static", n.cursor = "pointer", n.hitArea = new gt(0, 0, Math.max(0, i), Math.max(0, o)), n.on("pointerover", function () { return d(s.control.button.hoverFill); }), n.on("pointerout", function () { return d(s.control.button.fill); }), n.on("pointerdown", function (y) { var I; if ((y == null ? void 0 : y.button) !== 2) {
        if (d(s.control.button.activeFill), h) {
            var c = a(y);
            c > 0 && (u.focusedKeyByPointer.set(c, h), u.keyboardOwnerPointerId = c);
        }
        p == null || p(), (I = y.stopPropagation) == null || I.call(y);
    } }), n.on("pointerup", function () { return d(s.control.button.hoverFill); }); var b = e.attrs; }
    function nn(t, e) { var n = t.get(e); if (n)
        return n; var r = { x: 0, y: 0 }; return t.set(e, r), r; }
    function qr(t, e) { t.setPositionType(e.POSITION_TYPE_ABSOLUTE), t.setPosition(e.EDGE_LEFT, 0), t.setPosition(e.EDGE_TOP, 0), t.setAlignSelf(e.ALIGN_FLEX_START), t.setFlexGrow(0), t.setFlexShrink(0), t.setPadding(e.EDGE_LEFT, 12), t.setPadding(e.EDGE_RIGHT, 12), t.setPadding(e.EDGE_TOP, 12), t.setPadding(e.EDGE_BOTTOM, 12), t.setWidth(540), t.setMinWidth(360), t.setMinHeight(148); }
    function ti(t) { var B, $; var e = t.node, n = t.container, r = t.w, i = t.h, o = t.theme, s = t.selectedBy, u = t.getCursorColor, a = t.dialogStates, h = t.dialogDrags, p = t.bringToFront, d = t.requestPaint, b = e.key; if (!b)
        return; var y = s.get(b), I = y == null ? o.boxBorder : u(y), c = Math.max(0, Math.round(r)), _ = Math.max(0, Math.round(i)), g = At(n, "__dialogBorder"); kt(g), g.rect(0, 0, c, _), g.fill({ color: 16777215, alpha: .8 }); var M = y == null ? 1 : 2, S = M / 2; g.rect(S, S, Math.max(0, c - M), Math.max(0, _ - M)), g.stroke({ width: M, color: I, alignment: 0 }), g.eventMode = "static", g.cursor = "move", g.hitArea = new gt(0, 0, c, _), g.on("pointerdown", function (w) {
        var e_15, _a;
        var W, j, V, m, D, Y, rt, ot;
        var R = function (q) { try {
            typeof console != "undefined" && typeof console.log == "function" && console.log("[dialog pointerdown] ".concat(q));
        }
        catch (K) { } };
        if (R("start"), (w == null ? void 0 : w.button) === 2)
            return;
        R("pointer-id");
        var E = t.getPointerId ? t.getPointerId(w) : Number((V = (j = w == null ? void 0 : w.pointerId) != null ? j : (W = w == null ? void 0 : w.data) == null ? void 0 : W.pointerId) != null ? V : 0);
        if (E <= 0 || E <= 0)
            return;
        R("clear-other-drags");
        try {
            for (var _b = __values(h.entries()), _c = _b.next(); !_c.done; _c = _b.next()) {
                var _d = __read(_c.value, 2), q = _d[0], K = _d[1];
                K.key === b && q !== E && h.delete(q);
            }
        }
        catch (e_15_1) { e_15 = { error: e_15_1 }; }
        finally {
            try {
                if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
            }
            finally { if (e_15) throw e_15.error; }
        }
        R("select"), s.set(b, E), R("bring-to-front"), p == null || p(b), R("state");
        var x = nn(a, b);
        R("set-drag"), h.set(E, { key: b, startGX: (D = (m = w.global) == null ? void 0 : m.x) != null ? D : 0, startGY: (rt = (Y = w.global) == null ? void 0 : Y.y) != null ? rt : 0, originX: x.x, originY: x.y }), R("request-paint"), d == null || d(), R("stop-propagation"), (ot = w.stopPropagation) == null || ot.call(w), R("done");
    }); {
        var w = n.getChildByLabel, R = ($ = (B = w == null ? void 0 : w.call(n, "__children")) != null ? B : n.children.find(function (E) { return E && E.label === "__children"; })) != null ? $ : null;
        if (R && g.parent === n) {
            var E = n.getChildIndex(R), x = Math.max(0, n.children.length - 1), W = Math.max(0, Math.min(E - 1, x));
            n.getChildIndex(g) > W && n.setChildIndex(g, W);
        }
    } }
    function Dn(t, e, n) { var u; var r = t.get(e); if (r)
        return r; var i = Number((u = n == null ? void 0 : n.value) != null ? u : "0"), s = { value: Number.isFinite(i) ? i : 0 }; return t.set(e, s), s; }
    function ei(t, e) { t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0), t.setHeight(36), t.setMinHeight(36), t.setMinWidth(140), t.setFlexGrow(0), t.setFlexShrink(0); }
    function ho(t, e, n) { return Math.max(e, Math.min(n, t)); }
    function Nn(t, e, n) { var i; var r = Number((i = t == null ? void 0 : t[e]) != null ? i : ""); return Number.isFinite(r) ? r : n; }
    function mo(t, e, n, r, i, o) { var u = e + 3, a = e + r - 3, h = n + 3, p = n + i - 3; t.moveTo(u, p), t.lineTo((u + a) / 2, h), t.lineTo(a, p), t.stroke({ width: 2, color: o }); }
    function fo(t, e, n, r, i, o) { var u = e + 3, a = e + r - 3, h = n + 3, p = n + i - 3; t.moveTo(u, h), t.lineTo((u + a) / 2, p), t.lineTo(a, h), t.stroke({ width: 2, color: o }); }
    function ni(t) { var V; var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.theme, u = t.getValue, a = t.setValue, h = t.requestPaint, p = e.key, d = e.attrs, b = Nn(d, "min", 0), y = Nn(d, "max", 255), I = Math.max(1e-9, Nn(d, "step", 1)), c = u(), _ = 1, g = _ / 2; r.rect(g, g, Math.max(0, i - _), Math.max(0, o - _)), r.fill(s.control.background), r.stroke({ width: _, color: s.control.border }); var M = 22, S = Math.max(0, i - M); r.moveTo(S + .5, 0), r.lineTo(S + .5, o), r.stroke({ width: 1, color: s.control.border, alignment: 0 }); var B = At(n, "__arrows"); kt(B), mo(B, S, 0, M, o / 2, s.text), fo(B, S, o / 2, M, o / 2, s.text); var $ = ((V = d == null ? void 0 : d.channel) != null ? V : "").toLowerCase(), w = $ === "r" ? "R" : $ === "g" ? "G" : $ === "b" ? "B" : $ === "a" ? "A" : "", R = Nt(n, "__valueText", function (m) { m.style = { fontFamily: s.fontFamily, fontSize: s.fontSize, fill: s.text, fontWeight: "400", wordWrap: !1 }; }); if (R.text = w ? "".concat(w, ": ").concat(Math.round(c)) : String(Math.round(c)), R.position.set(8, 9 + _t), !p)
        return; var E = new gt(S, 0, M, o / 2), x = new gt(S, o / 2, M, o / 2), W = function (m) { var D = u(), Y = ho(D + m * I, b, y); a(Y), h == null || h(); }, j = At(n, "__hit"); kt(j), j.eventMode = "static", j.cursor = "default", j.hitArea = new gt(0, 0, Math.max(0, i), Math.max(0, o)), j.on("pointerdown", function (m) {
        var e_16, _a;
        var et, k, H, Z, G, at;
        if ((m == null ? void 0 : m.button) === 2)
            return;
        var D = t.getPointerId ? t.getPointerId(m) : Number((H = (k = m == null ? void 0 : m.pointerId) != null ? k : (et = m == null ? void 0 : m.data) == null ? void 0 : et.pointerId) != null ? H : 0);
        if (D <= 0)
            return;
        var Y = n.toLocal(m.global), rt = (Z = Y == null ? void 0 : Y.x) != null ? Z : 0, ot = (G = Y == null ? void 0 : Y.y) != null ? G : 0, q = E.contains(rt, ot) ? 1 : x.contains(rt, ot) ? -1 : null;
        if (!q)
            return;
        W(q);
        var K = t.numberHolds;
        if (K && p) {
            try {
                for (var _b = __values(K.entries()), _c = _b.next(); !_c.done; _c = _b.next()) {
                    var _d = __read(_c.value, 2), O = _d[0], v = _d[1];
                    O !== D && (v.timeoutId != null && window.clearTimeout(v.timeoutId), v.intervalId != null && window.clearInterval(v.intervalId), K.delete(O));
                }
            }
            catch (e_16_1) { e_16 = { error: e_16_1 }; }
            finally {
                try {
                    if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
                }
                finally { if (e_16) throw e_16.error; }
            }
            var L = K.get(D);
            L && (L.timeoutId != null && window.clearTimeout(L.timeoutId), L.intervalId != null && window.clearInterval(L.intervalId));
            var f_1 = { key: p, timeoutId: null, intervalId: null };
            f_1.timeoutId = window.setTimeout(function () { f_1.timeoutId = null, f_1.intervalId = window.setInterval(function () { W(q); }, 250); }, 500), K.set(D, f_1);
        }
        (at = m.stopPropagation) == null || at.call(m);
    }); }
    var rn = null;
    function ri() { return rn || (rn = new qe({ data: ne, label: "attribute-color-picker-colors", shrinkToFit: !1, usage: On.VERTEX | On.COPY_DST }), rn); }
    function ii(t, e, n) { var h, p, d, b; t.setPadding(n.EDGE_LEFT, 0), t.setPadding(n.EDGE_RIGHT, 0), t.setPadding(n.EDGE_TOP, 0), t.setPadding(n.EDGE_BOTTOM, 0); var r = Number((p = (h = e.attrs) == null ? void 0 : h.width) != null ? p : "0"), i = Number((b = (d = e.attrs) == null ? void 0 : d.height) != null ? b : "0"), o = Number.isFinite(r) && r > 0, s = Number.isFinite(i) && i > 0, u = o ? r : 240, a = s ? i : 200; (o || s) && (t.setAlignSelf(n.ALIGN_FLEX_START), t.setFlexGrow(0), t.setFlexShrink(0)), t.setWidth(u), t.setHeight(a), t.setMinWidth(Math.min(240, u)), t.setMinHeight(Math.min(200, a)); }
    function se(t) { return Number.isFinite(t) ? Math.max(0, Math.min(255, Math.round(t))) : 0; }
    function on(t) { return se(t).toString(16).padStart(2, "0"); }
    function po(t, e, n, r, i, o, s, u) { var a = s - n, h = u - r, p = i - n, d = o - r, b = t - n, y = e - r, I = a * a + h * h, c = a * p + h * d, _ = a * b + h * y, g = p * p + d * d, M = p * b + d * y, S = 1 / (I * g - c * c), B = (g * _ - c * M) * S, $ = (I * M - c * _) * S; return B >= 0 && $ >= 0 && B + $ <= 1; }
    function go(t, e, n, r, i, o, s, u) { var a = i - n, h = o - r, p = s - n, d = u - r, b = t - n, y = e - r, I = a * d - p * h; if (Math.abs(I) < 1e-9)
        return { w0: 1, w1: 0, w2: 0 }; var c = (b * d - p * y) / I, _ = (a * y - b * h) / I; return { w0: 1 - c - _, w1: c, w2: _ }; }
    var bo = { name: "solid-out", fragment: { main: "\n      outColor = vec4(1.0);\n    " } }, sn = null;
    function _o() { if (sn)
        return sn; var t = { name: "color-picker-vertex-color", bits: [ir, or, rr, bo] }; return sn = new $e({ glProgram: t, resources: {} }), sn; }
    function oi(t, e, n) { var r = new Float32Array(12), i = [-90, -30, 30, 90, 150, 210]; for (var o = 0; o < 6; o++) {
        var s = i[o] * Math.PI / 180;
        r[o * 2 + 0] = t + Math.cos(s) * n, r[o * 2 + 1] = e + Math.sin(s) * n;
    } return r; }
    var ne = new Uint8Array([255, 0, 0, 255, 128, 128, 0, 255, 0, 255, 0, 255, 0, 128, 128, 255, 0, 0, 255, 255, 128, 0, 128, 255]), Pe = new Uint32Array([0, 1, 2, 0, 2, 3, 0, 3, 4, 0, 4, 5]);
    function Ln(t) { var e = t.lx, n = t.ly, r = t.w, i = t.h, o = 10, s = Math.max(0, r - o * 2), u = Math.max(0, i - o * 2), a = o + s / 2, h = o + u / 2, p = Math.max(0, Math.min(s, u) / 2 - 2), d = oi(a, h, p); for (var b = 0; b < Pe.length; b += 3) {
        var y = Pe[b + 0], I = Pe[b + 1], c = Pe[b + 2], _ = d[y * 2 + 0], g = d[y * 2 + 1], M = d[I * 2 + 0], S = d[I * 2 + 1], B = d[c * 2 + 0], $ = d[c * 2 + 1];
        if (!po(e, n, _, g, M, S, B, $))
            continue;
        var w = go(e, n, _, g, M, S, B, $), R = y * 4, E = I * 4, x = c * 4, W = w.w0 * ne[R + 0] + w.w1 * ne[E + 0] + w.w2 * ne[x + 0], j = w.w0 * ne[R + 1] + w.w1 * ne[E + 1] + w.w2 * ne[x + 1], V = w.w0 * ne[R + 2] + w.w1 * ne[E + 2] + w.w2 * ne[x + 2];
        return { r: se(W), g: se(j), b: se(V) };
    } return null; }
    function si(t) { var K, et; var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.theme, u = t.rgb, a = t.setRgb, h = t.alpha, p = t.setAlpha, d = t.pick, b = t.setPick, y = t.requestPaint, I = t.getPointerId, c = t.setDraggingPointerId, _ = 1, g = _ / 2; r.rect(g, g, Math.max(0, i - _), Math.max(0, o - _)), r.fill(16777215), r.stroke({ width: _, color: s.control.border, alignment: 0 }); var M = 10, S = Math.max(0, i - M * 2), B = Math.max(0, o - M * 2), $ = M + S / 2, w = M + B / 2, R = Math.max(0, Math.min(S, B) / 2 - 2), E = oi($, w, R), x = "".concat(Math.round(i), "x").concat(Math.round(o)), W = n.getChildByLabel, j = W ? W.call(n, "__mesh") : n.children.find(function (k) { return (k == null ? void 0 : k.label) === "__mesh"; }); if (j) {
        if (j.__sizeKey !== x) {
            var k = new Float32Array(E.length), H = new Ie({ positions: E, uvs: k, indices: Pe });
            H.addAttribute("aColor", { buffer: ri(), format: "unorm8x4", stride: 4, offset: 0 });
            try {
                (et = (K = j.geometry) == null ? void 0 : K.destroy) == null || et.call(K);
            }
            catch (Z) { }
            j.geometry = H, j.__sizeKey = x;
        }
    }
    else {
        var k = new Float32Array(E.length), H = new Ie({ positions: E, uvs: k, indices: Pe });
        H.addAttribute("aColor", { buffer: ri(), format: "unorm8x4", stride: 4, offset: 0 }), j = new Ze({ geometry: H, shader: _o() }), j.label = "__mesh", n.addChild(j), j.__sizeKey = x;
    } j.removeAllListeners(), j.eventMode = "static", j.cursor = "crosshair", j.hitArea = new gt(M, M, S, B), j.on("pointerdown", function (k) { var f, O, v; if ((k == null ? void 0 : k.button) === 2)
        return; var H = I(k); if (H <= 0)
        return; var Z = n.toLocal(k.global), G = (f = Z == null ? void 0 : Z.x) != null ? f : 0, at = (O = Z == null ? void 0 : Z.y) != null ? O : 0, L = Ln({ lx: G, ly: at, w: i, h: o }); L && (b({ x: G, y: at }), a(L), c(H), y == null || y(), (v = k.stopPropagation) == null || v.call(k)); }); {
        var k = At(n, "__border");
        kt(k), k.moveTo(E[0], E[1]);
        for (var H = 1; H < 6; H++)
            k.lineTo(E[H * 2 + 0], E[H * 2 + 1]);
        k.closePath(), k.stroke({ width: 2, color: 0 });
    } var V = At(n, "__overlay"); kt(V); var m = 44, D = 18, Y = Math.max(M, i - M - m), rt = M; V.rect(Y, rt, m, D), V.fill({ color: se(u.r) << 16 | se(u.g) << 8 | se(u.b), alpha: Math.max(0, Math.min(1, se(h) / 255)) }), V.rect(Y + .5, rt + .5, m - 1, D - 1), V.stroke({ width: 1, color: s.control.border, alignment: 0 }), d && (V.circle(d.x, d.y, 4), V.stroke({ width: 2, color: 16777215 }), V.circle(d.x, d.y, 4), V.stroke({ width: 1, color: 0 })); var ot = "#".concat(on(u.r)).concat(on(u.g)).concat(on(u.b)).concat(on(h)).toUpperCase(), q = Nt(n, "__label", function (k) { k.style = { fontFamily: s.fontFamily, fontSize: Math.max(10, Math.floor(s.fontSize * .75)), fill: s.mutedText, fontWeight: "400", wordWrap: !1 }; }); q.text = ot, q.position.set(M, Math.max(M, o - M - q.height)), p && p(se(h)); }
    function de(t, e, n) { var r = t.get(e); if (r)
        return r; var i = { selectedIndex: Math.max(0, n | 0), open: !1 }; return t.set(e, i), i; }
    function ai(t, e) { t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0), t.setHeight(36), t.setMinHeight(36), t.setMinWidth(220); }
    function yo(t, e, n, r, i, o) { var u = e + 4, a = e + r - 4, h = n + 4, p = n + i - 4; t.moveTo(u, (h + p) / 2 - 2), t.lineTo((u + a) / 2, (h + p) / 2 + 2), t.lineTo(a, (h + p) / 2 - 2), t.stroke({ width: 2, color: o }); }
    function xo(t) {
        var r;
        var n = String((r = t == null ? void 0 : t["data-options"]) != null ? r : "").split("\n").map(function (i) { return i.trim(); }).filter(function (i) { return i.length > 0; });
        return n.length > 0 ? n : ["(empty)"];
    }
    function wo(t) { var n; var e = Number((n = t == null ? void 0 : t["data-selected-index"]) != null ? n : "0"); return Number.isFinite(e) ? Math.max(0, e | 0) : 0; }
    function an(t) { var j; var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.absX, u = t.absY, a = t.theme, h = t.selectStates, p = t.uiState, d = t.getPointerId, b = t.getCursorColor, y = t.requestPaint, I = t.popupSink, c = e.key; if (!c)
        return; var _ = xo(e.attrs), g = wo(e.attrs), M = de(h, c, g); M.selectedIndex = Math.max(0, Math.min(_.length - 1, M.selectedIndex | 0)); var S = (function () {
        var e_17, _a;
        var V = p.keyboardOwnerPointerId;
        if (p.focusedKeyByPointer.get(V) === c)
            return V;
        try {
            for (var _b = __values(p.focusedKeyByPointer.entries()), _c = _b.next(); !_c.done; _c = _b.next()) {
                var _d = __read(_c.value, 2), m = _d[0], D = _d[1];
                if (D === c)
                    return m;
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
    })(), B = S != null ? b(S) : null, $ = B != null ? 2 : 1, w = $ / 2; a.control.radius > 0 ? r.roundRect(w, w, Math.max(0, i - $), Math.max(0, o - $), a.control.radius) : r.rect(w, w, Math.max(0, i - $), Math.max(0, o - $)), r.fill(a.control.background), r.stroke({ width: $, color: B != null ? B : a.control.border }); var R = 22, E = Math.max(0, i - R); r.moveTo(E + .5, 0), r.lineTo(E + .5, o), r.stroke({ width: 1, color: a.control.border, alignment: 0 }), yo(r, E, 0, R, o, a.text); var x = (j = _[M.selectedIndex]) != null ? j : "", W = Nt(n, "__label", function (V) { V.style = { fontFamily: a.fontFamily, fontSize: a.fontSize, fill: a.text, fontWeight: "400", wordWrap: !1 }; }); W.text = x, W.position.set(8, 9 + _t), Xt(n), n.eventMode = "static", n.cursor = "pointer", n.hitArea = new gt(0, 0, Math.max(0, i), Math.max(0, o)), n.on("pointerdown", function (V) { var D; if ((V == null ? void 0 : V.button) === 2)
        return; var m = d(V); m <= 0 || (p.focusedKeyByPointer.set(m, c), p.keyboardOwnerPointerId = m, M.open = !M.open, y == null || y(), (D = V.stopPropagation) == null || D.call(V)); }), M.open && I.push({ key: c, absX: s, absY: u, w: i, h: o, options: _, selectedIndex: M.selectedIndex }); }
    function li(t) { var M; var e = t.popup, n = t.stage, r = t.theme, i = t.selectStates, o = t.uiState, s = t.getPointerId, u = t.requestPaint, a = t.viewportW, h = t.viewportH, p = 30, b = Math.min(7, e.options.length), y = b * p, I = e.absX, c = e.absY + e.h; I = Math.max(0, Math.min(I, Math.max(0, a - e.w))), c + y > h - 4 && (c = e.absY - y), c = Math.max(0, Math.min(c, Math.max(0, h - y))); var _ = new xt; _.position.set(I, c), n.addChild(_); var g = new bt; g.rect(0, 0, e.w, y), g.fill(16777215), g.rect(.5, .5, Math.max(0, e.w - 1), Math.max(0, y - 1)), g.stroke({ width: 1, color: r.control.border, alignment: 0 }), _.addChild(g), _.eventMode = "static", _.cursor = "pointer", _.hitArea = new gt(0, 0, e.w, y), _.on("pointerdown", function (S) { var W, j, V; if ((S == null ? void 0 : S.button) === 2)
        return; var B = s(S), $ = _.toLocal(S.global), w = (W = $ == null ? void 0 : $.x) != null ? W : -1, R = (j = $ == null ? void 0 : $.y) != null ? j : -1; if (w < 0 || w > e.w || R < 0 || R > y)
        return; var E = Math.max(0, Math.min(e.options.length - 1, Math.floor(R / p))), x = i.get(e.key); x && (x.selectedIndex = E, x.open = !1), B > 0 && (o.focusedKeyByPointer.set(B, e.key), o.keyboardOwnerPointerId = B), u == null || u(), (V = S.stopPropagation) == null || V.call(S); }); for (var S = 0; S < b; S++) {
        var B = S * p;
        if (S === e.selectedIndex) {
            var w = new bt;
            w.rect(1, B + 1, Math.max(0, e.w - 2), p - 2), w.fill({ color: 0, alpha: .06 }), _.addChild(w);
        }
        var $ = zt({ text: (M = e.options[S]) != null ? M : "", fontFamily: r.fontFamily, fontSize: r.fontSize, fill: r.text, wordWrap: !1 });
        $.position.set(8, B + 7 + _t), _.addChild($);
    } }
    function Dt(t, e, n) { var r = Number.isFinite(t) ? t | 0 : 0; return Math.max(e, Math.min(n, r)); }
    function Kt(t) { var e = Dt(t, 0, 99); return e < 10 ? "0".concat(e) : String(e); }
    function Zt(t, e, n) { var r = Number(t); if (!Number.isFinite(r))
        return null; var i = Math.trunc(r); return i < e || i > n ? null : i; }
    function un(t) { if (t.length !== 4)
        return null; for (var r = 0; r < t.length; r += 1) {
        var i = t.charCodeAt(r);
        if (i < 48 || i > 57)
            return null;
    } var e = Number(t); if (!Number.isFinite(e))
        return null; var n = e - 2e3; return n < 0 || n > 99 ? null : n; }
    function To(t) { var e = String(t != null ? t : "").trim().split(":"); if (e.length !== 2 && e.length !== 3)
        return null; var n = Zt(e[0], 0, 23), r = Zt(e[1], 0, 59), i = e.length === 3 ? Zt(e[2], 0, 59) : 0; return n == null || r == null || i == null ? null : { hour: n, minute: r, second: i }; }
    function Eo(t) { var e = String(t != null ? t : "").trim().split("-"); if (e.length !== 2)
        return null; var n = un(e[0]), r = Zt(e[1], 1, 12); return n == null || r == null ? null : { year2: n, month: r }; }
    function Io(t) { var e = String(t != null ? t : "").trim().split("-"); if (e.length !== 3)
        return null; var n = un(e[0]), r = Zt(e[1], 1, 12), i = Zt(e[2], 1, 31); if (n == null || r == null || i == null)
        return null; var o = Dt(Math.floor((i - 1) / 7) + 1, 1, 4); return { year2: n, month: r, weekIndex: o }; }
    function Mo(t) { var e = String(t != null ? t : "").trim(), n = e.indexOf("-W"); if (n < 0)
        return null; var r = un(e.slice(0, n)), i = Zt(e.slice(n + 2), 1, 48); if (r == null || i == null)
        return null; var o = Dt(Math.floor((i - 1) / 4) + 1, 1, 12), s = Dt((i - 1) % 4 + 1, 1, 4); return { year2: r, month: o, weekIndex: s }; }
    function So(t) { var e = String(t != null ? t : "").trim(), n = e.indexOf("T"); if (n < 0 && (n = e.indexOf(" ")), n < 0)
        return null; var r = e.slice(0, n).split("-"), i = e.slice(n + 1).split(":"); if (r.length !== 3 || i.length !== 2 && i.length !== 3)
        return null; var o = un(r[0]), s = Zt(r[1], 1, 12), u = Zt(r[2], 1, 31), a = Zt(i[0], 0, 23), h = Zt(i[1], 0, 59), p = i.length === 3 ? Zt(i[2], 0, 59) : 0; if (o == null || s == null || u == null || a == null || h == null || p == null)
        return null; var d = Dt(Math.floor((u - 1) / 7) + 1, 1, 4); return { year2: o, month: s, weekIndex: d, hour: a, minute: h, second: p }; }
    function ln(t) { return "20".concat(Kt(t.year2), "-").concat(Kt(t.month)); }
    function Po(t) { return (Dt(t.month, 1, 12) - 1) * 4 + Dt(t.weekIndex, 1, 4); }
    function cn(t) { return "20".concat(Kt(t.year2), "-W").concat(Kt(Po(t))); }
    function ke(t) { var e = (Dt(t.weekIndex, 1, 4) - 1) * 7 + 1; return "20".concat(Kt(t.year2), "-").concat(Kt(t.month), "-").concat(Kt(e)); }
    function Xe(t) { return "".concat(Kt(t.hour), ":").concat(Kt(t.minute), ":").concat(Kt(t.second)); }
    function Be(t) { return "".concat(ke(t), "T").concat(Xe(t)); }
    function ko(t) { var p; var e = t.map, n = t.yearSliderOwners, r = t.inputKey, i = t.kind, o = t.attrs, s = e.get(r); if (s)
        return s.kind = i, s; var u = new Date, a = { kind: i, year2: Dt(u.getFullYear() - 2e3, 0, 99), month: Dt(u.getMonth() + 1, 1, 12), weekIndex: 1, hour: Dt(u.getHours(), 0, 23), minute: Dt(u.getMinutes(), 0, 59), second: Dt(u.getSeconds(), 0, 59), openPanel: null, openYear: !1, openMonthGrid: !1, yearSliderKey: "".concat(r, ":year-slider") }, h = String((p = o == null ? void 0 : o.value) != null ? p : ""); if (h.trim().length > 0) {
        if (i === "time") {
            var d = To(h);
            d && (a.hour = d.hour, a.minute = d.minute, a.second = d.second);
        }
        else if (i === "month") {
            var d = Eo(h);
            d && (a.year2 = d.year2, a.month = d.month);
        }
        else if (i === "week") {
            var d = Mo(h);
            d && (a.year2 = d.year2, a.month = d.month, a.weekIndex = d.weekIndex);
        }
        else if (i === "date") {
            var d = Io(h);
            d && (a.year2 = d.year2, a.month = d.month, a.weekIndex = d.weekIndex);
        }
        else if (i === "datetime-local") {
            var d = So(h);
            d && (a.year2 = d.year2, a.month = d.month, a.weekIndex = d.weekIndex, a.hour = d.hour, a.minute = d.minute, a.second = d.second);
        }
    } return e.set(r, a), n.set(a.yearSliderKey, r), a; }
    function ui(t, e, n) { t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0), t.setHeight(36), t.setMinHeight(36), t.setMinWidth(n === "datetime-local" ? 340 : 220); }
    function Ro(t, e, n, r, i) { var o = i != null ? 2 : 1, s = o / 2; e.control.radius > 0 ? t.roundRect(s, s, Math.max(0, n - o), Math.max(0, r - o), e.control.radius) : t.rect(s, s, Math.max(0, n - o), Math.max(0, r - o)), t.fill(e.control.background), t.stroke({ width: o, color: i != null ? i : e.control.border }); }
    function ci(t, e, n, r, i) { var o = e + r / 2, s = n + r / 2; t.moveTo(e, s - 2), t.lineTo(o, s + 2), t.lineTo(e + r, s - 2), t.stroke({ width: 2, color: i }); }
    function di(t) { var E, x; var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.absX, u = t.absY, a = t.theme, h = t.uiState, p = t.getPointerId, d = t.getCursorColor, b = t.temporalStates, y = t.yearSliderOwners, I = t.getOrInitInputValue, c = t.requestPaint, _ = t.popupSink, g = e.key; if (!g || !e.tagName)
        return; var M = e.tagName === "timeinput" ? "time" : e.tagName === "monthinput" ? "month" : e.tagName === "weekinput" ? "week" : e.tagName === "dateinput" ? "date" : "datetime-local", S = ko({ map: b, yearSliderOwners: y, inputKey: g, kind: M, attrs: e.attrs }), B = I(g, _e(te({}, (E = e.attrs) != null ? E : {}), { type: "text" })); M === "time" ? B.value = Xe(S) : M === "month" ? B.value = ln(S) : M === "week" ? B.value = cn(S) : M === "date" ? B.value = ke(S) : B.value = Be(S); var $ = (function () {
        var e_18, _a;
        var W = h.keyboardOwnerPointerId;
        if (h.focusedKeyByPointer.get(W) === g)
            return W;
        try {
            for (var _b = __values(h.focusedKeyByPointer.entries()), _c = _b.next(); !_c.done; _c = _b.next()) {
                var _d = __read(_c.value, 2), j = _d[0], V = _d[1];
                if (V === g)
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
    })(), w = $ != null ? d($) : null; Ro(r, a, i, o, w); var R = 8; if (M !== "datetime-local") {
        var W = (x = B.value) != null ? x : "", j = Nt(n, "__shown", function (D) { D.style = { fontFamily: a.fontFamily, fontSize: a.fontSize, fill: a.text, fontWeight: "400", wordWrap: !1 }; });
        j.text = W, j.visible = !0, j.position.set(R, 9 + _t);
        var V = n.getChildByLabel ? n.getChildByLabel("__date") : n.children.find(function (D) { return (D == null ? void 0 : D.label) === "__date"; }), m = n.getChildByLabel ? n.getChildByLabel("__time") : n.children.find(function (D) { return (D == null ? void 0 : D.label) === "__time"; });
        V && (V.visible = !1), m && (m.visible = !1), ci(r, Math.max(0, i - 18), 11, 10, a.text);
    }
    else {
        var W = Math.max(0, Math.round(i * .52));
        r.moveTo(W + .5, 0), r.lineTo(W + .5, o), r.stroke({ width: 1, color: a.control.border, alignment: 0 });
        var j = ke(S), V = Xe(S), m = Nt(n, "__date", function (rt) { rt.style = { fontFamily: a.fontFamily, fontSize: a.fontSize, fill: a.text, fontWeight: "400", wordWrap: !1 }; });
        m.text = j, m.visible = !0, m.position.set(R, 9 + _t);
        var D = Nt(n, "__time", function (rt) { rt.style = { fontFamily: a.fontFamily, fontSize: a.fontSize, fill: a.text, fontWeight: "400", wordWrap: !1 }; });
        D.text = V, D.visible = !0, D.position.set(W + R, 9 + _t);
        var Y = n.getChildByLabel ? n.getChildByLabel("__shown") : n.children.find(function (rt) { return (rt == null ? void 0 : rt.label) === "__shown"; });
        Y && (Y.visible = !1), ci(r, Math.max(W + 0, W + (i - W) - 18), 11, 10, a.text);
    } Xt(n), n.eventMode = "static", n.cursor = "pointer", n.hitArea = new gt(0, 0, Math.max(0, i), Math.max(0, o)), n.on("pointerdown", function (W) { var V, m, D; if ((W == null ? void 0 : W.button) === 2)
        return; var j = p(W); if (!(j <= 0)) {
        if (h.focusedKeyByPointer.set(j, g), h.keyboardOwnerPointerId = j, M !== "datetime-local")
            S.openPanel = S.openPanel ? null : M === "time" ? "time" : M === "month" ? "month" : "week", S.openYear = !1, S.openMonthGrid = !1;
        else {
            var ot = ((m = (V = W.global) == null ? void 0 : V.x) != null ? m : 0) - s <= i * .52;
            S.openPanel = ot ? S.openPanel === "week" ? null : "week" : S.openPanel === "time" ? null : "time", S.openYear = !1, S.openMonthGrid = !1;
        }
        b.set(g, S), c == null || c(), (D = W.stopPropagation) == null || D.call(W);
    } }), S.openPanel === "month" ? _.push({ kind: "month-panel", inputKey: g, absX: s, absY: u, anchorW: i, anchorH: o }) : S.openPanel === "week" ? _.push({ kind: "week-panel", inputKey: g, absX: s, absY: u, anchorW: i, anchorH: o }) : S.openPanel === "time" && _.push({ kind: "time-panel", inputKey: g, absX: s, absY: u, anchorW: i, anchorH: o }); }
    function Ue(t, e, n, r) { t.rect(0, 0, n, r), t.fill(e.control.background), t.rect(.5, .5, Math.max(0, n - 1), Math.max(0, r - 1)), t.stroke({ width: 1, color: e.control.border, alignment: 0 }); }
    function Oo(t) { var e = t.stage, n = t.theme, r = t.popup, i = t.st, o = t.viewportW, s = t.viewportH, u = t.getPointerId, a = t.requestPaint, h = t.onPick, p = 4, d = 3, b = 44, y = 34, I = 8, c = I * 2 + p * b, _ = I * 2 + d * y, g = r.absX, M = r.absY + r.anchorH; g = Math.max(0, Math.min(g, Math.max(0, o - c))), M + _ > s - 4 && (M = r.absY - _), M = Math.max(0, Math.min(M, Math.max(0, s - _))); var S = new xt; S.position.set(g, M), e.addChild(S); var B = new bt; Ue(B, n, c, _), S.addChild(B); for (var $ = 0; $ < 12; $++) {
        var w = $ + 1, R = I + $ % p * b, E = I + Math.floor($ / p) * y;
        if (w === i.month) {
            var W = new bt;
            W.rect(R + 1, E + 1, b - 2, y - 2), W.fill({ color: 0, alpha: .06 }), S.addChild(W);
        }
        var x = zt({ text: String(w), fontFamily: n.fontFamily, fontSize: n.fontSize, fill: n.text, wordWrap: !1 });
        x.position.set(R + 14, E + 8 + _t), S.addChild(x), B.rect(R, E, b, y), B.stroke({ width: 1, color: n.control.border, alignment: 0 });
    } S.eventMode = "static", S.cursor = "pointer", S.hitArea = new gt(0, 0, c, _), S.on("pointerdown", function ($) { var rt, ot, q; if (($ == null ? void 0 : $.button) === 2 || u($) <= 0)
        return; var R = S.toLocal($.global), E = (rt = R == null ? void 0 : R.x) != null ? rt : -1, x = (ot = R == null ? void 0 : R.y) != null ? ot : -1, W = E - I, j = x - I; if (W < 0 || j < 0)
        return; var V = Math.floor(W / b), m = Math.floor(j / y); if (V < 0 || V >= p || m < 0 || m >= d)
        return; var Y = m * p + V + 1; Y < 1 || Y > 12 || (h(Y), a == null || a(), (q = $.stopPropagation) == null || q.call($)); }); }
    function Co(t) {
        var e_19, _a;
        var e = t.stage, n = t.theme, r = t.popup, i = t.st, o = t.viewportW, s = t.viewportH, u = t.sliders, a = t.sliderBounds, h = t.sliderDrags, p = t.getPointerId, d = t.requestPaint, b = t.onChange, y = 10, I = 250, c = 78, _ = r.absX, g = r.absY;
        _ = r.absX + r.anchorW + 6, g = r.absY, _ = Math.max(0, Math.min(_, Math.max(0, o - I))), g = Math.max(0, Math.min(g, Math.max(0, s - c)));
        var M = new xt;
        M.position.set(_, g), e.addChild(M);
        var S = new bt;
        Ue(S, n, I, c), M.addChild(S);
        var B = zt({ text: "20".concat(Kt(i.year2)), fontFamily: n.fontFamily, fontSize: n.fontSize, fill: n.text, wordWrap: !1 });
        B.position.set(y, 8 + _t), M.addChild(B);
        var $ = i.yearSliderKey, w = Math.max(0, Math.min(1, Dt(i.year2, 0, 99) / 99)), R = xe(u, $, { value: String(w) }), E = !1;
        try {
            for (var _b = __values(h.values()), _c = _b.next(); !_c.done; _c = _b.next()) {
                var V = _c.value;
                if (V.key === $) {
                    E = !0;
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
        E || (R.value = w);
        var x = new xt;
        x.position.set(y, 40), M.addChild(x);
        var W = new bt;
        x.addChild(W), en({ node: { key: $, attrs: { value: String(R.value) } }, container: x, graphics: W, w: I - y * 2, h: 14, absX: _ + y, absY: g + 40, theme: { text: n.text, control: { progress: n.control.progress } }, sliderStates: u, sliderBounds: a, sliderDrags: h, requestPaint: d, getPointerId: p });
        var j = Dt(Math.round(R.value * 99), 0, 99);
        j !== i.year2 && b(j), M.eventMode = "static", M.hitArea = new gt(0, 0, I, c), M.on("pointerdown", function (V) { var m; (m = V.stopPropagation) == null || m.call(V); });
    }
    function Ao(t) { var e = t.panel, n = t.theme, r = t.x, i = t.y, o = t.w, s = t.st, u = t.onPick, a = 30, h = 6, p = []; for (var d = 0; d < 4; d++) {
        var b = d + 1, y = i + d * (a + h), I = new bt;
        I.rect(r, y, o, a), I.fill({ color: 0, alpha: b === s.weekIndex ? .06 : .03 }), I.rect(r + .5, y + .5, Math.max(0, o - 1), Math.max(0, a - 1)), I.stroke({ width: 1, color: n.control.border, alignment: 0 }), e.addChild(I);
        var c = (Dt(s.month, 1, 12) - 1) * 4 + b, _ = zt({ text: "".concat(b, " [").concat(Kt(c), "]"), fontFamily: n.fontFamily, fontSize: n.fontSize, fill: n.text, wordWrap: !1 });
        _.position.set(r + 10, y + 7 + _t), e.addChild(_), p.push({ x: r, y: y, w: o, h: a, weekIndex: b });
    } return { hitRects: p }; }
    function hi(t) {
        var e_20, _a, e_21, _b;
        var M, S, B, $, w, R;
        var e = t.popups, n = t.stage, r = t.theme, i = t.viewportW, o = t.viewportH, s = t.temporalStates, u = t.getOrInitInputValue, a = t.sliders, h = t.sliderBounds, p = t.sliderDrags, d = t.selects, b = t.selectPopups, y = t.getCursorColor, I = t.uiFocus, c = t.getPointerId, _ = t.requestPaint, g = [];
        var _loop_1 = function (E) {
            var x = s.get(E.inputKey);
            if (x) {
                if (E.kind === "month-panel") {
                    var K = E.absX, et = E.absY + E.anchorH;
                    K = Math.max(0, Math.min(K, Math.max(0, i - 196))), et + 156 > o - 4 && (et = E.absY - 156), et = Math.max(0, Math.min(et, Math.max(0, o - 156)));
                    var k_1 = new xt;
                    k_1.position.set(K, et), n.addChild(k_1);
                    var H = new bt;
                    Ue(H, r, 196, 156), k_1.addChild(H);
                    var Z_1 = { x: 10, y: 10, w: 132, h: 24 };
                    {
                        var L = new bt;
                        L.rect(Z_1.x, Z_1.y, Z_1.w, Z_1.h), L.fill({ color: 0, alpha: .03 }), L.rect(Z_1.x + .5, Z_1.y + .5, Math.max(0, Z_1.w - 1), Math.max(0, Z_1.h - 1)), L.stroke({ width: 1, color: r.control.border, alignment: 0 }), k_1.addChild(L);
                        var f = zt({ text: "Year 20".concat(Kt(x.year2)), fontFamily: r.fontFamily, fontSize: r.fontSize, fill: r.text, wordWrap: !1 });
                        f.position.set(Z_1.x + 8, Z_1.y + 4 + _t), k_1.addChild(f);
                    }
                    var G_1 = 10, at_1 = 44;
                    for (var L = 0; L < 12; L++) {
                        var f = L + 1, O = G_1 + L % 4 * 44, v = at_1 + Math.floor(L / 4) * 34;
                        if (f === x.month) {
                            var C = new bt;
                            C.rect(O + 1, v + 1, 42, 32), C.fill({ color: 0, alpha: .06 }), k_1.addChild(C);
                        }
                        var T = zt({ text: String(f), fontFamily: r.fontFamily, fontSize: r.fontSize, fill: r.text, wordWrap: !1 });
                        T.position.set(O + 14, v + 8 + _t), k_1.addChild(T), H.rect(O, v, 44, 34), H.stroke({ width: 1, color: r.control.border, alignment: 0 });
                    }
                    k_1.eventMode = "static", k_1.cursor = "pointer", k_1.hitArea = new gt(0, 0, 196, 156), k_1.on("pointerdown", function (L) { var nt, ht, yt, Tt; if ((L == null ? void 0 : L.button) === 2)
                        return; var f = c(L); if (f <= 0)
                        return; I.focusedKeyByPointer.set(f, E.inputKey), I.keyboardOwnerPointerId = f; var O = k_1.toLocal(L.global), v = (nt = O == null ? void 0 : O.x) != null ? nt : -1, T = (ht = O == null ? void 0 : O.y) != null ? ht : -1; if (v >= Z_1.x && v <= Z_1.x + Z_1.w && T >= Z_1.y && T <= Z_1.y + Z_1.h) {
                        x.openYear = !0, s.set(E.inputKey, x), _ == null || _(), (yt = L.stopPropagation) == null || yt.call(L);
                        return;
                    } var P = v - G_1, A = T - at_1; if (P < 0 || A < 0)
                        return; var N = Math.floor(P / 44), J = Math.floor(A / 34); if (N < 0 || N >= 4 || J < 0 || J >= 3)
                        return; var U = J * 4 + N + 1; if (U < 1 || U > 12)
                        return; x.month = U, x.openPanel = null, x.openYear = !1, x.openMonthGrid = !1, s.set(E.inputKey, x); var Q = u(E.inputKey, { type: "text" }); Q.value = ln(x), _ == null || _(), (Tt = L.stopPropagation) == null || Tt.call(L); }), k_1.on("pointerdown", function (L) { var f; (f = L.stopPropagation) == null || f.call(L); }), x.openYear && g.push({ kind: "year-panel", inputKey: E.inputKey, absX: K, absY: et, anchorW: 196, anchorH: 0 });
                }
                if (E.kind === "week-panel") {
                    var m = E.absX, D = E.absY + E.anchorH;
                    m = Math.max(0, Math.min(m, Math.max(0, i - 280))), D + 192 > o - 4 && (D = E.absY - 192), D = Math.max(0, Math.min(D, Math.max(0, o - 192)));
                    var Y_1 = new xt;
                    Y_1.position.set(m, D), n.addChild(Y_1);
                    var rt = new bt;
                    Ue(rt, r, 280, 192), Y_1.addChild(rt);
                    var ot_1 = { x: 10, y: 10, w: 104, h: 24 }, q_1 = { x: 10 + ot_1.w + 10, y: 10, w: 120, h: 24 }, K = function (H, Z) { var G = new bt; G.rect(H.x, H.y, H.w, H.h), G.fill({ color: 0, alpha: .03 }), G.rect(H.x + .5, H.y + .5, Math.max(0, H.w - 1), Math.max(0, H.h - 1)), G.stroke({ width: 1, color: r.control.border, alignment: 0 }), Y_1.addChild(G); var at = zt({ text: Z, fontFamily: r.fontFamily, fontSize: r.fontSize, fill: r.text, wordWrap: !1 }); at.position.set(H.x + 8, H.y + 4 + _t), Y_1.addChild(at); };
                    K(ot_1, "Month ".concat(x.month)), K(q_1, "Year 20".concat(Kt(x.year2)));
                    var et = 44, k_2 = Ao({ panel: Y_1, theme: r, x: 10, y: et, w: 280 - 10 * 2, st: x, onPick: function () { } }).hitRects;
                    Y_1.eventMode = "static", Y_1.cursor = "pointer", Y_1.hitArea = new gt(0, 0, 280, 192), Y_1.on("pointerdown", function (H) {
                        var e_23, _a;
                        var O, v, T, C, P;
                        if ((H == null ? void 0 : H.button) === 2)
                            return;
                        var Z = c(H);
                        if (Z <= 0)
                            return;
                        I.focusedKeyByPointer.set(Z, E.inputKey), I.keyboardOwnerPointerId = Z;
                        var G = Y_1.toLocal(H.global), at = (O = G == null ? void 0 : G.x) != null ? O : -1, L = (v = G == null ? void 0 : G.y) != null ? v : -1, f = function (A) { return at >= A.x && at <= A.x + A.w && L >= A.y && L <= A.y + A.h; };
                        if (f(ot_1)) {
                            x.openMonthGrid = !x.openMonthGrid, s.set(E.inputKey, x), _ == null || _(), (T = H.stopPropagation) == null || T.call(H);
                            return;
                        }
                        if (f(q_1)) {
                            x.openYear = !0, s.set(E.inputKey, x), _ == null || _(), (C = H.stopPropagation) == null || C.call(H);
                            return;
                        }
                        try {
                            for (var k_3 = (e_23 = void 0, __values(k_2)), k_3_1 = k_3.next(); !k_3_1.done; k_3_1 = k_3.next()) {
                                var A = k_3_1.value;
                                if (f(A)) {
                                    x.weekIndex = A.weekIndex;
                                    var N = u(E.inputKey, { type: "text" });
                                    x.kind === "week" ? N.value = cn(x) : x.kind === "date" ? N.value = ke(x) : N.value = Be(x), x.openPanel = null, x.openYear = !1, x.openMonthGrid = !1, s.set(E.inputKey, x), _ == null || _(), (P = H.stopPropagation) == null || P.call(H);
                                    return;
                                }
                            }
                        }
                        catch (e_23_1) { e_23 = { error: e_23_1 }; }
                        finally {
                            try {
                                if (k_3_1 && !k_3_1.done && (_a = k_3.return)) _a.call(k_3);
                            }
                            finally { if (e_23) throw e_23.error; }
                        }
                    }), x.openMonthGrid && g.push({ kind: "month-grid", inputKey: E.inputKey, absX: m, absY: D + ot_1.y + ot_1.h + 4, anchorW: 0, anchorH: 0 }), x.openYear && g.push({ kind: "year-panel", inputKey: E.inputKey, absX: m + q_1.x, absY: D + q_1.y, anchorW: q_1.w, anchorH: 0 });
                }
                if (E.kind === "time-panel") {
                    var m_1 = E.absX, D_1 = E.absY + E.anchorH;
                    m_1 = Math.max(0, Math.min(m_1, Math.max(0, i - 330))), D_1 + 80 > o - 4 && (D_1 = E.absY - 80), D_1 = Math.max(0, Math.min(D_1, Math.max(0, o - 80)));
                    var Y_2 = new xt;
                    Y_2.position.set(m_1, D_1), n.addChild(Y_2);
                    var rt = new bt;
                    Ue(rt, r, 330, 80), Y_2.addChild(rt);
                    var ot = zt({ text: "Time", fontFamily: r.fontFamily, fontSize: r.fontSize, fill: r.text, wordWrap: !1 });
                    ot.position.set(10, 8 + _t), Y_2.addChild(ot);
                    var q_2 = function (J) { return Array.from({ length: J }, function (z, U) { return Kt(U); }).join("\n"); }, K = E.inputKey, et = "".concat(K, ":time-h"), k = "".concat(K, ":time-m"), H = "".concat(K, ":time-s"), Z = de(d, et, Dt(x.hour, 0, 23)), G = de(d, k, Dt(x.minute, 0, 59)), at = de(d, H, Dt(x.second, 0, 59));
                    Z.selectedIndex = Dt(x.hour, 0, 23), G.selectedIndex = Dt(x.minute, 0, 59), at.selectedIndex = Dt(x.second, 0, 59);
                    var L_1 = 96, f_2 = 36, O_1 = 32, v = 8, T = function (J, z, U) { var Q = new xt; Q.position.set(z, O_1), Y_2.addChild(Q); var nt = new bt; Q.addChild(nt), an({ node: { key: J, attrs: { "data-options": q_2(U), "data-selected-index": String(de(d, J, 0).selectedIndex) } }, container: Q, graphics: nt, w: L_1, h: f_2, absX: m_1 + z, absY: D_1 + O_1, theme: r, selectStates: d, uiState: I, getPointerId: c, getCursorColor: y, requestPaint: _, popupSink: b }); };
                    T(et, 10, 24), T(k, 10 + L_1 + v, 60), T(H, 10 + (L_1 + v) * 2, 60);
                    var C = Dt((S = (M = d.get(et)) == null ? void 0 : M.selectedIndex) != null ? S : x.hour, 0, 23), P = Dt(($ = (B = d.get(k)) == null ? void 0 : B.selectedIndex) != null ? $ : x.minute, 0, 59), A = Dt((R = (w = d.get(H)) == null ? void 0 : w.selectedIndex) != null ? R : x.second, 0, 59);
                    x.hour = C, x.minute = P, x.second = A, s.set(E.inputKey, x);
                    var N = u(E.inputKey, { type: "text" });
                    x.kind === "time" ? N.value = Xe(x) : N.value = Be(x), Y_2.eventMode = "static", Y_2.hitArea = new gt(0, 0, 330, 80), Y_2.on("pointerdown", function (J) { var z; (z = J.stopPropagation) == null || z.call(J); });
                }
            }
        };
        try {
            for (var e_22 = __values(e), e_22_1 = e_22.next(); !e_22_1.done; e_22_1 = e_22.next()) {
                var E = e_22_1.value;
                _loop_1(E);
            }
        }
        catch (e_20_1) { e_20 = { error: e_20_1 }; }
        finally {
            try {
                if (e_22_1 && !e_22_1.done && (_a = e_22.return)) _a.call(e_22);
            }
            finally { if (e_20) throw e_20.error; }
        }
        var _loop_2 = function (E) {
            var x = s.get(E.inputKey);
            x && (E.kind === "month-grid" && Oo({ stage: n, theme: r, popup: E, st: x, viewportW: i, viewportH: o, getPointerId: c, requestPaint: _, onPick: function (W) { x.month = W, x.openMonthGrid = !1, s.set(E.inputKey, x); var j = u(E.inputKey, { type: "text" }); x.kind === "month" ? j.value = ln(x) : x.kind === "week" ? j.value = cn(x) : x.kind === "date" ? j.value = ke(x) : j.value = Be(x); } }), E.kind === "year-panel" && Co({ stage: n, theme: r, popup: E, st: x, viewportW: i, viewportH: o, sliders: a, sliderBounds: h, sliderDrags: p, getPointerId: c, requestPaint: _, onChange: function (W) { x.year2 = W, s.set(E.inputKey, x); var j = u(E.inputKey, { type: "text" }); x.kind === "month" ? j.value = ln(x) : x.kind === "week" ? j.value = cn(x) : x.kind === "date" ? j.value = ke(x) : x.kind === "time" ? j.value = Xe(x) : j.value = Be(x); } }));
        };
        try {
            for (var g_1 = __values(g), g_1_1 = g_1.next(); !g_1_1.done; g_1_1 = g_1.next()) {
                var E = g_1_1.value;
                _loop_2(E);
            }
        }
        catch (e_21_1) { e_21 = { error: e_21_1 }; }
        finally {
            try {
                if (g_1_1 && !g_1_1.done && (_b = g_1.return)) _b.call(g_1);
            }
            finally { if (e_21) throw e_21.error; }
        }
    }
    function mi(t) {
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
    var fi = 5e4, Ye = new WeakMap, gi = new Map, No = 1, bi = 0, Do = 0, pi = !1, we = [], vn = null;
    function Ce(t) { return t instanceof bt ? "Graphics" : t instanceof jt ? "Text" : t instanceof xt ? "Container" : "Object"; }
    function Lo(t) { var e = t && typeof t == "object" ? t.label : void 0, n = t && typeof t == "object" ? Ce(t) : "Object"; return e ? "".concat(n, ":").concat(String(e).slice(0, 80)) : n; }
    function ae(t) { var e = Ye.get(t); return e || (e = No++, Ye.set(t, e)), gi.set(e, t), e; }
    function dn(t) { var e, n, r, i, o, s; if (t == null || typeof t == "number" || typeof t == "string" || typeof t == "boolean")
        return t; if (Array.isArray(t))
        return t.slice(0, 16).map(dn); if (typeof t == "object") {
        var u = t;
        return "color" in u || "alpha" in u || "width" in u && !("x" in u) && !("y" in u) && !("height" in u) ? { color: u.color, alpha: u.alpha, width: u.width } : "x" in u || "y" in u || "width" in u || "height" in u ? { x: Number((e = u.x) != null ? e : 0), y: Number((n = u.y) != null ? n : 0), w: Number((i = (r = u.width) != null ? r : u.w) != null ? i : 0), h: Number((s = (o = u.height) != null ? o : u.h) != null ? s : 0) } : Ce(u);
    } return String(t); }
    function Oe(t, e, n) {
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
            return Ce(t);
        if (n.add(t), Array.isArray(t))
            return t.slice(0, 256).map(function (i) { return Oe(i, e + 1, n); });
        var r = {};
        try {
            for (var _b = __values(Object.entries(t).slice(0, 128)), _c = _b.next(); !_c.done; _c = _b.next()) {
                var _d = __read(_c.value, 2), i = _d[0], o = _d[1];
                r[i] = Oe(o, e + 1, n);
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
    function Hn(t) { if (t != null)
        return typeof t == "symbol" ? t.toString() : String(t); }
    function _i(t) { if (t != null)
        return typeof t == "function" ? { type: "function", name: t.name || void 0, arity: t.length } : typeof t == "object" ? { id: ae(t), type: Ce(t) } : { type: typeof t }; }
    function vo(t) { if (t != null)
        return typeof t == "object" ? { id: ae(t), type: Ce(t) } : typeof t == "function" ? { type: "function" } : { type: typeof t }; }
    function Go(t) { var e = { event: Hn(t[0]), listener: _i(t[1]) }; return t.length > 2 && (e.context = vo(t[2])), [e]; }
    function Ho(t) { return String(t != null ? t : "").slice(0, 240); }
    function $o(t) {
        var e_26, _a;
        var r, i;
        if (!t || typeof t != "object")
            return dn(t);
        var e = t, n = { type: (i = (r = t.constructor) == null ? void 0 : r.name) != null ? i : "object" };
        try {
            for (var _b = __values(["fontFamily", "fontSize", "fontStyle", "fontWeight", "fill", "align", "lineHeight", "letterSpacing", "wordWrap", "wordWrapWidth", "padding"]), _c = _b.next(); !_c.done; _c = _b.next()) {
                var o = _c.value;
                var s = e[o];
                s !== void 0 && (n[o] = dn(s));
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
    function Wo(t) { var s, u, a, h, p, d; if (!t || typeof t != "object")
        return; var e = t, n = Number((s = e.x) != null ? s : 0), r = Number((u = e.y) != null ? u : 0), i = Number((h = (a = e.width) != null ? a : e.w) != null ? h : 0), o = Number((d = (p = e.height) != null ? p : e.h) != null ? d : 0); if (!(!Number.isFinite(n) || !Number.isFinite(r) || !Number.isFinite(i) || !Number.isFinite(o)) && !(i <= 0 || o <= 0))
        return { x: n, y: r, w: i, h: o }; }
    function Fo(t, e) { if (e) {
        if (t === "addChild" || t === "removeChild")
            return e.map(function (n) { return n && typeof n == "object" ? ae(n) : 0; });
        if (t === "mask") {
            var n = e[0];
            return [n && typeof n == "object" ? ae(n) : 0];
        }
        if (t === "addChildAt" || t === "setChildIndex") {
            var n = e[0];
            return [n && typeof n == "object" ? ae(n) : 0, Number(e[1]) || 0];
        }
        return t === "on" ? Go(e) : t === "snapshot" ? e : t === "text.text.set" ? e.length ? [Ho(e[0])] : [] : t === "text.style.set" ? e.length ? [$o(e[0])] : [] : e.map(dn);
    } }
    function hn(t, e, n) { var r, i; try {
        window.__TRUEOS_PIXI_CAPTURE_STEP__ = "record:".concat(e, ":begin");
        var o = window.__pixiCapture;
        if (!(o != null && o.enabled))
            return;
        o.counts[e] = ((r = o.counts[e]) != null ? r : 0) + 1;
        var s = { frame: bi, seq: ++Do, op: e, id: t && typeof t == "object" ? ae(t) : void 0, target: Lo(t), event: e === "on" && (n != null && n.length) ? Hn(n[0]) : void 0, listener: e === "on" && (n != null && n.length) ? _i(n[1]) : void 0, args: Fo(e, n) };
        window.__TRUEOS_PIXI_CAPTURE_STEP__ = "record:".concat(e, ":push"), o.commands.push(s), o.persist && Bo(s), o.commands.length > fi && o.commands.splice(0, o.commands.length - fi), window.__TRUEOS_PIXI_CAPTURE_STEP__ = "record:".concat(e, ":done");
    }
    catch (o) {
        try {
            window.__TRUEOS_PIXI_CAPTURE_ERROR__ = "record:".concat(e, ":").concat(String((i = o == null ? void 0 : o.message) != null ? i : o));
        }
        catch (s) { }
    } }
    function Bo(t) { if (we.push(t), t.op === "snapshot") {
        Ke();
        return;
    } if (we.length >= 512) {
        Ke();
        return;
    } vn == null && (vn = window.setTimeout(function () { vn = null, Ke(); }, 50)); }
    function Ke() {
        if (we.length === 0)
            return;
        var t = we;
        we = [];
        var e = t.map(function (n) { return JSON.stringify(n); }).join("\n") + "\n";
        navigator.sendBeacon && navigator.sendBeacon("/__pixi_capture", new Blob([e], { type: "application/x-ndjson" })) || fetch("/__pixi_capture", { method: "POST", headers: { "Content-Type": "application/x-ndjson" }, body: e, keepalive: !0 }).catch(function () { we = t.concat(we); });
    }
    function Uo(t, e, n) {
        var e_27, _a, e_28, _b, e_29, _c;
        var r, i;
        if (e === "on") {
            var o = Hn(n[0]), s = n[1];
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
                            var u = s.parent.children.indexOf(s);
                            u >= 0 && s.parent.children.splice(u, 1);
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
                var u = o.parent.children.indexOf(o);
                u >= 0 && o.parent.children.splice(u, 1);
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
                    var u = t.children.indexOf(s);
                    u >= 0 && t.children.splice(u, 1), s && (s.parent = null);
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
            var o = Math.max(0, Number((r = n[0]) != null ? r : 0) | 0), s = Array.isArray(t.children) ? t.children.length : o, u = Math.max(o, Math.min(Number((i = n[1]) != null ? i : s) | 0, s)), a = t.children.splice(o, u - o);
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
            var u = Math.max(0, Math.min(Number(n[1]) | 0, t.children.length));
            t.children.splice(u, 0, o);
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
    function Xo() { var t = function () { return !!(window.__TRUEOS_PIXI_REPAINT_REQUIRED__ || window.__TRUEOS_PIXI_SCROLL_REPAINT_REQUIRED__); }; window.__TRUEOS_DISPATCH_PIXI_POINTER__ = function (e, n, r, i, o, s, u) {
        var e_30, _a;
        if (u === void 0) { u = 0; }
        var S, B, $, w, R, E, x, W, j, V, m, D, Y, rt, ot, q, K, et, k, H, Z;
        var a = function (G) { try {
            window.__TRUEOS_PIXI_POINTER_DISPATCH_STEP__ = G, typeof console != "undefined" && typeof console.log == "function" && console.log("[trueos pointer dispatch] ".concat(G));
        }
        catch (at) { } };
        a("start node=".concat(Number(e) || 0, " event=").concat(String(n || "")));
        var h = window.__TRUEOS_PIXI_APP;
        if (String(n || "") === "wheel") {
            var G = h == null ? void 0 : h.canvas;
            if (!G || typeof G.dispatchEvent != "function")
                return a("wheel-canvas-missing"), { handled: 0, listenerCount: 0, painted: 0, targetFound: 0 };
            var at = ($ = (B = (S = window.__pixiCapture) == null ? void 0 : S.commands) == null ? void 0 : B.length) != null ? $ : 0, L = { type: "wheel", deltaX: 0, deltaY: Number(u) || 0, deltaMode: 0, offsetX: Number(r) || 0, offsetY: Number(i) || 0, clientX: Number(r) || 0, clientY: Number(i) || 0, pointerId: Number(o) || 1, buttons: Number(s) || 0, defaultPrevented: !1, propagationStopped: !1, preventDefault: function () { this.defaultPrevented = !0; }, stopPropagation: function () { this.propagationStopped = !0; } };
            a("wheel-dispatch deltaY=".concat(L.deltaY)), G.dispatchEvent(L);
            var f = 0;
            if (window.__TRUEOS_CAPTURE_ONLY__) {
                var A = window.__TRUEOS_REPAINT_NOW__;
                t() && typeof A == "function" && (a("wheel-repaint-call"), A(), a("wheel-repaint-return"), f = 1);
            }
            else
                (w = h == null ? void 0 : h.renderer) != null && w.render && (h != null && h.stage) && (h.renderer.render(h.stage), f = 1);
            var O = (x = (E = (R = window.__pixiCapture) == null ? void 0 : R.commands) == null ? void 0 : E.length) != null ? x : at, v = (W = G.listeners) == null ? void 0 : W.wheel, T = Array.isArray(v) ? v.length : typeof v == "function" ? 1 : 0, C = L.defaultPrevented || T > 0 ? 1 : 0;
            a("wheel-done handled=".concat(C, " listeners=").concat(T, " painted=").concat(f));
            var P = window.__TRUEOS_PIXI_LAST_SCROLL_FAST_PATH__;
            return (P == null ? void 0 : P.owner) === "root" || (P == null ? void 0 : P.owner) === "iframe" ? { handled: C, listenerCount: T, painted: 1, targetFound: 1, scrollFastPath: 1, rootNode: Number(P.rootNode) || 0, contentNode: Number(P.contentNode) || 0, contentY: Number(P.contentY) || 0, scrollbarNode: Number(P.scrollbarNode) || 0, scrollbarVisible: Number(P.scrollbarVisible) || 0, trackX: Number(P.trackX) || 0, trackY: Number(P.trackY) || 0, trackW: Number(P.trackW) || 0, trackH: Number(P.trackH) || 0, thumbX: Number(P.thumbX) || 0, thumbY: Number(P.thumbY) || 0, thumbW: Number(P.thumbW) || 0, thumbH: Number(P.thumbH) || 0 } : { handled: C, listenerCount: T, painted: O > at || f ? 1 : 0, targetFound: 1 };
        }
        var p = gi.get(Number(e) || 0), d = 0, b = 0, y = 0;
        if (!p)
            return a("target-missing"), { handled: d, listenerCount: b, painted: y, targetFound: 0 };
        window.__TRUEOS_PIXI_LAST_GRAPHICS_FAST_PATH__ = null;
        var I = { type: String(n || ""), button: Number(s) & 2 ? 2 : 0, buttons: Number(s) || 0, pointerId: Number(o) || 1, pointerType: "mouse", global: { x: Number(r) || 0, y: Number(i) || 0 }, data: { pointerId: Number(o) || 1, pointerType: "mouse", global: { x: Number(r) || 0, y: Number(i) || 0 } }, target: p, currentTarget: p, defaultPrevented: !1, propagationStopped: !1, preventDefault: function () { this.defaultPrevented = !0; }, stopPropagation: function () { this.propagationStopped = !0; } }, c = (m = (V = (j = window.__pixiCapture) == null ? void 0 : j.commands) == null ? void 0 : V.length) != null ? m : 0;
        a("target-found label=".concat(String((D = p.label) != null ? D : "")));
        for (var G = p; G; G = G.parent) {
            I.currentTarget = G;
            var at = (Y = G.listeners) == null ? void 0 : Y[I.type];
            if (!(!Array.isArray(at) || at.length === 0)) {
                b += at.length, a("listeners node=".concat((rt = Ye.get(G)) != null ? rt : 0, " count=").concat(at.length));
                try {
                    for (var _b = (e_30 = void 0, __values(at.slice())), _c = _b.next(); !_c.done; _c = _b.next()) {
                        var L = _c.value;
                        if (typeof L == "function" && (d = 1, a("listener-call node=".concat((ot = Ye.get(G)) != null ? ot : 0)), L.call(G, I), a("listener-return node=".concat((q = Ye.get(G)) != null ? q : 0)), I.propagationStopped))
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
                if (I.propagationStopped)
                    break;
            }
        }
        if (window.__TRUEOS_CAPTURE_ONLY__) {
            var G = window.__TRUEOS_REPAINT_NOW__;
            t() && typeof G == "function" && (a("capture-repaint-call"), G(), a("capture-repaint-return"), y = 1);
        }
        else
            (K = h == null ? void 0 : h.renderer) != null && K.render && (h != null && h.stage) && (a("paint-call"), h.renderer.render(h.stage), a("paint-return"), y = 1);
        var _ = (H = (k = (et = window.__pixiCapture) == null ? void 0 : et.commands) == null ? void 0 : k.length) != null ? H : c, g = window.__TRUEOS_PIXI_LAST_SCROLL_FAST_PATH__;
        if ((g == null ? void 0 : g.owner) === "root" || (g == null ? void 0 : g.owner) === "iframe")
            return a("scroll-fast owner=".concat(g.owner)), { handled: d, listenerCount: b, painted: 1, targetFound: 1, scrollFastPath: 1, rootNode: Number(g.rootNode) || 0, contentNode: Number(g.contentNode) || 0, contentY: Number(g.contentY) || 0, scrollbarNode: Number(g.scrollbarNode) || 0, scrollbarVisible: Number(g.scrollbarVisible) || 0, trackX: Number(g.trackX) || 0, trackY: Number(g.trackY) || 0, trackW: Number(g.trackW) || 0, trackH: Number(g.trackH) || 0, thumbX: Number(g.thumbX) || 0, thumbY: Number(g.thumbY) || 0, thumbW: Number(g.thumbW) || 0, thumbH: Number(g.thumbH) || 0 };
        var M = window.__TRUEOS_PIXI_LAST_GRAPHICS_FAST_PATH__;
        return (M == null ? void 0 : M.owner) === "context-menu-hover" && (I.type === "pointerover" || I.type === "pointerout") && _ > c ? ((Z = window.__pixiCapture) != null && Z.commands && window.__pixiCapture.commands.splice(c, _ - c), a("graphics-fast owner=".concat(M.owner)), { handled: d, listenerCount: b, painted: 1, targetFound: 1, graphicsFastPath: 1, rootNode: Number(M.rootNode) || 0, graphicsNode: Number(M.graphicsNode) || 0, rectX: Number(M.x) || 0, rectY: Number(M.y) || 0, rectW: Number(M.w) || 0, rectH: Number(M.h) || 0, fillColor: Number(M.fillColor) || 0, fillAlpha: Number(M.fillAlpha) || 0 }) : (y = _ > c || y ? 1 : 0, a("done handled=".concat(d, " listeners=").concat(b, " painted=").concat(y)), { handled: d, listenerCount: b, painted: y, targetFound: 1 });
    }; }
    function Gn(t, e, n) {
        if (n === void 0) { n = e; }
        var r = t == null ? void 0 : t[e];
        if (typeof r != "function" || r.__pixiCapturePatched)
            return;
        var i = function () {
            var s = [];
            for (var _a = 0; _a < arguments.length; _a++) {
                s[_a] = arguments[_a];
            }
            var u;
            if (hn(this, n, s), !window.__TRUEOS_CAPTURE_ONLY__)
                return r.apply(this, s);
            try {
                window.__TRUEOS_PIXI_CAPTURE_STEP__ = "invoke:".concat(n, ":begin");
                var a = Uo(this, n, s);
                return window.__TRUEOS_PIXI_CAPTURE_STEP__ = "invoke:".concat(n, ":done"), a;
            }
            catch (a) {
                try {
                    window.__TRUEOS_PIXI_CAPTURE_ERROR__ = "invoke:".concat(n, ":").concat(String((u = a == null ? void 0 : a.message) != null ? u : a));
                }
                catch (h) { }
                return n === "addChild" || n === "addChildAt" || n === "removeChild" ? s[0] : this;
            }
        };
        i.__pixiCapturePatched = !0, t[e] = i;
    }
    function Yo(t, e) { var n = t; for (; n;) {
        var r = Object.getOwnPropertyDescriptor(n, e);
        if (r)
            return r;
        n = Object.getPrototypeOf(n);
    } }
    function Re(t, e, n) { var o, s; if (!(t != null && t.constructor) || t.constructor["__pixiCapturePatched_".concat(n)])
        return; var r = Yo(t, e); if ((r == null ? void 0 : r.configurable) === !1 || r && !r.set && !r.writable)
        return; var i = typeof Symbol == "function" ? Symbol("pixiCapture:".concat(n)) : "__pixiCaptureValue_".concat(n); Object.defineProperty(t, e, { configurable: (o = r == null ? void 0 : r.configurable) != null ? o : !0, enumerable: (s = r == null ? void 0 : r.enumerable) != null ? s : !0, get: r != null && r.get ? function () { var a; return (a = r.get) == null ? void 0 : a.call(this); } : function () { var a = this; return Object.prototype.hasOwnProperty.call(a, i) ? a[i] : r && "value" in r ? r.value : void 0; }, set: function (a) { if (hn(this, n, [a]), !window.__TRUEOS_CAPTURE_ONLY__) {
            r != null && r.set ? r.set.call(this, a) : Object.defineProperty(this, i, { configurable: !0, enumerable: !1, writable: !0, value: a });
            return;
        } var h = this; n === "text.text.set" ? h._text = String(a != null ? a : "") : n === "text.style.set" ? h._style = a != null ? a : {} : n === "text.resolution.set" ? h._resolution = Math.max(1, Number(a) || 1) : Object.defineProperty(h, i, { configurable: !0, enumerable: !1, writable: !0, value: a }); } }), t.constructor["__pixiCapturePatched_".concat(n)] = !0; }
    function yi(t, e) {
        if (e === void 0) { e = 0; }
        var s, u, a, h, p, d, b, y, I;
        if (!t || e > 64)
            return null;
        var n, r;
        try {
            var c = typeof t.getGlobalPosition == "function" ? t.getGlobalPosition() : null;
            c && Number.isFinite(Number(c.x)) && Number.isFinite(Number(c.y)) && (n = Number(c.x), r = Number(c.y));
        }
        catch (c) { }
        var i = { id: ae(t), type: Ce(t), label: (s = t.label) != null ? s : void 0, x: (h = (a = (u = t.position) == null ? void 0 : u.x) != null ? a : t.x) != null ? h : 0, y: (b = (d = (p = t.position) == null ? void 0 : p.y) != null ? d : t.y) != null ? b : 0, globalX: n, globalY: r, scaleX: Number.isFinite(Number((y = t.scale) == null ? void 0 : y.x)) ? Number(t.scale.x) : 1, scaleY: Number.isFinite(Number((I = t.scale) == null ? void 0 : I.y)) ? Number(t.scale.y) : 1, visible: t.visible, alpha: Number.isFinite(Number(t.alpha)) ? Number(t.alpha) : 1, maskId: t.mask ? ae(t.mask) : 0, zIndex: Number(t.zIndex) || 0, sortableChildren: t.sortableChildren === !0 }, o = Wo(t.hitArea);
        if (o && (i.hitArea = o), t.listeners && typeof t.listeners == "object") {
            var c = Object.keys(t.listeners).filter(function (_) { var M; var g = (M = t.listeners) == null ? void 0 : M[_]; return Array.isArray(g) && g.length > 0; });
            c.length > 0 && (i.listeners = c.slice(0, 16));
        }
        if (t instanceof bt && Array.isArray(t.commands) && t.commands.length > 0 && (i.commands = t.commands.slice(-256).map(function (c) { return Oe(c, 0); })), typeof t.text == "string" && (i.text = t.text.slice(0, 120), t instanceof jt && t.style && typeof t.style == "object")) {
            var c = {}, _ = t.style;
            typeof _.fontSize != "undefined" && (c.fontSize = Oe(_.fontSize, 0)), typeof _.fontWeight != "undefined" && (c.fontWeight = Oe(_.fontWeight, 0)), typeof _.fill != "undefined" && (c.fill = Oe(_.fill, 0)), Object.keys(c).length > 0 && (i.textStyle = c);
        }
        return Array.isArray(t.children) && t.children.length && (i.children = t.children.map(function (c) { return yi(c, e + 1); })), i;
    }
    function xi() {
        var e_31, _a, e_32, _b;
        if (window.__pixiCapture)
            return window.__pixiCapture;
        var t = { enabled: !0, persist: !window.__TRUEOS_CAPTURE_ONLY__, commands: [], counts: Object.create(null), objectId: function (e) { return ae(e); }, clear: function () { this.commands.length = 0, this.counts = Object.create(null); }, dump: function (e) {
                if (e === void 0) { e = 200; }
                return this.commands.slice(-e);
            }, flush: function () { Ke(); }, summary: function () { return te({}, this.counts); } };
        if (window.__pixiCapture = t, Xo(), window.addEventListener("beforeunload", function () { return Ke(); }), !pi) {
            pi = !0, typeof bt.prototype.image != "function" && (bt.prototype.image = function () { return this; });
            try {
                for (var _c = __values(["clear", "rect", "roundRect", "circle", "ellipse", "moveTo", "lineTo", "closePath", "poly", "fill", "stroke", "image", "svg"]), _d = _c.next(); !_d.done; _d = _c.next()) {
                    var e = _d.value;
                    Gn(bt.prototype, e);
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
                    Gn(xt.prototype, e);
                }
            }
            catch (e_32_1) { e_32 = { error: e_32_1 }; }
            finally {
                try {
                    if (_g && !_g.done && (_b = _f.return)) _b.call(_f);
                }
                finally { if (e_32) throw e_32.error; }
            }
            Re(jt.prototype, "text", "text.text.set"), Re(jt.prototype, "style", "text.style.set"), Re(jt.prototype, "resolution", "text.resolution.set"), Gn(jt.prototype, "setSize", "text.setSize"), Re(xt.prototype, "visible", "visible"), Re(xt.prototype, "alpha", "alpha"), Re(xt.prototype, "mask", "mask");
        }
        return t;
    }
    function wi(t) { var e = t.renderer, n = e == null ? void 0 : e.render; if (typeof n != "function" || n.__pixiCapturePatched)
        return; var r = function (o) { var u; var s = o && typeof o == "object" && "container" in o ? o.container : o || t.stage; return bi++, window.__TRUEOS_CAPTURE_ONLY__ && ((u = window.__pixiCapture) == null || u.clear()), hn(s, "render", []), hn(s, "snapshot", [yi(s)]), window.__TRUEOS_CAPTURE_ONLY__ ? s : n.call(this, o); }; r.__pixiCapturePatched = !0, e.render = r; }
    xi();
    var st = null, gn = 6, Te = 10, Ft = 1, Ut = 3, Yt = 4, Ae = 512, Ci = new Map;
    var l = { focusedKeyByPointer: new Map, keyboardOwnerPointerId: 1, inputs: new Map, sliders: new Map, sliderDrags: new Map, sliderBounds: new Map, dialogs: new Map, dialogDrags: new Map, dialogSelectedBy: new Map, dialogZ: new Map, dialogZCounter: 1, numbers: new Map, numberHolds: new Map, selects: new Map, temporals: new Map, temporalYearOwners: new Map, color: { rgb: { r: 255, g: 0, b: 0 }, a: 255, pick: null, draggingPointerId: null, bounds: null }, cursorColors: new Map, primaryMousePointerId: 1, harness: { enabled: !0, activeUserPointerId: Ft, periodMs: 3e3 }, userCursorPos: new Map, lastMouse: { x: 0, y: 0, has: !1 }, scroll: { y: 0, contentHeight: 0, viewportHeight: 0, draggingPointerId: null, dragOffsetY: 0, track: { x: 0, y: 0, w: Te, h: 0 }, thumb: { x: 0, y: 0, w: Te, h: 0 } }, iframeScroll: new Map, iframeScrollRoots: new Map, iframeScrollbarGraphics: new Map, iframeRects: [], hoverRects: [], hoverHandlers: new Map, hoveredKeyByPointer: new Map, hoveredCursorByPointer: new Map, virtualCursor: { enabled: !1, x: 0, y: 0, t: 0, radius: 120, speed: .9 }, textDrags: new Map, fieldBounds: new Map, dialogDragBounds: new Map, detailsOpen: new Map, contextMenus: new Map, clipboards: new Map }, mn = null, Bn = 0;
    function zo(t) { if (!mn) {
        var n = document.createElement("canvas").getContext("2d");
        if (!n)
            throw new Error("2D canvas not available");
        mn = n;
    } return mn.font = "".concat(t.fontSize, "px ").concat(t.fontFamily), function (e) { return (Bn += 1, mn.measureText(e).width); }; }
    function Wn(t, e) {
        if (e === void 0) { e = 16; }
        return Object.entries(t).sort(function (n, r) { return r[1] - n[1] || (n[0] < r[0] ? -1 : n[0] > r[0] ? 1 : 0); }).slice(0, e).map(function (_a) {
            var _b = __read(_a, 2), n = _b[0], r = _b[1];
            return "".concat(n, ":").concat(r);
        }).join(",");
    }
    function $n(t) { var e = (2166136261 ^ t.length) >>> 0, n = function (o, s) { for (var u = o; u < s; u += 1) {
        var a = t.charCodeAt(u);
        e = e + (a & 65535) >>> 0, e = e + (e << 10) >>> 0, e ^= e >>> 6;
    } }, r = t.length, i = 4096; if (r <= i * 3)
        n(0, r);
    else {
        n(0, i);
        var o = Math.max(i, Math.floor((r - i) / 2));
        n(o, Math.min(r, o + i)), n(Math.max(0, r - i), r);
    } return e = e + (e << 3) >>> 0, e ^= e >>> 11, e = e + (e << 15) >>> 0, "0x".concat(e.toString(16).padStart(8, "0")); }
    function Ai(t) {
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
    function Ni(t) { return t.kind === "text" ? { kind: "text", text: t.text } : { kind: "block", key: t.key, tagName: t.tagName, attrs: Ai(t.attrs), children: t.children.map(Ni) }; }
    function Di(t) { var e, n, r; return t.kind === "text" ? { kind: "text", text: (e = t.text) != null ? e : "", x: t.x, y: t.y, width: t.width, height: t.height, children: [] } : { kind: "block", key: (n = t.key) != null ? n : "", tagName: (r = t.tagName) != null ? r : "", attrs: Ai(t.attrs), x: t.x, y: t.y, width: t.width, height: t.height, children: t.children.map(Di) }; }
    function jo(t, e, n, r, i) {
        Mt("[trueos pixi widgets] prepixi stage=canonical-render begin");
        var o = e.map(Ni);
        Mt("[trueos pixi widgets] prepixi stage=canonical-render done"), Mt("[trueos pixi widgets] prepixi stage=canonical-layout begin");
        var s = Di(n);
        Mt("[trueos pixi widgets] prepixi stage=canonical-layout done"), Mt("[trueos pixi widgets] prepixi stage=stringify begin");
        var u = JSON.stringify(o), a = JSON.stringify(s);
        Mt("[trueos pixi widgets] prepixi stage=stringify done render_bytes=".concat(u.length, " layout_bytes=").concat(a.length)), Mt("[trueos pixi widgets] prepixi stage=hash begin");
        var h = $n(u), p = $n(a), d = $n("".concat(u, "\n").concat(a));
        Mt("[trueos pixi widgets] prepixi stage=hash done"), Mt("[trueos pixi widgets] prepixi stage=trace-stringify begin");
        var b = JSON.stringify({ version: 1, source: t, viewport: { width: r, height: i }, renderHash: h, layoutHash: p, hash: d, renderNodes: o, layout: s });
        return Mt("[trueos pixi widgets] prepixi stage=trace-stringify done bytes=".concat(b.length)), window.__TRUEOS_PIXI_PREPIX_TRACE__ = b, window.__TRUEOS_PIXI_PREPIX_HASH__ = d, window.__TRUEOS_PIXI_PREPIX_RENDER_HASH__ = h, window.__TRUEOS_PIXI_PREPIX_LAYOUT_HASH__ = p, Pt() && console.log("[trueos pixi widgets] prepixi source=".concat(t, " hash=").concat(d, " render_hash=").concat(h, " layout_hash=").concat(p, " bytes=").concat(b.length)), { hash: d, renderHash: h, layoutHash: p, bytes: b.length };
    }
    function Ne(t) { var e = typeof t == "string" ? t : ""; return e.indexOf("<truesurfer-") >= 0 && (e = e.replace(/<truesurfer-[A-Za-z0-9._-]+>/g, "")), e; }
    function Vo(t, e) { if (e >= t.length)
        return !0; var n = t.charCodeAt(e); return n === 95 || n === 40 || n === 91 || n === 123 || n === 34 || n === 39 || n >= 48 && n <= 57 || n >= 65 && n <= 90; }
    function Li(t) { var e = t, n = !0; for (; n;) {
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
        r >= 2 && Vo(e, r) && (e = e.slice(r), n = !0);
    } return e; }
    function Jo(t) { var e = Ne(t), n = e.indexOf("__trueos") >= 0 || e.indexOf("tsNu") >= 0 || e.indexOf("tsNum") >= 0; return e.indexOf("__TRUEOS_HOST_READY__") >= 0 && (e = e.replace(/__TRUEOS_HOST_READY__/g, "")), e.indexOf("__trueos") >= 0 && (e = Qo(e), e = e.replace(/__trueosNumberValue/g, "").replace(/__trueosHostNum/g, "").replace(/__trueosNum/g, "").replace(/__trueosNu/g, "").replace(/__trueos/g, "")), (e.indexOf("tsNu") >= 0 || e.indexOf("tsNum") >= 0) && (e = e.replace(/tsNum/g, "").replace(/tsNutsNutsNutsNu/g, "").replace(/tsNutsNutsNu/g, "").replace(/tsNutsNu/g, "").replace(/tsNu/g, "")), n && (e = Li(e.trimStart())), e; }
    function Qo(t) { var e = "__trueosN", n = t, r = 0; for (; r < n.length;) {
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
    function vi(t) { return Jo(t); }
    function Un(t) { return Li(vi(t).trimStart()); }
    function Zo(t) { var e = he(Un(t)); return !(e.length === 0 || e === "true" || e === "false" || e === "N" || e === "Nu" || e === "Num" || e.startsWith("<truesurfer-") || e.startsWith("__trueo")); }
    function Gi(t, e) { var r; var n = Ne(e) || "block"; t[n] = ((r = t[n]) != null ? r : 0) + 1; }
    function qo(t) {
        var e_34, _a;
        var e = { nodes: 0, blocks: 0, text: 0, maxDepth: 0, tags: {} }, n = function (r, i) {
            var e_35, _a;
            if (e.nodes += 1, e.maxDepth = Math.max(e.maxDepth, i), r.kind === "text") {
                e.text += 1;
                return;
            }
            e.blocks += 1, Gi(e.tags, r.tagName);
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
    function ts(t) { var e = { nodes: 0, blocks: 0, text: 0, maxDepth: 0, tags: {} }, n = function (r, i) {
        var e_36, _a;
        var o;
        e.nodes += 1, e.maxDepth = Math.max(e.maxDepth, i), r.kind === "text" ? e.text += 1 : (e.blocks += 1, Gi(e.tags, (o = r.tagName) != null ? o : "block"));
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
    function wn(t, e) {
        if (e === void 0) { e = 64; }
        var n = he(vi(t)), r = "";
        for (var i = 0; i < n.length && r.length < e; i += 1) {
            var o = n.charAt(i);
            r += o === "|" || o === '"' || o === "\\" ? "_" : o;
        }
        return r;
    }
    function bn(t, e) {
        if (e === void 0) { e = 120; }
        var n = "";
        for (var r = 0; r < t.length && n.length < e; r += 1) {
            var i = t.charAt(r);
            n += i === "\r" || i === "\n" || i === "	" || i === "|" || i === '"' || i === "\\" ? "_" : i;
        }
        return n;
    }
    function es(t) { if (t.length <= 0 || t.length > 1e6 || t.indexOf("\0") >= 0)
        return !1; var e = t.slice(0, 256).trimStart().toLowerCase(); return e.startsWith("<!doctype") || e.startsWith("<html") || e.startsWith("<body") || e.startsWith("<"); }
    function Ii(t, e) {
        if (e === void 0) { e = 12; }
        var n = [], r = function (i, o, s) { if (n.length >= e)
            return; if (i.kind === "text") {
            n.push("#".concat(n.length, "@").concat(o, ":").concat(s, " chars=").concat(i.text.length, " sample=\"").concat(wn(i.text), "\""));
            return;
        } var u = Ne(i.tagName || "block") || "block", a = i.key || ""; for (var h = 0; h < i.children.length; h += 1)
            r(i.children[h], u, a); };
        for (var i = 0; i < t.length; i += 1)
            r(t[i], "root", "");
        return n.join("|");
    }
    function Mi(t, e) {
        if (e === void 0) { e = 12; }
        var n = [], r = function (i, o, s) { var h; if (n.length >= e)
            return; if (i.kind === "text") {
            var p = (h = i.text) != null ? h : "";
            n.push("#".concat(n.length, "@").concat(o, ":").concat(s, " chars=").concat(p.length, " box=").concat(Math.round(i.x), ",").concat(Math.round(i.y), ",").concat(Math.round(i.width), ",").concat(Math.round(i.height), " sample=\"").concat(wn(p), "\""));
            return;
        } var u = Ne(i.tagName || "block") || "block", a = i.key || ""; for (var p = 0; p < i.children.length; p += 1)
            r(i.children[p], u, a); };
        return r(t, "root", ""), n.join("|");
    }
    function Si(t, e) {
        if (e === void 0) { e = 24; }
        var n = [], r = new Set(["label", "input", "timeinput", "dateinput", "monthinput", "weekinput", "datetimelocalinput", "button", "select", "searchrow", "searchbutton"]), i = function (o, s, u, a) {
            var e_37, _a;
            var d;
            if (n.length >= e)
                return;
            var h = u + o.x, p = a + o.y;
            if (o.kind === "block") {
                var b = Ne(o.tagName || "block") || "block";
                if (r.has(b)) {
                    var y = wn(Tn(o), 36);
                    n.push("#".concat(n.length, "@").concat(s, ">").concat(b, ":").concat((d = o.key) != null ? d : "", " box=").concat(Math.round(h), ",").concat(Math.round(p), ",").concat(Math.round(o.width), ",").concat(Math.round(o.height), " text=\"").concat(y, "\""));
                }
                try {
                    for (var _b = __values(o.children), _c = _b.next(); !_c.done; _c = _b.next()) {
                        var y = _c.value;
                        i(y, b, h, p);
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
    function _n(t) { return (typeof t == "string" ? t : "").replace(/&quot;/g, '"').replace(/&#34;/g, '"').replace(/&#39;/g, "'").replace(/&apos;/g, "'").replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&amp;/g, "&"); }
    function Fn(t) { return he(_n((typeof t == "string" ? t : "").replace(/<[^>]*>/g, " "))); }
    function ns(t) { var e = 0, n = String(t != null ? t : ""); for (; e < n.length && n.charCodeAt(e) <= 32;)
        e += 1; for (n.charAt(e) === "/" && (e += 1); e < n.length && n.charCodeAt(e) <= 32;)
        e += 1; var r = e; for (; e < n.length;) {
        var i = n.charCodeAt(e);
        if (!(i >= 48 && i <= 57 || i >= 65 && i <= 90 || i >= 97 && i <= 122 || i === 45 || i === 58))
            break;
        e += 1;
    } return n.slice(r, e).toLowerCase(); }
    function rs(t) { return t === "h1" || t === "h2" || t === "h3" || t === "h4" || t === "h5" || t === "h6" || t === "summary" || t === "p" || t === "button" || t === "label" || t === "legend" || t === "option"; }
    function Hi(t) { var e = typeof t == "string" ? t : "", n = [], r = function (p) { var d = Fn(p); d.length !== 0 && (d.startsWith("<truesurfer-") || d.startsWith("__trueo") || n.push(d)); }, i = [], o = e.toLowerCase(), s = o.indexOf("<body"); if (s >= 0) {
        var p = e.indexOf(">", s);
        s = p >= 0 ? p + 1 : s;
    }
    else
        s = 0; var u = o.indexOf("</body>", s), a = u >= 0 ? u : e.length, h = ""; for (; s < a && n.length < Ae;) {
        var p = e.charAt(s);
        if (p !== "<") {
            h += p, s += 1;
            continue;
        }
        var d = _n(h);
        if (d.length > 0) {
            for (var S = i.length - 1; S >= 0; S -= 1)
                if (i[S].wanted) {
                    i[S].text += " ".concat(d);
                    break;
                }
        }
        h = "";
        var b = e.indexOf(">", s + 1);
        if (b < 0)
            break;
        var y = e.slice(s, b + 1), I = e.slice(s + 1, b), c = ns(I);
        if (I.trimStart().charAt(0) === "/") {
            for (var S = i.length - 1; S >= 0; S -= 1) {
                var B = i.pop();
                if (B != null && B.wanted && r(B.text), (B == null ? void 0 : B.tag) === c)
                    break;
            }
            s = b + 1;
            continue;
        }
        if (c === "script" || c === "style" || c === "template") {
            var S = "</".concat(c, ">"), B = o.indexOf(S, b + 1);
            s = B >= 0 ? B + S.length : b + 1;
            continue;
        }
        if (c === "input") {
            var S = Pi(y, "type").toLowerCase();
            (S === "button" || S === "submit" || S === "reset") && r(Pi(y, "value"));
        }
        var g = y.length - 1;
        for (; g >= 0 && y.charCodeAt(g) <= 32;)
            g -= 1;
        g >= 1 && y.charAt(g) === ">" && y.charAt(g - 1) === "/" || c === "input" || c === "br" || c === "hr" || c === "img" || i.push({ tag: c, wanted: rs(c), text: "" }), s = b + 1;
    } if (h.length > 0) {
        var p = _n(h);
        for (var d = i.length - 1; d >= 0; d -= 1)
            if (i[d].wanted) {
                i[d].text += " ".concat(p);
                break;
            }
    } for (; i.length && n.length < Ae;) {
        var p = i.pop();
        p != null && p.wanted && r(p.text);
    } if (n.length === 0) {
        var p = o.indexOf("<body");
        if (p >= 0) {
            var c = e.indexOf(">", p);
            p = c >= 0 ? c + 1 : p;
        }
        else
            p = 0;
        var d = o.indexOf("</body>", p), b = d >= 0 ? d : e.length, y = !1, I = "";
        for (var c = p; c < b && n.length < Ae; c += 1) {
            var _ = e.charAt(c);
            if (_ === "<") {
                r(I), I = "", y = !0;
                continue;
            }
            if (_ === ">") {
                y = !1;
                continue;
            }
            y || (I += _);
        }
        r(I);
    } return n; }
    function yn(t) { var e = window == null ? void 0 : window[t]; return e !== void 0 ? e : globalThis == null ? void 0 : globalThis[t]; }
    function is(t, e) {
        if (e === void 0) { e = 8; }
        var n = [];
        for (var r = 0; r < t.length && n.length < e; r += 1)
            n.push("#".concat(r, "=\"").concat(bn(t[r], 48), "\""));
        return n.join("|");
    }
    function Pi(t, e) { var i, o, s; var r = new RegExp("".concat(e, "[ \\t\\r\\n\\f]*=[ \\t\\r\\n\\f]*(\"([^\"]*)\"|'([^']*)'|([^ \\t\\r\\n\\f>]+))"), "i").exec(t); return _n((s = (o = (i = r == null ? void 0 : r[2]) != null ? i : r == null ? void 0 : r[3]) != null ? o : r == null ? void 0 : r[4]) != null ? s : ""); }
    function je(t) { var e = []; for (var n = 0; n < t.length; n += 1) {
        var r = t[n];
        typeof r == "string" && xn(e, r);
    } return e; }
    function os(t) { var e = "", n = !1; for (var r = 0; r < t.length; r += 1) {
        var i = t.charCodeAt(r);
        if (i === 32 || i === 9 || i === 10 || i === 13 || i === 12) {
            n = !0;
            continue;
        }
        n && e.length > 0 && (e += " "), e += t.charAt(r), n = !1;
    } return e; }
    function xn(t, e) { var n = os(e); if (n.length !== 0 && !(n.indexOf("<truesurfer-") === 0 || n.indexOf("__trueo") === 0)) {
        for (var r = 0; r < t.length; r += 1)
            if (t[r] === n)
                return;
        t.push(n);
    } }
    function ss(t) {
        if (typeof t != "string" || t.length === 0)
            return [];
        var e = [], n = "";
        for (var r = 0; r < t.length; r += 1) {
            var i = t.charAt(r);
            if (i === "\r" || i === "\n") {
                xn(e, n), n = "", i === "\r" && t.charAt(r + 1) === "\n" && (r += 1);
                continue;
            }
            n += i;
        }
        return xn(e, n), e;
    }
    function as(t) { var e = []; if (!Array.isArray(t))
        return e; for (var n = 0; n < t.length; n += 1) {
        var r = t[n];
        typeof r == "string" && xn(e, r);
    } return e; }
    function ls(t) { var e = []; if (!Array.isArray(t))
        return e; for (var n = 0; n < t.length; n += 1) {
        var r = t[n];
        typeof r != "string" || r.length === 0 || r.indexOf("<truesurfer-") === 0 || r.indexOf("__trueo") === 0 || (e[e.length] = r);
    } return e; }
    function cs(t) {
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
    function us(t) { var e = yn("__TRUEOS_WIDGET_TEXT_ROWS_TEXT__"), n = yn("__TRUEOS_WIDGET_TEXT_ROWS__"), r = ls(n); if (r.length > 0)
        return { source: "array-trusted", rows: r }; var i = cs(e); if (i.length > 0)
        return { source: "text-trusted", rows: i }; var o = ss(e); if (o.length > 0)
        return { source: "text", rows: o }; var s = as(n); if (s.length > 0)
        return { source: "array", rows: s }; var u = Hi(t); if (Pt()) {
        var a = Array.isArray(n) && typeof n[0] == "string" ? bn(n[0], 72) : "", h = typeof e == "string" ? bn(e, 72) : "";
        console.log("[trueos pixi widgets] text-fallback-globals text_type=".concat(typeof e, " text_len=").concat(typeof e == "string" ? e.length : 0, " text_rows=").concat(o.length, " text_sample=\"").concat(h, "\" array=").concat(Array.isArray(n) ? n.length : -1, " array_rows=").concat(s.length, " array0=\"").concat(a, "\" html_len=").concat(t.length, " html_rows=").concat(u.length));
    } return { source: "html", rows: u }; }
    function ds() { var e; var t = yn("__TRUEOS_WIDGET_RENDER_TREE_JSON__"); if (typeof t == "string" && t.length > 0)
        try {
            return { source: "json", tree: JSON.parse(t) };
        }
        catch (n) {
            Pt() && console.log("[trueos pixi widgets] render-tree-json parse failed err=".concat(String((e = n == null ? void 0 : n.message) != null ? e : n)));
        } return { source: "window", tree: yn("__TRUEOS_WIDGET_RENDER_TREE__") }; }
    function hs(t) { var o, s, u, a; var e = [], n = String(t != null ? t : "").replace(/<script[^]*?<\/script>/gi, " ").replace(/<style[^]*?<\/style>/gi, " "), r = /<(h[1-6]|p|label|button)\b[^>]*>([^]*?)<\/\1>|<input\b[^>]*>/gi, i; for (; (i = r.exec(n)) && e.length < Ae;) {
        var h = (o = i[0]) != null ? o : "", p = String((s = i[1]) != null ? s : "").toLowerCase();
        if (h.toLowerCase().startsWith("<input"))
            continue;
        var d = Fn(p === "p" || p === "label" ? (u = i[2]) != null ? u : "" : (a = i[2]) != null ? a : "");
        d.length > 0 && e.push(d);
    } return e; }
    function ms(t) { var e = hs(t), n = je(e); return je(n); }
    function fs(t, e, n, r) {
        var e_38, _a;
        var a, h, p, d, b, y;
        var i = je((h = Ci.get(String((a = t.key) != null ? a : ""))) != null ? h : []), o = je(String((d = (p = t.attrs) == null ? void 0 : p["data-trueos-srcdoc-text"]) != null ? d : "").split("\n").map(function (I) { return he(I); })), s = i.length > 0 ? i : o.length > 0 ? o : ms(String((y = (b = t.attrs) == null ? void 0 : b.srcdoc) != null ? y : "")), u = n + 48;
        try {
            for (var s_2 = __values(s), s_2_1 = s_2.next(); !s_2_1.done; s_2_1 = s_2.next()) {
                var I = s_2_1.value;
                if (r.length >= Ae)
                    return;
                r.push({ x: e + 16, y: u, text: I }), u += 32;
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
    function Tn(t) { var e; return t.kind === "text" ? (e = t.text) != null ? e : "" : t.children.map(Tn).join(" "); }
    function ps(t) { var e = [], n = function (r, i, o, s) {
        var e_39, _a;
        var c, _, g;
        if (e.length >= Ae)
            return;
        var u = i + r.x, a = o + r.y, h = r.kind === "block" && r.tagName === "iframe" && String((_ = (c = r.attrs) == null ? void 0 : c["data-root"]) != null ? _ : "") !== "1", p = s + (h ? 1 : 0), d = r.kind === "block" && r.tagName === "button", b = r.kind === "text" ? (g = r.text) != null ? g : "" : d ? Tn(r) : "", y = he(Un(b)), I = e.length;
        if (Zo(y)) {
            var M = d ? u + 8 : u, S = d ? a + Math.max(0, Math.floor((r.height - ye.fontSize * 1.25) / 2)) : a;
            e.push({ x: M, y: S, text: y });
        }
        if (!d) {
            try {
                for (var _b = __values(r.children), _c = _b.next(); !_c.done; _c = _b.next()) {
                    var M = _c.value;
                    n(M, u, a, p);
                }
            }
            catch (e_39_1) { e_39 = { error: e_39_1 }; }
            finally {
                try {
                    if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
                }
                finally { if (e_39) throw e_39.error; }
            }
            h && e.length === I && fs(r, u, a, e);
        }
    }; return n(t, 0, 0, 0), e; }
    function gs(t, e) {
        if (e === void 0) { e = 8; }
        var n = [];
        for (var r = 0; r < t.length && n.length < e; r += 1) {
            var i = t[r];
            n.push("#".concat(n.length, " x=").concat(Math.round(i.x), " y=").concat(Math.round(i.y), " text=\"").concat(wn(i.text), "\""));
        }
        return n.join("|");
    }
    function bs() {
        var e_40, _a;
        var i, o, s, u;
        var t = (o = (i = window.__pixiCapture) == null ? void 0 : i.commands) != null ? o : [], e = {}, n = {}, r = new Set(["addChild", "addChildAt", "setChildIndex", "removeChild", "removeChildren", "removeAllListeners", "on", "clear", "rect", "roundRect", "circle", "ellipse", "moveTo", "lineTo", "closePath", "poly", "fill", "stroke", "image", "visible", "alpha", "scale", "mask", "text.text.set", "text.style.set", "text.resolution.set", "text.setSize", "render", "snapshot"]);
        try {
            for (var t_2 = __values(t), t_2_1 = t_2.next(); !t_2_1.done; t_2_1 = t_2.next()) {
                var a = t_2_1.value;
                var h = Ne(a == null ? void 0 : a.op);
                h && (e[h] = ((s = e[h]) != null ? s : 0) + 1, r.has(h) || (n[h] = ((u = n[h]) != null ? u : 0) + 1));
            }
        }
        catch (e_40_1) { e_40 = { error: e_40_1 }; }
        finally {
            try {
                if (t_2_1 && !t_2_1.done && (_a = t_2.return)) _a.call(t_2);
            }
            finally { if (e_40) throw e_40.error; }
        }
        return { total: t.length, ops: Wn(e, 24), unsupported: Wn(n, 24) };
    }
    function ki(t, e, n, r, i, o) {
        if (i === void 0) { i = ""; }
        if (o === void 0) { o = { hash: "", renderHash: "", layoutHash: "", bytes: 0 }; }
        if (!Pt())
            return;
        var s = bs();
        window.__TRUEOS_PIXI_BRIDGE_STATS__ = { renderNodes: t.nodes, renderBlocks: t.blocks, renderText: t.text, renderTags: Wn(t.tags, 24), renderTextSamples: n, layoutBoxes: e.nodes, layoutBlocks: e.blocks, layoutText: e.text, layoutMaxDepth: e.maxDepth, layoutTextSamples: r, layoutWidgetSamples: i, prePixiHash: o.hash, prePixiRenderHash: o.renderHash, prePixiLayoutHash: o.layoutHash, prePixiTraceBytes: o.bytes, measureTextCalls: Bn, scrollbarVisible: l.scroll.track.h > 0 ? 1 : 0, scrollbarTrack: "".concat(Math.round(l.scroll.track.x), ",").concat(Math.round(l.scroll.track.y), ",").concat(Math.round(l.scroll.track.w), ",").concat(Math.round(l.scroll.track.h)), scrollbarThumb: "".concat(Math.round(l.scroll.thumb.x), ",").concat(Math.round(l.scroll.thumb.y), ",").concat(Math.round(l.scroll.thumb.w), ",").concat(Math.round(l.scroll.thumb.h)), pixiCommands: s.total, pixiOps: s.ops, pixiUnsupported: s.unsupported };
    }
    var Ri = new WeakMap;
    function Xn(t, e) { var n = t; for (; n;) {
        if (n === e)
            return !0;
        n = n.parent;
    } return !1; }
    function $i(t) { return Array.isArray(t.children) || (t.children = []), t.children; }
    function qt(t, e, n) { var r = Number(e) || 0, i = Number(n) || 0; (!t.position || typeof t.position != "object") && (t.position = { x: 0, y: 0 }), t.position.x = r, t.position.y = i; }
    function ze(t, e, n) { if (e === t || Xn(t, e))
        return; var r = $i(t); if (e.parent !== t) {
        var u = Math.max(0, Math.min(n, r.length));
        t.addChildAt(e, u);
        return;
    } var i = Math.max(0, r.length - 1), o = Math.max(0, Math.min(n, i)); t.getChildIndex(e) !== o && t.setChildIndex(e, o); }
    function Oi(t, e, n) { if (e === t || Xn(t, e))
        return; var r = $i(t); if (e.parent !== t) {
        var u = Math.max(0, Math.min(n, r.length));
        t.addChildAt(e, u);
        return;
    } var i = Math.max(0, r.length - 1), o = Math.max(0, Math.min(n, i)); t.getChildIndex(e) !== o && t.setChildIndex(e, o); }
    var pn = null, ct = null;
    function Vt(t) { var e = l.cursorColors.get(t); if (e != null)
        return e; var n = [1118481, 2450411, 1483594, 14427686, 8141549, 959977, 16096779], r = Math.abs(Number(t) || 0) % n.length, i = n[r]; return l.cursorColors.set(t, i), i; }
    function Wt(t) { var i, o, s, u, a, h; var e = Number((s = (o = t == null ? void 0 : t.pointerId) != null ? o : (i = t == null ? void 0 : t.data) == null ? void 0 : i.pointerId) != null ? s : 0), r = String((h = (a = t == null ? void 0 : t.pointerType) != null ? a : (u = t == null ? void 0 : t.data) == null ? void 0 : u.pointerType) != null ? h : "").toLowerCase() === "mouse" || e === 1 || e === l.primaryMousePointerId; return l.harness.enabled && r ? l.harness.activeUserPointerId : e; }
    function Pt() { return !!globalThis.__TRUEOS_CAPTURE_ONLY__; }
    function wt(t) { var i; if (!Pt() || (window.__TRUEOS_PIXI_APP_PHASE__ = t, !{ "main:start": !0, "main:yoga": !0, "main:create-app": !0, "main:attach-capture": !0, "main:append-canvas": !0, "main:capture-flags": !0, "main:canvas-listeners": !0, "main:stage:done": !0, "main:roots": !0, "main:text-measure": !0, "main:html": !0, "main:render-tree": !0, "main:first-rerender": !0, "main:layout-build": !0, "main:layout-commit": !0, "main:paint:clamp": !0, "main:paint:render-to-pixi": !0, "main:paint:scrollbar": !0, "main:paint:renderer-render": !0, "main:paint:done": !0, "main:cursor-setup": !0, "main:input-listeners": !0, "main:done": !0 }[t]))
        return; var n = window, r = (i = n.__TRUEOS_PIXI_PHASE_TRACE_SEEN__) != null ? i : n.__TRUEOS_PIXI_PHASE_TRACE_SEEN__ = {}; r[t] || (r[t] = 1, console.log("[Trace] [pixi] phase=".concat(t))); }
    function X(t) { Pt() && (window.__TRUEOS_PIXI_LAYOUT_STEP__ = t); }
    function Mt(t) { Pt() && console.log(t); }
    function re(t, e, n) { var o; if (!Pt())
        return; var r = "__TRUEOS_".concat(t, "_LOG_COUNT__"), i = Number((o = window[r]) != null ? o : 0) || 0; i >= e || (window[r] = i + 1, console.log(n)); }
    function Wi(t) { var u, a, h, p, d; var e = (u = window.__TRUEOS_PIXI_APP_PHASE__) != null ? u : "unknown", n = (a = window.__TRUEOS_PIXI_LAYOUT_STEP__) != null ? a : "", r = t, i = String((h = r == null ? void 0 : r.name) != null ? h : "Error"), o = String((p = r == null ? void 0 : r.message) != null ? p : t), s = String((d = r == null ? void 0 : r.stack) != null ? d : ""); return "phase=".concat(e, " layout=").concat(n, " name=").concat(i, " message=").concat(o, " stack=").concat(s); }
    function _s() { var t = Math.max(1, Number(window.innerWidth || 1920) | 0), e = Math.max(1, Number(window.innerHeight || 1080) | 0), n = new gt(0, 0, t, e), r = document.createElement("canvas"), i = { width: t, height: e, screen: n, render: function (o) { return o; }, resize: function (o, s) { var u = Math.max(1, Number(o || t) | 0), a = Math.max(1, Number(s || e) | 0); this.width = u, this.height = a, n.width = u, n.height = a; } }; return { stage: new xt, screen: n, canvas: r, renderer: i, ticker: { stop: function () { }, add: function () { }, remove: function () { } } }; }
    function ys() { var g = 0, M = 0, S = 2e4; return { Node: { create: function () { return ({ children: [], measureFunc: null, paddingLeft: 0, paddingTop: 0, paddingRight: 0, paddingBottom: 0, marginLeft: 0, marginTop: 0, marginRight: 0, marginBottom: 0, width: 0, height: 0, minWidth: 0, minHeight: 0, flexDirection: 0, alignItems: 0, justifyContent: 1, flexWrap: 0, positionType: 0, positionLeft: null, positionTop: null, positionRight: null, positionBottom: null, computed: { left: 0, top: 0, width: 0, height: 0 }, debugLabel: "node", setMeasureFunc: function (w) { this.measureFunc = w; }, setMargin: function (w, R) { var E = Number(R) || 0; w === 0 ? this.marginLeft = E : w === 1 ? this.marginTop = E : w === 2 ? this.marginRight = E : w === 3 && (this.marginBottom = E); }, setPadding: function (w, R) { var E = Number(R) || 0; w === 0 ? this.paddingLeft = E : w === 1 ? this.paddingTop = E : w === 2 ? this.paddingRight = E : w === 3 && (this.paddingBottom = E); }, setFlexDirection: function (w) { this.flexDirection = w; }, setAlignItems: function (w) { this.alignItems = Number(w) || 0; }, setJustifyContent: function (w) { this.justifyContent = Number(w) || 0; }, setFlexWrap: function (w) { this.flexWrap = Number(w) === 1 ? 1 : 0; }, setFlexGrow: function (w) { }, setFlexShrink: function (w) { }, setAlignSelf: function (w) { }, setPositionType: function (w) { this.positionType = Number(w) === 1 ? 1 : 0; }, setPosition: function (w, R) { var E = Number(R) || 0; w === 0 ? this.positionLeft = E : w === 1 ? this.positionTop = E : w === 2 ? this.positionRight = E : w === 3 && (this.positionBottom = E); }, setWidth: function (w) { this.width = Math.max(0, Number(w) || 0); }, setHeight: function (w) { this.height = Math.max(0, Number(w) || 0); }, setMinWidth: function (w) { this.minWidth = Math.max(0, Number(w) || 0); }, setMinHeight: function (w) { this.minHeight = Math.max(0, Number(w) || 0); }, insertChild: function (w, R) { this.children.splice(Math.max(0, Math.min(R, this.children.length)), 0, w); }, getChildCount: function () { return this.children.length; }, getComputedLeft: function () { return this.computed.left; }, getComputedTop: function () { return this.computed.top; }, getComputedWidth: function () { return this.computed.width; }, getComputedHeight: function () { return this.computed.height; }, freeRecursive: function () { }, calculateLayout: function (w, R) {
                    if (w === void 0) { w = this.width; }
                    if (R === void 0) { R = this.height; }
                    this.layout(0, 0, Math.max(1, Number(w) || this.width || 1), Math.max(1, Number(R) || this.height || 1));
                }, layout: function (w, R, E, x) {
                    var e_41, _a, e_42, _b, e_43, _c;
                    var D, Y, rt, ot;
                    if (g += 1, (g <= 80 || g % 500 === 0) && (M += 1, M <= 140 && Mt("[trueos pixi widgets] yoga-layout-call #".concat(g, " label=\"").concat(this.debugLabel, "\" children=").concat(this.children.length, " flex=").concat(this.flexDirection, " pos=").concat(this.positionType, " xy=").concat(Math.round(w), ",").concat(Math.round(R), " avail=").concat(Math.round(E), "x").concat(Math.round(x), " own=").concat(Math.round(this.width), "x").concat(Math.round(this.height), " min=").concat(Math.round(this.minWidth), "x").concat(Math.round(this.minHeight)))), g > S)
                        throw new Error("capture yoga layout budget exceeded count=".concat(g, " label=\"").concat(this.debugLabel, "\" children=").concat(this.children.length, " flex=").concat(this.flexDirection, " pos=").concat(this.positionType, " avail=").concat(Math.round(E), "x").concat(Math.round(x)));
                    var W = this.paddingLeft + this.paddingRight, j = this.paddingTop + this.paddingBottom, V = Math.max(this.minWidth, this.width || E), m = Math.max(this.minHeight, this.height || 0);
                    if (this.computed.left = w, this.computed.top = R, this.computed.width = V, this.measureFunc) {
                        var q = this.measureFunc(Math.max(0, V - W), 0);
                        m = Math.max(m, Math.ceil(Number(q.height) || 0) + j), this.computed.height = m;
                        return;
                    }
                    if (this.flexDirection === 1) {
                        var q = this.paddingLeft, K = 0, et = Math.max(1, this.children.length);
                        try {
                            for (var _d = __values(this.children), _f = _d.next(); !_f.done; _f = _d.next()) {
                                var k = _f.value;
                                if (k.positionType === 1)
                                    continue;
                                var H = k.width || k.minWidth || Math.max(24, (V - W) / et);
                                k.layout(q + k.marginLeft, this.paddingTop + k.marginTop, H, x), q += k.computed.width + k.marginLeft + k.marginRight, K = Math.max(K, k.computed.height + k.marginTop + k.marginBottom);
                            }
                        }
                        catch (e_41_1) { e_41 = { error: e_41_1 }; }
                        finally {
                            try {
                                if (_f && !_f.done && (_a = _d.return)) _a.call(_d);
                            }
                            finally { if (e_41) throw e_41.error; }
                        }
                        try {
                            for (var _g = __values(this.children), _h = _g.next(); !_h.done; _h = _g.next()) {
                                var k = _h.value;
                                if (k.positionType === 1) {
                                    var H = k.width || k.minWidth || Math.max(0, V - W - k.marginLeft - k.marginRight), Z = k.height || k.minHeight || x, G = k.positionLeft != null ? this.paddingLeft + k.positionLeft : Math.max(0, V - this.paddingRight - ((D = k.positionRight) != null ? D : 0) - H), at = k.positionTop != null ? this.paddingTop + k.positionTop : Math.max(0, m - this.paddingBottom - ((Y = k.positionBottom) != null ? Y : 0) - Z);
                                    k.layout(G + k.marginLeft, at + k.marginTop, H, Z);
                                }
                            }
                        }
                        catch (e_42_1) { e_42 = { error: e_42_1 }; }
                        finally {
                            try {
                                if (_h && !_h.done && (_b = _g.return)) _b.call(_g);
                            }
                            finally { if (e_42) throw e_42.error; }
                        }
                        m = Math.max(m, K + j);
                    }
                    else {
                        var q = this.paddingTop;
                        try {
                            for (var _j = __values(this.children), _k = _j.next(); !_k.done; _k = _j.next()) {
                                var K = _k.value;
                                if (K.positionType === 1) {
                                    var k = K.width || K.minWidth || Math.max(0, V - W - K.marginLeft - K.marginRight), H = K.height || K.minHeight || x, Z = K.positionLeft != null ? this.paddingLeft + K.positionLeft : Math.max(0, V - this.paddingRight - ((rt = K.positionRight) != null ? rt : 0) - k), G = K.positionTop != null ? this.paddingTop + K.positionTop : Math.max(0, m - this.paddingBottom - ((ot = K.positionBottom) != null ? ot : 0) - H);
                                    K.layout(Z + K.marginLeft, G + K.marginTop, k, H);
                                    continue;
                                }
                                var et = Math.max(0, V - W - K.marginLeft - K.marginRight);
                                K.layout(this.paddingLeft + K.marginLeft, q + K.marginTop, et, x), q += K.computed.height + K.marginTop + K.marginBottom;
                            }
                        }
                        catch (e_43_1) { e_43 = { error: e_43_1 }; }
                        finally {
                            try {
                                if (_k && !_k.done && (_c = _j.return)) _c.call(_j);
                            }
                            finally { if (e_43) throw e_43.error; }
                        }
                        m = Math.max(m, q + this.paddingBottom);
                    }
                    this.computed.height = Math.max(this.minHeight, m);
                } }); } }, EDGE_LEFT: 0, EDGE_TOP: 1, EDGE_RIGHT: 2, EDGE_BOTTOM: 3, FLEX_DIRECTION_COLUMN: 0, FLEX_DIRECTION_ROW: 1, FLEX_DIRECTION_ROW_REVERSE: 1, ALIGN_STRETCH: 0, ALIGN_CENTER: 1, ALIGN_FLEX_START: 2, JUSTIFY_CENTER: 0, JUSTIFY_FLEX_START: 1, JUSTIFY_SPACE_BETWEEN: 2, WRAP_WRAP: 1, WRAP_NO_WRAP: 0, POSITION_TYPE_RELATIVE: 0, POSITION_TYPE_ABSOLUTE: 1, DIRECTION_LTR: 0, MEASURE_MODE_UNDEFINED: 0 }; }
    function xs(t) {
        var e_44, _a;
        var r;
        var e = 0, n = function (i, o, s) {
            var e_45, _a;
            var h;
            var u = o + i.x, a = s + i.y;
            if (!(i.kind === "block" && i.tagName === "dialog")) {
                e = Math.max(e, a + i.height);
                try {
                    for (var _b = __values((h = i.children) != null ? h : []), _c = _b.next(); !_c.done; _c = _b.next()) {
                        var p = _c.value;
                        n(p, u, a);
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
    function fn(t, e) { var o, s, u, a; var n = l.inputs.get(t); if (n)
        return n; var r = {}, i = ((o = e == null ? void 0 : e.type) != null ? o : "text").toLowerCase(); if (i === "checkbox" || i === "radio") {
        if (r.checked = e ? Object.prototype.hasOwnProperty.call(e, "checked") : !1, i === "checkbox") {
            var h = ((s = e == null ? void 0 : e["aria-checked"]) != null ? s : "").toLowerCase(), p = ((u = e == null ? void 0 : e["data-indeterminate"]) != null ? u : "").toLowerCase();
            r.indeterminate = (e ? Object.prototype.hasOwnProperty.call(e, "indeterminate") : !1) || h === "mixed" || p === "true" || p === "1" || p === "yes";
        }
    }
    else
        r.value = (a = e == null ? void 0 : e.value) != null ? a : ""; return l.inputs.set(t, r), r; }
    function ws(t) { var e = new Map; function n(r) {
        var e_46, _a;
        var i, o, s, u, a;
        if (r.kind === "block" && r.tagName === "input" && ((o = (i = r.attrs) == null ? void 0 : i.type) != null ? o : "text").toLowerCase() === "radio") {
            var d = "radio:".concat((u = (s = r.attrs) == null ? void 0 : s.name) != null ? u : "__default__"), b = r.key;
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
        catch (e_46_1) { e_46 = { error: e_46_1 }; }
        finally {
            try {
                if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
            }
            finally { if (e_46) throw e_46.error; }
        }
    } return n(t), e; }
    function he(t) { var e = "", n = !1, r = typeof t == "string" ? t : ""; for (var i = 0; i < r.length; i += 1) {
        var o = r.charCodeAt(i);
        if (o === 32 || o === 9 || o === 10 || o === 13 || o === 12) {
            n = !0;
            continue;
        }
        n && e.length > 0 && (e += " "), e += r.charAt(i), n = !1;
    } return e; }
    function Ts(t) {
        var e_47, _a;
        if (!t || typeof t != "object")
            return;
        var e = {};
        try {
            for (var _b = __values(Object.entries(t)), _c = _b.next(); !_c.done; _c = _b.next()) {
                var _d = __read(_c.value, 2), n = _d[0], r = _d[1];
                typeof n != "string" || n.length === 0 || (e[n] = typeof r == "string" ? r : "");
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
    function Fi(t, e, n) { var h, p; if (!t || typeof t != "object")
        return null; var r = t, i = typeof r.kind == "string" ? r.kind : ""; if (i === "text") {
        var d = typeof r.text == "string" ? r.text : "", b = "", y = (h = n == null ? void 0 : n.rows[n.index]) != null ? h : "", I = !1;
        if (n && n.index < n.rows.length ? (n.index += 1, b = y, I = !0) : b = he(Un(d)), !I && (d.indexOf("<truesurfer-") >= 0 || d.indexOf("__trueo") >= 0) || b.startsWith("<truesurfer-") || b.startsWith("__trueo"))
            b = "";
        else if (b.length === 0) {
            var _ = (p = n == null ? void 0 : n.rows[n.index]) != null ? p : "";
            n && _ && (n.index += 1), _ && (b = _);
        }
        return b.length > 0 ? { kind: "text", text: b } : null;
    } if (i !== "block")
        return null; var o = typeof r.tagName == "string" ? r.tagName.toLowerCase() : ""; if (o.length === 0)
        return null; var s = typeof r.key == "string" ? r.key : "".concat(e, ":").concat(o), u = [], a = Array.isArray(r.children) ? r.children : []; for (var d = 0; d < a.length; d += 1) {
        var b = Fi(a[d], "".concat(e, ".").concat(d), n);
        b && u.push(b);
    } return { kind: "block", key: s, tagName: o, attrs: Ts(r.attrs), children: u }; }
    function Es(t, e) { var n = Array.isArray(t) ? t : t && typeof t == "object" && Array.isArray(t.widgetRenderTree) ? t.widgetRenderTree : [], i = { rows: Array.isArray(e) ? je(e) : Hi(e), index: 0 }, o = []; for (var s = 0; s < n.length; s += 1) {
        var u = Fi(n[s], "0.".concat(s), i);
        u && o.push(u);
    } return o; }
    function Is(t, e) { if (!Array.isArray(e) || e.length === 0)
        return 0; var n = 0, r = 0, i = function (o) { if (o.kind === "text") {
        if (n < e.length) {
            var s = e[n];
            n += 1, typeof s == "string" && s.length > 0 && s.indexOf("<truesurfer-") !== 0 && s.indexOf("__trueo") !== 0 && (o.text = s, r += 1);
        }
        return;
    } for (var s = 0; s < o.children.length; s += 1)
        i(o.children[s]); }; for (var o = 0; o < t.length; o += 1)
        i(t[o]); return r; }
    function Ms(t) { var n = document.createElement("canvas").getContext("2d"); if (!n)
        throw new Error("2D canvas not available"); n.font = t; var r = t.indexOf("px"), i = r; for (; i > 0;) {
        var u = t.charCodeAt(i - 1);
        if (u < 48 || u > 57)
            break;
        i -= 1;
    } var o = r > i ? Number(t.slice(i, r)) : 16, s = Math.ceil(o * 1.25); return { measure: function (u, a) {
            var e_48, _a;
            Bn += 1;
            var h = he(u).split(" ").filter(Boolean);
            if (h.length === 0)
                return { width: 0, height: s, lines: [""] };
            var p = [], d = "";
            try {
                for (var h_1 = __values(h), h_1_1 = h_1.next(); !h_1_1.done; h_1_1 = h_1.next()) {
                    var I = h_1_1.value;
                    var c = d ? "".concat(d, " ").concat(I) : I, _ = n.measureText(c).width, g = a != null ? a : Number.POSITIVE_INFINITY;
                    _ <= g || !d ? d = c : (p.push(d), d = I);
                }
            }
            catch (e_48_1) { e_48 = { error: e_48_1 }; }
            finally {
                try {
                    if (h_1_1 && !h_1_1.done && (_a = h_1.return)) _a.call(h_1);
                }
                finally { if (e_48) throw e_48.error; }
            }
            d && p.push(d);
            var b = Math.min(Math.max.apply(Math, __spreadArray([], __read(p.map(function (I) { return n.measureText(I).width; })), false)), a != null ? a : Number.POSITIVE_INFINITY), y = p.length * s;
            return { width: Math.ceil(b), height: Math.ceil(y), lines: p };
        }, lineHeight: s, font: t }; }
    function Ss(t, e, n) { var I; X("build:start nodes=".concat(t.length, " viewport=").concat(e, "x").concat(n)), window.__TRUEOS_PIXI_LAYOUT_BUILD_COUNT__ = 0, Mt("[trueos pixi widgets] layout-build begin nodes=".concat(t.length, " viewport=").concat(e, "x").concat(n)); var r = 12, i = 8, o = ye; X("build:measurer"); var s = Ms("".concat(o.fontSize, "px ").concat(o.fontFamily)); function u(c) { return c.kind !== "block" || c.tagName === "hr" || c.tagName === "tr" || c.tagName === "td" || c.tagName === "th" ? 0 : i; } var a = 0; function h(c) { if (a += 1, (a <= 140 || a % 250 === 0) && Mt("[trueos pixi widgets] layout-box-build #".concat(a, " label=\"").concat(c, "\"")), a > 5e3)
        throw new Error("layout box build budget exceeded count=".concat(a, " label=\"").concat(c, "\"")); } function p(c) { var $; var _ = c.kind === "text" ? "text:".concat(c.text.slice(0, 24)) : "".concat(c.tagName, ":").concat(c.key); if (X("node:".concat(_, ":start")), c.kind === "text") {
        var w_1 = st.Node.create();
        return w_1.debugLabel = _, X("node:".concat(_, ":measure-func")), w_1.setMeasureFunc(function (R, E) { X("node:".concat(_, ":measure-call")); var x = E === st.MEASURE_MODE_UNDEFINED ? void 0 : Math.max(0, R), W = s.measure(c.text, x); return { width: W.width, height: W.height }; }), w_1.setMargin(st.EDGE_RIGHT, 6), w_1.setMargin(st.EDGE_BOTTOM, 0), { yogaNode: w_1, buildBox: function () { return (h(_), { kind: "text", text: c.text, x: w_1.getComputedLeft(), y: w_1.getComputedTop(), width: w_1.getComputedWidth(), height: w_1.getComputedHeight(), children: [] }); } };
    } if (c.tagName === "sliderlabel")
        return X("node:".concat(c.tagName, ":").concat(c.key, ":sliderlabel")), cr({ node: c, Yoga: st, measurer: s }); X("node:".concat(c.tagName, ":").concat(c.key, ":create")); var g = st.Node.create(); if (g.debugLabel = _, X("node:".concat(c.tagName, ":").concat(c.key, ":base-defaults")), g.setFlexDirection(st.FLEX_DIRECTION_COLUMN), g.setAlignItems(st.ALIGN_STRETCH), g.setPadding(st.EDGE_LEFT, r), g.setPadding(st.EDGE_RIGHT, r), g.setPadding(st.EDGE_TOP, r), g.setPadding(st.EDGE_BOTTOM, r), g.setMargin(st.EDGE_BOTTOM, 0), An(c.tagName) && (X("node:".concat(c.tagName, ":").concat(c.key, ":heading-defaults")), Ir(g, st)), c.tagName === "hr" && (X("node:".concat(c.tagName, ":").concat(c.key, ":hr-defaults")), gr(g, st)), (c.tagName === "p" || c.tagName === "label") && (X("node:".concat(c.tagName, ":").concat(c.key, ":inline-scan")), c.children.some(function (R) { return R.kind === "block" && (R.tagName === "input" || R.tagName === "button" || R.tagName === "select" || R.tagName === "textarea" || R.tagName === "timeinput" || R.tagName === "dateinput" || R.tagName === "monthinput" || R.tagName === "weekinput" || R.tagName === "datetimelocalinput" || R.tagName === "progress" || R.tagName === "meter" || R.tagName === "slider" || R.tagName === "number" || R.tagName === "color"); }) && (g.setFlexDirection(st.FLEX_DIRECTION_ROW), g.setFlexWrap(st.WRAP_WRAP), g.setAlignItems(st.ALIGN_CENTER)), g.setPadding(st.EDGE_TOP, 4), g.setPadding(st.EDGE_BOTTOM, 4), g.setPadding(st.EDGE_LEFT, 4), g.setPadding(st.EDGE_RIGHT, 4)), c.tagName === "table" && (X("node:".concat(c.tagName, ":").concat(c.key, ":table-defaults")), wr(g, st)), c.tagName === "tr" && (X("node:".concat(c.tagName, ":").concat(c.key, ":tr-defaults")), Tr(g, st)), (c.tagName === "td" || c.tagName === "th") && (X("node:".concat(c.tagName, ":").concat(c.key, ":cell-defaults")), Er(g, st)), c.tagName === "input" && (X("node:".concat(c.tagName, ":").concat(c.key, ":input-defaults")), Yr(g, c, st)), c.tagName === "textarea" && (X("node:".concat(c.tagName, ":").concat(c.key, ":textarea-defaults")), zr(g, st)), c.tagName === "select" && (X("node:".concat(c.tagName, ":").concat(c.key, ":select-defaults")), ai(g, st)), c.tagName === "timeinput" || c.tagName === "dateinput" || c.tagName === "monthinput" || c.tagName === "weekinput" || c.tagName === "datetimelocalinput") {
        var w = c.tagName === "timeinput" ? "time" : c.tagName === "monthinput" ? "month" : c.tagName === "weekinput" ? "week" : c.tagName === "dateinput" ? "date" : "datetime-local";
        X("node:".concat(c.tagName, ":").concat(c.key, ":temporal-defaults")), ui(g, st, w);
    } c.tagName === "img" && (X("node:".concat(c.tagName, ":").concat(c.key, ":img-defaults")), Nr(g, c, st)), c.tagName === "svg" && (X("node:".concat(c.tagName, ":").concat(c.key, ":svg-defaults")), $r(g, c, st)), c.tagName === "canvas" && (X("node:".concat(c.tagName, ":").concat(c.key, ":canvas-defaults")), Fr(g, c, st)), c.tagName === "iframe" && (X("node:".concat(c.tagName, ":").concat(c.key, ":iframe-defaults")), Ur(g, c, st)), c.tagName === "button" && (X("node:".concat(c.tagName, ":").concat(c.key, ":button-defaults")), _r(g, st)), c.tagName === "dialog" && (X("node:".concat(c.tagName, ":").concat(c.key, ":dialog-defaults")), qr(g, st)), c.tagName === "number" && (X("node:".concat(c.tagName, ":").concat(c.key, ":number-defaults")), ei(g, st)), c.tagName === "color" && (X("node:".concat(c.tagName, ":").concat(c.key, ":color-defaults")), ii(g, c, st)), c.tagName === "searchrow" && (X("node:".concat(c.tagName, ":").concat(c.key, ":searchrow-defaults")), Jr(g, st)), c.tagName === "searchbutton" && (X("node:".concat(c.tagName, ":").concat(c.key, ":searchbutton-defaults")), Qr(g, st)), c.tagName === "summary" && (X("node:".concat(c.tagName, ":").concat(c.key, ":summary-defaults")), hr(g, st)), c.tagName === "details" && (X("node:".concat(c.tagName, ":").concat(c.key, ":details-defaults")), mr(g, st)), c.tagName === "barrow" && (X("node:".concat(c.tagName, ":").concat(c.key, ":barrow-defaults")), Vr(g, st)), (c.tagName === "progress" || c.tagName === "meter") && (X("node:".concat(c.tagName, ":").concat(c.key, ":progress-defaults")), ar(g, st)), c.tagName === "slider" && (X("node:".concat(c.tagName, ":").concat(c.key, ":slider-defaults")), lr(g, st)), X("node:".concat(c.tagName, ":").concat(c.key, ":children-effective")); var M = fr(c, l.detailsOpen), S = Number(($ = window.__TRUEOS_PIXI_LAYOUT_BUILD_COUNT__) != null ? $ : 0) + 1; window.__TRUEOS_PIXI_LAYOUT_BUILD_COUNT__ = S, (S <= 120 || S % 50 === 0) && Mt("[trueos pixi widgets] layout-build-node #".concat(S, " label=\"").concat(_, "\" children=").concat(c.children.length, " effective=").concat(M.length)), X("node:".concat(c.tagName, ":").concat(c.key, ":children-map count=").concat(M.length)); var B = M.map(p); (S <= 120 || S % 50 === 0) && Mt("[trueos pixi widgets] layout-build-node-mapped #".concat(S, " label=\"").concat(_, "\" pairs=").concat(B.length)), X("node:".concat(c.tagName, ":").concat(c.key, ":children-insert")); for (var w = 0; w < B.length; w++) {
        var R = M[w], E = B[w];
        if (R && R.kind === "block") {
            var x = w === B.length - 1 ? 0 : u(R);
            E.yogaNode.setMargin(st.EDGE_BOTTOM, x);
        }
        g.insertChild(E.yogaNode, g.getChildCount());
    } return { yogaNode: g, buildBox: function () { return (h(_), { kind: "block", key: c.key, tagName: c.tagName, attrs: c.attrs, x: g.getComputedLeft(), y: g.getComputedTop(), width: g.getComputedWidth(), height: g.getComputedHeight(), children: B.map(function (w) { return w.buildBox(); }) }); } }; } var d = st.Node.create(); d.debugLabel = "root", X("root:flex-direction"), d.setFlexDirection(st.FLEX_DIRECTION_COLUMN), X("root:align-items"), d.setAlignItems(st.ALIGN_STRETCH), X("root:width"), d.setWidth(e), X("root:height"), d.setHeight(n), X("root:padding-left"), d.setPadding(st.EDGE_LEFT, 16), X("root:padding-top"), d.setPadding(st.EDGE_TOP, 16), X("root:padding-right"), d.setPadding(st.EDGE_RIGHT, 16 + gn), X("root:padding-bottom"), d.setPadding(st.EDGE_BOTTOM, 16), X("root:children-map count=".concat(t.length)), Mt("[trueos pixi widgets] layout-root children-map count=".concat(t.length)); var b = t.map(p); X("root:children-insert"), Mt("[trueos pixi widgets] layout-root children-insert pairs=".concat(b.length)); for (var c = 0; c < b.length; c++) {
        var _ = t[c], g = b[c];
        if (_ && _.kind === "block") {
            var M = c === b.length - 1 ? 0 : u(_);
            g.yogaNode.setMargin(st.EDGE_BOTTOM, M);
        }
        d.insertChild(g.yogaNode, d.getChildCount());
    } X("root:calculate"), Mt("[trueos pixi widgets] layout-root calculate begin"), d.calculateLayout(e, n, st.DIRECTION_LTR), Mt("[trueos pixi widgets] layout-root calculate done"), X("root:build-box"), Mt("[trueos pixi widgets] layout-root build-box begin"), h("root"); var y = { kind: "block", tagName: "root", x: 0, y: 0, width: d.getComputedWidth(), height: d.getComputedHeight(), children: b.map(function (c) { return c.buildBox(); }) }; return Mt("[trueos pixi widgets] layout-root build-box done boxes=".concat(a)), X("root:free"), (I = d.freeRecursive) == null || I.call(d), X("build:done"), y; }
    function Ps(t, e, n) {
        var e_49, _a, e_50, _b, e_51, _c, e_52, _d, e_53, _f;
        var j, V;
        X("render:start");
        var r = ye, i = n != null ? n : t.stage;
        X("render:get-background");
        var o = At(i, "__background");
        X("render:get-content-root");
        var s = le(i, "__contentRoot");
        X("render:get-dialog-root");
        var u = le(i, "__dialogRoot");
        X("render:get-overlay-root");
        var a = le(i, "__overlayRoot");
        X("render:ensure-background"), Oi(i, o, 0), X("render:ensure-content-root"), ze(i, s, 1), X("render:ensure-dialog-root"), ze(i, u, 2), X("render:ensure-overlay-root"), ze(i, a, 3), X("render:overlay-remove-children"), a.removeChildren(), X("render:overlay-removed");
        var h = [], p = [], d = ws(e);
        X("render:clear-ui-state"), l.fieldBounds.clear(), l.sliderBounds.clear(), l.dialogDragBounds.clear(), l.hoverRects.length = 0, l.hoverHandlers.clear(), l.iframeRects.length = 0, l.iframeScrollRoots.clear(), l.iframeScrollbarGraphics.clear(), X("render:node-cache");
        var b = (j = Ri.get(i)) != null ? j : new Map;
        Ri.set(i, b);
        var y = new Set, I = function (m) {
            var e_54, _a;
            var rt;
            var D = 0, Y = function (ot, q, K) {
                var e_55, _a;
                var H;
                if (ot.kind === "block" && ot.tagName === "dialog")
                    return;
                var et = q + ot.x, k = K + ot.y;
                D = Math.max(D, k + ot.height);
                try {
                    for (var _b = __values((H = ot.children) != null ? H : []), _c = _b.next(); !_c.done; _c = _b.next()) {
                        var Z = _c.value;
                        Y(Z, et, k);
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
                for (var _b = __values((rt = m.children) != null ? rt : []), _c = _b.next(); !_c.done; _c = _b.next()) {
                    var ot = _c.value;
                    Y(ot, 0, 0);
                }
            }
            catch (e_54_1) { e_54 = { error: e_54_1 }; }
            finally {
                try {
                    if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
                }
                finally { if (e_54) throw e_54.error; }
            }
            return D;
        }, c = new Set;
        try {
            for (var _g = __values(l.textDrags.values()), _h = _g.next(); !_h.done; _h = _g.next()) {
                var m = _h.value;
                c.add(m.key);
            }
        }
        catch (e_49_1) { e_49 = { error: e_49_1 }; }
        finally {
            try {
                if (_h && !_h.done && (_a = _g.return)) _a.call(_g);
            }
            finally { if (e_49) throw e_49.error; }
        }
        X("render:measure");
        var _ = zo(r);
        function g(m, D, Y) { return Math.max(D, Math.min(Y, m)); }
        var M = function (m) {
            var e_56, _a;
            try {
                for (var _b = __values(l.textDrags.entries()), _c = _b.next(); !_c.done; _c = _b.next()) {
                    var _d = __read(_c.value, 2), D = _d[0], Y = _d[1];
                    if (Y.key === m)
                        return D;
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
        }, S = function (m) {
            var e_57, _a;
            var D = l.keyboardOwnerPointerId;
            if (l.focusedKeyByPointer.get(D) === m)
                return D;
            try {
                for (var _b = __values(l.focusedKeyByPointer.entries()), _c = _b.next(); !_c.done; _c = _b.next()) {
                    var _d = __read(_c.value, 2), Y = _d[0], rt = _d[1];
                    if (rt === m)
                        return Y;
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
        X("render:background-clear"), kt(o), X("render:background-rect"), o.rect(0, 0, t.renderer.width, t.renderer.height), X("render:background-fill"), o.fill(r.background), X("render:content-position");
        {
            var m = l.scroll, D = m && Number(m.y || 0) || 0, Y = s.position;
            Y && (Y.x = 0, Y.y = -D);
        }
        X("render:content-position-done");
        function B(m, D, Y, rt, ot, q, K, et, k) {
            var e_58, _a;
            if (rt === void 0) { rt = 0; }
            if (ot === void 0) { ot = 0; }
            var O, v, T, C, P, A, N, J, z, U, Q, nt, ht, yt, Tt, Ot, Rt, Et, Ht, Ct;
            X("render:draw:".concat(et, ":").concat(m.kind, ":").concat(m.kind === "block" ? m.tagName : "text", ":start"));
            var H = m.kind === "block" ? m.key && m.key.length > 0 ? m.key : "".concat(et, ":").concat((O = m.tagName) != null ? O : "block") : "", Z = m.kind === "block" ? "b:".concat(H) : "t:".concat(et);
            X("render:draw:".concat(et, ":cache"));
            var G = b.get(Z);
            (!G || Xn(D, G)) && (X("render:draw:".concat(et, ":new-container")), G = new xt, G.label = Z, b.set(Z, G)), X("render:draw:".concat(et, ":ensure-child")), y.add(Z), ze(D, G, k), X("render:draw:".concat(et, ":children-root"));
            var at = le(G, "__children");
            if (X("render:draw:".concat(et, ":ensure-children-root")), ze(G, at, 1), X("render:draw:".concat(et, ":position")), qt(G, m.x, m.y), m.kind === "block" && m.tagName === "hr" && qt(G, Math.round(m.x), Math.round(m.y)), m.kind === "block" && m.tagName === "dialog" && m.key) {
                var tt = nn(l.dialogs, m.key), lt = Math.max(0, m.width), ut = Math.max(0, m.height), dt = K.x, Bt = K.y, Lt = Math.max(dt, K.x + K.w - lt), $t = Math.max(Bt, K.y + K.h - ut);
                if (l.dialogDragBounds.set(m.key, { minX: dt, minY: Bt, maxX: Lt, maxY: $t }), Pt() && !tt.__trueosInitialPositionSeeded) {
                    var Jt = K.w <= 760 && K.h <= 800, ee = dt + Math.max(12, Math.floor((K.w - lt) / 2)), me = Bt + Math.max(Jt ? 190 : 40, Math.floor((K.h - ut) / 2));
                    tt.x = Math.max(dt, Math.min(Lt, ee)), tt.y = Math.max(Bt, Math.min($t, me)), tt.__trueosInitialPositionSeeded = !0;
                }
                tt.x = Math.max(dt, Math.min(Lt, tt.x)), tt.y = Math.max(Bt, Math.min($t, tt.y)), qt(G, tt.x, tt.y);
            }
            var L = rt + G.position.x, f = ot + G.position.y;
            if (m.kind === "block") {
                X("render:draw:".concat(et, ":block:").concat(m.tagName, ":begin"));
                var tt = Y;
                (m.tagName === "h1" || m.tagName === "h2" || m.tagName === "h3" || m.tagName === "summary" || m.tagName === "th") && (tt = { bold: !0 }), X("render:draw:".concat(et, ":graphics"));
                var lt = At(G, "__g");
                X("render:draw:".concat(et, ":graphics-clear")), kt(lt), X("render:draw:".concat(et, ":graphics-ensure")), Oi(G, lt, 0), lt.zIndex = -10;
                var ut = Math.max(0, m.width), dt = Math.max(0, m.height), Bt = null;
                if ((m.tagName === "h1" || m.tagName === "h2" || m.tagName === "h3") && (qt(G, Math.round(m.x), Math.round(m.y)), ut = Math.round(ut), dt = Math.round(dt)), X("render:draw:".concat(et, ":widget:").concat(m.tagName)), m.tagName === "hr")
                    pr({ graphics: lt, w: ut, theme: r });
                else if (m.tagName !== "barrow") {
                    if (m.tagName !== "searchrow") {
                        if (m.tagName === "searchbutton")
                            Zr({ node: m, container: G, graphics: lt, w: ut, h: dt, theme: r, uiState: l, getPointerId: Wt, focusInputKey: (v = m.attrs) == null ? void 0 : v["data-focus-key"], requestPaint: ct });
                        else if (m.tagName === "progress" || m.tagName === "meter")
                            sr({ node: m, graphics: lt, w: ut, h: dt, theme: r });
                        else if (m.tagName === "sliderlabel")
                            ur({ node: m, container: G, theme: r, sliderStates: l.sliders });
                        else if (m.tagName === "slider")
                            en({ node: m, container: G, graphics: lt, w: ut, h: dt, absX: L, absY: f, theme: r, sliderStates: l.sliders, sliderBounds: l.sliderBounds, sliderDrags: l.sliderDrags, requestPaint: ct, getPointerId: Wt });
                        else if (m.tagName === "timeinput" || m.tagName === "dateinput" || m.tagName === "monthinput" || m.tagName === "weekinput" || m.tagName === "datetimelocalinput")
                            di({ node: m, container: G, graphics: lt, w: ut, h: dt, absX: L, absY: f, theme: r, uiState: l, getPointerId: Wt, getCursorColor: Vt, temporalStates: l.temporals, yearSliderOwners: l.temporalYearOwners, getOrInitInputValue: function (F, it) { return fn(F, it); }, requestPaint: ct, popupSink: p });
                        else if (m.tagName === "input") {
                            var F = m.key, it = F != null ? S(F) : null, It = F != null && l.focusedKeyByPointer.get(l.keyboardOwnerPointerId) === F, mt = F == null ? null : It ? l.keyboardOwnerPointerId : c.has(F) ? M(F) : null, vt = mt != null, Gt = it != null ? Vt(it) : null;
                            Kr({ node: m, container: G, graphics: lt, w: ut, h: dt, absX: L, absY: f, theme: r, textMeasure: _, uiState: l, getOrInitInputState: fn, clamp: g, radioGroups: d, textDrags: l.textDrags, requestPaint: ct, showCaret: vt, caretPointerId: mt, focusColor: Gt != null ? Gt : void 0, getCursorColor: Vt, getPointerId: Wt });
                        }
                        else if (m.tagName === "textarea") {
                            var F = m.key, it = F != null ? S(F) : null, It = F != null && l.focusedKeyByPointer.get(l.keyboardOwnerPointerId) === F, mt = F == null ? null : It ? l.keyboardOwnerPointerId : c.has(F) ? M(F) : null, vt = mt != null, Gt = it != null ? Vt(it) : null;
                            jr({ node: m, container: G, graphics: lt, w: ut, h: dt, absX: L, absY: f, theme: r, textMeasure: _, uiState: l, getOrInitInputState: fn, clamp: g, textDrags: l.textDrags, requestPaint: ct, showCaret: vt, caretPointerId: mt, focusColor: Gt != null ? Gt : void 0, getCursorColor: Vt, getPointerId: Wt });
                        }
                        else if (m.tagName === "select") {
                            if (m.key) {
                                var F = Number((C = (T = m.attrs) == null ? void 0 : T["data-selected-index"]) != null ? C : "0");
                                de(l.selects, m.key, Number.isFinite(F) ? F : 0);
                            }
                            an({ node: m, container: G, graphics: lt, w: ut, h: dt, absX: L, absY: f, theme: r, selectStates: l.selects, uiState: l, getPointerId: Wt, getCursorColor: Vt, requestPaint: ct, popupSink: h });
                        }
                        else if (m.tagName === "summary")
                            m.key && l.hoverRects.push({ key: m.key, kind: "summary", cursor: "pointer", x: L, y: f, w: ut, h: dt }), dr({ node: m, container: G, w: ut, h: dt, theme: r, detailsOpen: l.detailsOpen, requestRerender: pn });
                        else if (m.tagName === "dialog")
                            ti({ node: m, container: G, w: ut, h: dt, theme: r, selectedBy: l.dialogSelectedBy, getCursorColor: Vt, dialogStates: l.dialogs, dialogDrags: l.dialogDrags, bringToFront: function (F) { l.dialogZ.set(F, l.dialogZCounter++); }, requestPaint: ct, getPointerId: Wt });
                        else if (m.tagName === "img")
                            Ar({ node: m, container: G, graphics: lt, w: ut, h: dt, theme: r, requestRerender: pn });
                        else if (m.tagName === "svg") {
                            var F = (A = (P = m.attrs) == null ? void 0 : P["data-svg"]) != null ? A : "";
                            Wr({ svgMarkup: F, container: G, w: ut, h: dt, requestRerender: pn });
                        }
                        else if (m.tagName === "canvas")
                            Br({ node: m, container: G, graphics: lt, w: ut, h: dt, theme: r });
                        else if (m.tagName === "iframe")
                            Xr({ node: m, container: G, graphics: lt, w: ut, h: dt, theme: r });
                        else if (m.tagName === "color")
                            l.color.bounds = { x: L, y: f, w: Math.max(0, ut), h: Math.max(0, dt) }, si({ node: m, container: G, graphics: lt, w: ut, h: dt, theme: r, rgb: l.color.rgb, setRgb: function (F) { l.color.rgb = F; }, alpha: l.color.a, setAlpha: function (F) { l.color.a = Math.max(0, Math.min(255, Math.round(F))); }, pick: l.color.pick, setPick: function (F) { l.color.pick = F; }, requestPaint: ct, getPointerId: Wt, setDraggingPointerId: function (F) { l.color.draggingPointerId = F; } });
                        else if (m.tagName === "number") {
                            var F_1 = m.key, it_1 = String((J = (N = m.attrs) == null ? void 0 : N.channel) != null ? J : "").toLowerCase(), It_1 = it_1 === "r" || it_1 === "g" || it_1 === "b" || it_1 === "a";
                            F_1 && ni({ node: m, container: G, graphics: lt, w: ut, h: dt, theme: r, getValue: function () { var mt, vt; return It_1 ? it_1 === "a" ? (mt = l.color.a) != null ? mt : 255 : (vt = l.color.rgb[it_1]) != null ? vt : 0 : Dn(l.numbers, F_1, m.attrs).value; }, setValue: function (mt) { It_1 ? it_1 === "a" ? l.color.a = Math.max(0, Math.min(255, Math.round(mt))) : l.color.rgb[it_1] = Math.max(0, Math.min(255, Math.round(mt))) : Dn(l.numbers, F_1, m.attrs).value = mt; }, requestPaint: ct, numberHolds: l.numberHolds, getPointerId: Wt });
                        }
                        else if (m.tagName === "button") {
                            var F = he(Tn(m));
                            m.key && l.hoverRects.push({ key: m.key, kind: "button", cursor: "pointer", x: L, y: f, w: ut, h: dt }), br({ container: G, graphics: lt, w: ut, h: dt, label: Pt() ? "" : F, theme: r, registerHoverHandlers: m.key ? function (it) { l.hoverHandlers.set(m.key, it); } : void 0 });
                        }
                        else if (!An(m.tagName))
                            if (m.tagName === "table")
                                yr({ graphics: lt, w: ut, h: dt, boxBorder: r.boxBorder });
                            else if (m.tagName === "td" || m.tagName === "th")
                                xr({ nodeTag: m.tagName, graphics: lt, w: ut, h: dt, theme: r });
                            else {
                                var F = Math.max(0, Math.round(ut)), it = Math.max(0, Math.round(dt));
                                lt.rect(0, 0, F, it), lt.stroke({ width: 1, color: r.boxBorder, alignment: 0 });
                            }
                    }
                }
                X("render:draw:".concat(et, ":overlay-label")), Bt && G.addChild(Bt);
                var Lt = null, $t = null, Jt = m.tagName === "iframe" && String((U = (z = m.attrs) == null ? void 0 : z["data-root"]) != null ? U : "") === "1";
                if (m.tagName === "iframe" && !Jt) {
                    m.key && l.iframeRects.push({ key: m.key, x: L, y: f, w: Math.max(0, ut), h: Math.max(0, dt) }), Lt = le(G, "__iframeContentRoot"), qt(Lt, 0, 0);
                    var mt = At(G, "__iframeContentMask");
                    kt(mt);
                    var vt = 0, Gt = 34, pe = Math.max(0, ut), En = Math.max(0, dt - 34);
                    mt.rect(vt, Gt, pe, En), mt.fill(16777215), mt.alpha = 0, Lt.mask = mt;
                    var ie_1 = (Q = m.key) != null ? Q : "", ft_1 = (nt = l.iframeScroll.get(ie_1)) != null ? nt : { y: 0, contentHeight: 0, viewportHeight: 0, draggingPointerId: null, dragOffsetY: 0, track: { x: 0, y: 0, w: Te, h: 0 }, thumb: { x: 0, y: 0, w: Te, h: 0 }, rect: { x: L, y: f, w: Math.max(0, ut), h: Math.max(0, dt) } };
                    ft_1.rect = { x: L, y: f, w: Math.max(0, ut), h: Math.max(0, dt) }, ft_1.contentHeight = I(m), ft_1.viewportHeight = Math.max(0, dt - 34 - 8);
                    var Ee_1 = Math.max(0, ft_1.contentHeight - ft_1.viewportHeight);
                    ft_1.y = Math.max(0, Math.min(ft_1.y, Ee_1)), $t = le(Lt, "__iframeScrollRoot"), qt($t, 0, -ft_1.y), ie_1 && l.iframeScrollRoots.set(ie_1, $t);
                    var oe = At(G, "__iframeScrollbar");
                    ie_1 && l.iframeScrollbarGraphics.set(ie_1, oe), kt(oe), oe.eventMode = "static";
                    var In = gn, ge = Te, Ve = Math.max(0, ut - ge - In), Mn = 34 + In, Ge = Math.max(0, dt - 34 - In * 2), Yn = Ee_1 > .5 && Ge > 1;
                    if (oe.visible = Yn, Yn) {
                        var Sn = Math.max(24, (ft_1.viewportHeight || 1) / Math.max(1, ft_1.contentHeight) * Ge), Bi = Math.max(1, Ge - Sn), Ui = Ee_1 <= 0 ? 0 : ft_1.y / Ee_1, Kn = Mn + Bi * Ui;
                        ft_1.track = { x: L + Ve, y: f + Mn, w: ge, h: Ge }, ft_1.thumb = { x: L + Ve, y: f + Kn, w: ge, h: Sn }, oe.rect(Ve, Mn, ge, Ge), oe.fill({ color: 0, alpha: .06 }), oe.rect(Ve, Kn, ge, Sn), oe.fill({ color: 0, alpha: .25 }), oe.on("pointerdown", function (Qt) { var Vn, Jn, Qn, Zn, qn, tr; if ((Qt == null ? void 0 : Qt.button) === 2)
                            return; var Pn = Wt(Qt); if (Pn <= 0)
                            return; var Je = (Jn = (Vn = Qt.global) == null ? void 0 : Vn.x) != null ? Jn : 0, be = (Zn = (Qn = Qt.global) == null ? void 0 : Qn.y) != null ? Zn : 0; if (!(Je >= ft_1.track.x && Je <= ft_1.track.x + ft_1.track.w && be >= ft_1.track.y && be <= ft_1.track.y + ft_1.track.h))
                            return; if (Je >= ft_1.thumb.x && Je <= ft_1.thumb.x + ft_1.thumb.w && be >= ft_1.thumb.y && be <= ft_1.thumb.y + ft_1.thumb.h) {
                            ft_1.draggingPointerId = Pn, ft_1.dragOffsetY = be - ft_1.thumb.y, l.iframeScroll.set(ie_1, ft_1), (qn = Qt.stopPropagation) == null || qn.call(Qt);
                            return;
                        } var zn = Math.max(1, ft_1.track.h - ft_1.thumb.h), jn = Math.max(ft_1.track.y, Math.min(ft_1.track.y + zn, be - ft_1.thumb.h / 2)), Xi = (jn - ft_1.track.y) / zn; ft_1.y = Math.max(0, Math.min(Ee_1, Xi * Ee_1)), ft_1.draggingPointerId = Pn, ft_1.dragOffsetY = be - jn, l.iframeScroll.set(ie_1, ft_1), Pt() && (window.__TRUEOS_PIXI_SCROLL_REPAINT_REQUIRED__ = !0, window.__TRUEOS_PIXI_SCROLL_REPAINT_OWNER__ = "iframe", window.__TRUEOS_PIXI_SCROLL_REPAINT_IFRAME_KEY__ = ie_1), ct == null || ct(), (tr = Qt.stopPropagation) == null || tr.call(Qt); });
                    }
                    else
                        ft_1.track = { x: 0, y: 0, w: ge, h: 0 }, ft_1.thumb = { x: 0, y: 0, w: ge, h: 0 };
                    l.iframeScroll.set(ie_1, ft_1);
                }
                var ee = [], me = m.tagName === "dialog" || m.tagName === "iframe" && !Jt ? ee : q, fe = K;
                if (m.tagName === "dialog")
                    fe = { x: 0, y: 0, w: Math.max(0, ut), h: Math.max(0, dt) };
                else if (m.tagName === "iframe" && !Jt) {
                    var F = (ht = m.key) != null ? ht : "", it = l.iframeScroll.get(F), It = it ? it.y : 0, mt = 34;
                    fe = { x: 0, y: mt + It, w: Math.max(0, ut), h: Math.max(0, dt - mt) };
                }
                var De = (yt = $t != null ? $t : Lt) != null ? yt : at, Le = L + ((Tt = Lt == null ? void 0 : Lt.position.x) != null ? Tt : 0), ve = f + ((Ot = Lt == null ? void 0 : Lt.position.y) != null ? Ot : 0) + ((Rt = $t == null ? void 0 : $t.position.y) != null ? Rt : 0);
                X("render:draw:".concat(et, ":children"));
                var St = 0;
                for (var F = 0; F < ((Et = m.children) != null ? Et : []).length; F++) {
                    var it = ((Ht = m.children) != null ? Ht : [])[F];
                    if (it.kind === "block" && it.tagName === "dialog")
                        me.push(it);
                    else {
                        if (m.tagName === "button" && it.kind === "text" && !Pt())
                            continue;
                        B(it, De, tt, Le, ve, me, fe, "".concat(et, ".").concat(F), St++);
                    }
                }
                if ((m.tagName === "dialog" || m.tagName === "iframe" && !Jt) && ee.length > 0) {
                    ee.sort(function (F, it) { var vt, Gt; var It = F.key && (vt = l.dialogZ.get(F.key)) != null ? vt : 0, mt = it.key && (Gt = l.dialogZ.get(it.key)) != null ? Gt : 0; return It - mt; });
                    try {
                        for (var ee_1 = __values(ee), ee_1_1 = ee_1.next(); !ee_1_1.done; ee_1_1 = ee_1.next()) {
                            var F = ee_1_1.value;
                            var it = F.key && F.key.length > 0 ? F.key : "".concat(et, ".dlg.").concat(St);
                            B(F, De, tt, Le, ve, ee, fe, "".concat(et, ".dlg.").concat(it), St++);
                        }
                    }
                    catch (e_58_1) { e_58 = { error: e_58_1 }; }
                    finally {
                        try {
                            if (ee_1_1 && !ee_1_1.done && (_a = ee_1.return)) _a.call(ee_1);
                        }
                        finally { if (e_58) throw e_58.error; }
                    }
                }
            }
            else {
                X("render:draw:".concat(et, ":text:begin"));
                var tt = Nt(G, "__text", function (lt) { lt.style = { fontFamily: r.fontFamily, fontSize: r.fontSize, fill: r.text, fontWeight: Y.bold ? "700" : "400", wordWrap: !0, wordWrapWidth: 0 }; });
                tt.text = (Ct = m.text) != null ? Ct : "", tt.style.fontFamily = r.fontFamily, tt.style.fontSize = r.fontSize, tt.style.fill = r.text, tt.style.fontWeight = Y.bold ? "700" : "400", tt.style.wordWrap = !0, tt.style.wordWrapWidth = Math.max(0, Math.ceil(m.width) + Me), qt(tt, 0, _t), X("render:draw:".concat(et, ":text:done"));
            }
        }
        X("render:root-loop");
        var $ = { bold: !1 }, w = { x: 0, y: 0, w: t.renderer.width, h: t.renderer.height }, R = [], E = s.position, x = E && Number(E.y || 0) || 0, W = 0;
        for (var m = 0; m < e.children.length; m++) {
            X("render:root-loop:".concat(m));
            var D = e.children[m];
            D && (D.kind === "block" && D.tagName === "dialog" ? R.push(D) : (X("render:root-loop:".concat(m, ":dispatch")), B(D, s, $, 0, x, R, w, "root.".concat(m), W++)));
        }
        if (X("render:root-dialogs"), R.length > 0) {
            R.sort(function (D, Y) { var q, K; var rt = D.key && (q = l.dialogZ.get(D.key)) != null ? q : 0, ot = Y.key && (K = l.dialogZ.get(Y.key)) != null ? K : 0; return rt - ot; });
            var m = 0;
            try {
                for (var R_1 = __values(R), R_1_1 = R_1.next(); !R_1_1.done; R_1_1 = R_1.next()) {
                    var D = R_1_1.value;
                    var Y = D.key && D.key.length > 0 ? D.key : "rootdlg.".concat(m);
                    B(D, u, $, 0, 0, R, w, "dlg.".concat(Y), m++);
                }
            }
            catch (e_50_1) { e_50 = { error: e_50_1 }; }
            finally {
                try {
                    if (R_1_1 && !R_1_1.done && (_b = R_1.return)) _b.call(R_1);
                }
                finally { if (e_50) throw e_50.error; }
            }
        }
        if (X("render:temporal-popups"), p.length > 0 && hi({ popups: p, stage: a, theme: r, viewportW: t.renderer.width, viewportH: t.renderer.height, temporalStates: l.temporals, getOrInitInputValue: function (m, D) { return fn(m, D); }, sliders: l.sliders, sliderBounds: l.sliderBounds, sliderDrags: l.sliderDrags, selects: l.selects, selectPopups: h, uiFocus: l, getPointerId: Wt, getCursorColor: Vt, requestPaint: ct }), X("render:select-popups"), h.length > 0)
            try {
                for (var h_2 = __values(h), h_2_1 = h_2.next(); !h_2_1.done; h_2_1 = h_2.next()) {
                    var m = h_2_1.value;
                    li({ popup: m, stage: a, theme: r, selectStates: l.selects, uiState: l, getPointerId: Wt, requestPaint: ct, viewportW: t.renderer.width, viewportH: t.renderer.height });
                }
            }
            catch (e_51_1) { e_51 = { error: e_51_1 }; }
            finally {
                try {
                    if (h_2_1 && !h_2_1.done && (_c = h_2.return)) _c.call(h_2);
                }
                finally { if (e_51) throw e_51.error; }
            }
        X("render:context-menus");
        var _loop_3 = function (m, D) {
            if (!(D != null && D.open))
                return "continue";
            var Y = new xt;
            Y.eventMode = "static", Y.cursor = "default", qt(Y, D.x, D.y);
            var rt = 140, ot = 28, q = 6, K = ["Copy", "Paste", "Close"], et = new bt;
            et.rect(0, 0, rt + q * 2, K.length * ot + q * 2), et.fill(16777215);
            var k = 1;
            et.rect(k, k, rt + q * 2 - k * 2, K.length * ot + q * 2 - k * 2), et.stroke({ width: 2, color: Vt(m), alignment: 0 }), Y.addChild(et), K.forEach(function (H, Z) { var G = q + Z * ot, at = new xt; at.eventMode = "static", at.cursor = "pointer", qt(at, q, G); var L = new bt; L.rect(0, 0, rt, ot), L.fill(16777215), at.addChild(L); var f = zt({ text: H, fontFamily: r.fontFamily, fontSize: r.fontSize, fill: r.text }); qt(f, 8, Math.max(0, (ot - f.height) / 2) + _t), at.addChild(f); var O = function (T) { return Wt(T) === m; }, v = function (T) { if (!Pt())
                return; var C = window.__pixiCapture, P = C && typeof C.objectId == "function" ? C.objectId.bind(C) : null; P && (window.__TRUEOS_PIXI_LAST_GRAPHICS_FAST_PATH__ = { owner: "context-menu-hover", rootNode: P(t.stage), graphicsNode: P(L), x: 0, y: 0, w: rt, h: ot, fillColor: T, fillAlpha: 1 }); }; at.on("pointerover", function (T) { O(T) && (L.clear(), L.rect(0, 0, rt, ot), L.fill(15921906), v(15921906)); }), at.on("pointerout", function (T) { O(T) && (L.clear(), L.rect(0, 0, rt, ot), L.fill(16777215), v(16777215)); }), at.on("pointerdown", function (T) { var J, z, U, Q, nt, ht, yt, Tt, Ot, Rt, Et; if (!O(T))
                return; (J = T.stopPropagation) == null || J.call(T); var C = (z = l.focusedKeyByPointer.get(m)) != null ? z : null, P = C ? l.inputs.get(C) : null, A = C != null && l.fieldBounds.has(C) && P != null && typeof P.value == "string"; if (H === "Copy" && A) {
                var Ht = P, Ct = (U = Ht.value) != null ? U : "", tt = (nt = (Q = Ht.selections) == null ? void 0 : Q.get(m)) != null ? nt : null, lt = tt ? Math.max(0, Math.min(Ct.length, (ht = tt.start) != null ? ht : 0)) : 0, ut = tt ? Math.max(0, Math.min(Ct.length, (yt = tt.end) != null ? yt : lt)) : lt, dt = Math.min(lt, ut), Bt = Math.max(lt, ut), Lt = dt !== Bt ? Ct.slice(dt, Bt) : Ct;
                l.clipboards.set(m, Lt);
            }
            else if (H === "Paste" && A) {
                var Ht = (Tt = l.clipboards.get(m)) != null ? Tt : "";
                if (Ht.length > 0) {
                    var Ct = P, tt = (Ot = Ct.value) != null ? Ot : "";
                    if (Ct.selections || (Ct.selections = new Map), !Ct.selections.has(m)) {
                        var Jt = tt.length;
                        Ct.selections.set(m, { start: Jt, end: Jt });
                    }
                    var lt = Ct.selections.get(m), ut = Math.max(0, Math.min(tt.length, (Rt = lt.start) != null ? Rt : tt.length)), dt = Math.max(0, Math.min(tt.length, (Et = lt.end) != null ? Et : ut)), Bt = Math.min(ut, dt), Lt = Math.max(ut, dt);
                    Ct.value = tt.slice(0, Bt) + Ht + tt.slice(Lt);
                    var $t = Bt + Ht.length;
                    lt.start = $t, lt.end = $t;
                }
            } var N = l.contextMenus.get(m); N && (N.open = !1, l.contextMenus.set(m, N)), ct == null || ct(); }), Y.addChild(at); }), a.addChild(Y);
        };
        try {
            for (var _j = __values(l.contextMenus.entries()), _k = _j.next(); !_k.done; _k = _j.next()) {
                var _l = __read(_k.value, 2), m = _l[0], D = _l[1];
                _loop_3(m, D);
            }
        }
        catch (e_52_1) { e_52 = { error: e_52_1 }; }
        finally {
            try {
                if (_k && !_k.done && (_d = _j.return)) _d.call(_j);
            }
            finally { if (e_52) throw e_52.error; }
        }
        X("render:prune-cache");
        try {
            for (var _m = __values(b.entries()), _p = _m.next(); !_p.done; _p = _m.next()) {
                var _q = __read(_p.value, 2), m = _q[0], D = _q[1];
                if (!y.has(m)) {
                    try {
                        D.removeFromParent(), (V = D.destroy) == null || V.call(D, { children: !0 });
                    }
                    catch (Y) { }
                    b.delete(m);
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
        X("render:done");
    }
    function ks() {
        return Qe(this, null, function () {
            var t, e, n, r, _a, i_1, _b, o_3, s, u, a_2, h, p_1, d_1, b_1, y_1, c_1, _1, g_2, M, S, _c, B, $_1, w_2, R, E_1, x_1, W_1, j_1, V_1, m_2, D_2, Y_3, rt_1, ot_2, q_3, K_1, et_1, k_4, H, Z, G_2, at_2, f, O, v, L_2, n_3, r;
            return __generator(this, function (_d) {
                switch (_d.label) {
                    case 0:
                        _d.trys.push([0, 9, , 10]);
                        wt("main:start");
                        n = (t = document.getElementById("app")) != null ? t : document.body, r = !0;
                        wt("main:yoga");
                        if (!r) return [3 /*break*/, 1];
                        _a = ys();
                        return [3 /*break*/, 3];
                    case 1: return [4 /*yield*/, Promise.resolve().then(function () { return (Ei(), Ti); })];
                    case 2:
                        _a = (_d.sent()).default;
                        _d.label = 3;
                    case 3:
                        st = _a, wt("main:create-app");
                        i_1 = r ? _s() : new tn;
                        _b = r;
                        if (_b) return [3 /*break*/, 5];
                        return [4 /*yield*/, i_1.init({ background: "#ffffff", resizeTo: window, antialias: !1, preference: "webgl" })];
                    case 4:
                        _b = (_d.sent());
                        _d.label = 5;
                    case 5:
                        _b, wt("main:attach-capture"), wi(i_1), window.__TRUEOS_PIXI_APP = i_1, wt("main:append-canvas"), n.appendChild(i_1.canvas), i_1.ticker.stop(), wt("main:capture-flags"), r && (l.harness.enabled = !1, l.virtualCursor.enabled = !1, window.__pixiCapture && (window.__pixiCapture.persist = !1)), wt("main:canvas-listeners"), i_1.canvas.addEventListener("contextmenu", function (f) { return f.preventDefault(); }), i_1.canvas.addEventListener("wheel", function (f) { var J, z; var O = (J = f.offsetX) != null ? J : 0, v = (z = f.offsetY) != null ? z : 0, T = function (U) { var ht; if (!Pt())
                            return; var Q = window, nt = Number((ht = Q.__TRUEOS_WHEEL_ROUTE_LOG_COUNT__) != null ? ht : 0) || 0; nt >= 32 || (Q.__TRUEOS_WHEEL_ROUTE_LOG_COUNT__ = nt + 1, console.log("[trueos pixi widgets] wheel-route ".concat(U))); }, C = null; for (var U = l.iframeRects.length - 1; U >= 0; U--) {
                            var Q = l.iframeRects[U];
                            if (O >= Q.x && O <= Q.x + Q.w && v >= Q.y && v <= Q.y + Q.h) {
                                C = Q.key;
                                break;
                            }
                        } var P = !1; if (C) {
                            var U = l.iframeScroll.get(C);
                            if (U) {
                                var Q = Math.max(0, U.contentHeight - U.viewportHeight);
                                if (T("hit=iframe x=".concat(Math.round(O), " y=").concat(Math.round(v), " delta=").concat(Math.round(f.deltaY), " y0=").concat(Math.round(U.y), " max=").concat(Math.round(Q))), Q > 0) {
                                    var nt = Math.max(0, Math.min(Q, U.y + f.deltaY));
                                    nt !== U.y && (U.y = nt, l.iframeScroll.set(C, U), Pt() && (window.__TRUEOS_PIXI_SCROLL_REPAINT_REQUIRED__ = !0, window.__TRUEOS_PIXI_SCROLL_REPAINT_OWNER__ = "iframe", window.__TRUEOS_PIXI_SCROLL_REPAINT_IFRAME_KEY__ = C), ct == null || ct(), f.preventDefault(), P = !0, T("owner=iframe y1=".concat(Math.round(nt), " repaint=1")));
                                }
                            }
                        } if (P)
                            return; var A = Math.max(0, l.scroll.contentHeight - l.scroll.viewportHeight); if (A <= 0) {
                            T("owner=none x=".concat(Math.round(O), " y=").concat(Math.round(v), " delta=").concat(Math.round(f.deltaY), " root_y=").concat(Math.round(l.scroll.y), " root_max=0"));
                            return;
                        } var N = Math.max(0, Math.min(A, l.scroll.y + f.deltaY)); if (N !== l.scroll.y) {
                            var U = l.scroll.y;
                            l.scroll.y = N, Pt() && (window.__TRUEOS_PIXI_SCROLL_REPAINT_REQUIRED__ = !0, window.__TRUEOS_PIXI_SCROLL_REPAINT_OWNER__ = "root", window.__TRUEOS_PIXI_SCROLL_REPAINT_IFRAME_KEY__ = ""), ct == null || ct(), f.preventDefault(), T("owner=root x=".concat(Math.round(O), " y=").concat(Math.round(v), " delta=").concat(Math.round(f.deltaY), " y0=").concat(Math.round(U), " y1=").concat(Math.round(N), " max=").concat(Math.round(A), " repaint=1"));
                        }
                        else
                            T("owner=root-boundary x=".concat(Math.round(O), " y=").concat(Math.round(v), " delta=").concat(Math.round(f.deltaY), " y0=").concat(Math.round(l.scroll.y), " max=").concat(Math.round(A))); }, { passive: !1 }), wt("main:stage:eventMode"), i_1.stage.eventMode = "static", wt("main:stage:hitArea"), i_1.stage.hitArea = i_1.screen, wt("main:stage:on:pointerdown"), i_1.stage.on("pointerdown", function (f) {
                            var e_59, _a;
                            var O, v, T, C, P, A;
                            if ((f == null ? void 0 : f.button) === 2) {
                                var N = Wt(f);
                                if (N > 0) {
                                    var J = (O = l.contextMenus.get(N)) != null ? O : { open: !1, x: 0, y: 0 };
                                    J.open = !0, J.x = (T = (v = f.global) == null ? void 0 : v.x) != null ? T : 0, J.y = (P = (C = f.global) == null ? void 0 : C.y) != null ? P : 0, l.contextMenus.set(N, J);
                                }
                                ct == null || ct(), (A = f.preventDefault) == null || A.call(f);
                                return;
                            }
                            if ((f == null ? void 0 : f.button) !== 2) {
                                var N = Wt(f), J = N > 0 ? l.contextMenus.get(N) : null;
                                J && J.open && (J.open = !1, l.contextMenus.set(N, J), ct == null || ct());
                            }
                            if ((f == null ? void 0 : f.button) !== 2) {
                                var N = !1;
                                try {
                                    for (var _b = __values(l.selects.values()), _c = _b.next(); !_c.done; _c = _b.next()) {
                                        var J = _c.value;
                                        J.open && (J.open = !1, N = !0);
                                    }
                                }
                                catch (e_59_1) { e_59 = { error: e_59_1 }; }
                                finally {
                                    try {
                                        if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
                                    }
                                    finally { if (e_59) throw e_59.error; }
                                }
                                N && (ct == null || ct());
                            }
                            (f == null ? void 0 : f.button) !== 2 && mi(l.temporals) && (ct == null || ct()), k_4();
                        }), wt("main:stage:done"), wt("main:roots");
                        o_3 = new xt, s = new xt;
                        s.eventMode = "static";
                        u = new xt;
                        u.eventMode = "none", i_1.stage.addChild(o_3), i_1.stage.addChild(s), i_1.stage.addChild(u);
                        a_2 = new bt;
                        a_2.label = "__trueosGlobalScrollbar", a_2.eventMode = "static", s.addChild(a_2);
                        h = function (f, O) { f.clear(); var v = O.half, T = O.strokeWidth, C = O.color; f.moveTo(-v, 0), f.lineTo(v, 0), f.stroke({ width: T, color: C }), f.moveTo(0, -v), f.lineTo(0, v), f.stroke({ width: T, color: C }); }, p_1 = new bt;
                        p_1.eventMode = "none", p_1.visible = !1, u.addChild(p_1);
                        d_1 = new bt;
                        d_1.eventMode = "none", d_1.visible = !1, u.addChild(d_1);
                        b_1 = new bt;
                        b_1.eventMode = "none", b_1.visible = !1, u.addChild(b_1);
                        y_1 = new bt;
                        y_1.eventMode = "none", u.addChild(y_1), wt("main:text-measure");
                        c_1 = document.createElement("canvas").getContext("2d");
                        if (!c_1)
                            throw new Error("2D canvas not available");
                        c_1.font = "".concat(ye.fontSize, "px ").concat(ye.fontFamily);
                        _1 = function (f) { return c_1.measureText(f).width; }, g_2 = ye.fontSize * 1.25;
                        wt("main:html");
                        if (!(typeof window.__TRUEOS_INPUT_HTML__ == "string")) return [3 /*break*/, 6];
                        _c = window.__TRUEOS_INPUT_HTML__;
                        return [3 /*break*/, 8];
                    case 6: return [4 /*yield*/, fetch("/input.html").then(function (f) { return f.text(); })];
                    case 7:
                        _c = _d.sent();
                        _d.label = 8;
                    case 8:
                        M = _c, S = es(M) ? M : "";
                        Pt() && console.log("[trueos pixi widgets] input-html chars=".concat(M.length, " usable=").concat(S ? 1 : 0, " sample=\"").concat(bn(M), "\"")), wt("main:render-tree"), Ci.clear();
                        B = us(S), $_1 = ds(), w_2 = Es($_1.tree, B.rows), R = Is(w_2, B.rows);
                        if (Pt() && (console.log("[trueos pixi widgets] text-fallback source=".concat(B.source, " rows=").concat(B.rows.length, " samples=").concat(is(B.rows))), console.log("[trueos pixi widgets] render-tree source=".concat($_1.source, " nodes=").concat(w_2.length, " trusted_text_applied=").concat(R))), w_2.length === 0)
                            throw new Error("TrueSurfer widget render tree is missing");
                        E_1 = qo(w_2), x_1 = null, W_1 = { nodes: 0, blocks: 0, text: 0, maxDepth: 0, tags: {} }, j_1 = { hash: "", renderHash: "", layoutHash: "", bytes: 0 }, V_1 = 0, m_2 = function () { var f = Math.max(0, l.scroll.contentHeight - l.scroll.viewportHeight); l.scroll.y = Math.max(0, Math.min(l.scroll.y, f)); }, D_2 = function () { var f = i_1.renderer.width, O = i_1.renderer.height; l.scroll.viewportHeight = O; var v = l.scroll.contentHeight, T = Math.max(0, v - O), C = T > .5; if (a_2.clear(), a_2.visible = C, !C) {
                            l.scroll.track = { x: 0, y: 0, w: l.scroll.track.w, h: 0 }, l.scroll.thumb = { x: 0, y: 0, w: l.scroll.thumb.w, h: 0 };
                            return;
                        } var P = gn, A = Te, N = Math.max(0, f - A - P), J = P, z = Math.max(0, O - P * 2), Q = Math.max(24, O / Math.max(O, v) * z), nt = Math.max(1, z - Q), ht = T <= 0 ? 0 : l.scroll.y / T, yt = J + nt * ht; l.scroll.track = { x: N, y: J, w: A, h: z }, l.scroll.thumb = { x: N, y: yt, w: A, h: Q }, a_2.rect(N, J, A, z), a_2.fill({ color: 0, alpha: .06 }), a_2.rect(N, yt, A, Q), a_2.fill({ color: 0, alpha: .25 }); }, Y_3 = function () {
                            var e_60, _a;
                            var O = gn, v = Te;
                            try {
                                for (var _b = __values(l.iframeScrollRoots.entries()), _c = _b.next(); !_c.done; _c = _b.next()) {
                                    var _d = __read(_c.value, 2), T = _d[0], C = _d[1];
                                    var P = l.iframeScroll.get(T);
                                    if (!P)
                                        continue;
                                    var A = Math.max(0, P.contentHeight - P.viewportHeight);
                                    P.y = Math.max(0, Math.min(P.y, A)), qt(C, 0, -P.y);
                                    var N = l.iframeScrollbarGraphics.get(T);
                                    if (!N) {
                                        l.iframeScroll.set(T, P);
                                        continue;
                                    }
                                    var J = Math.max(0, P.rect.w), z = Math.max(0, P.rect.h), U = Math.max(0, J - v - O), Q = 34 + O, nt = Math.max(0, z - 34 - O * 2), ht = A > .5 && nt > 1;
                                    if (N.clear(), N.visible = ht, !ht) {
                                        P.track = { x: 0, y: 0, w: v, h: 0 }, P.thumb = { x: 0, y: 0, w: v, h: 0 }, l.iframeScroll.set(T, P);
                                        continue;
                                    }
                                    var Tt = Math.max(24, (P.viewportHeight || 1) / Math.max(1, P.contentHeight) * nt), Ot = Math.max(1, nt - Tt), Rt = A <= 0 ? 0 : P.y / A, Et = Q + Ot * Rt;
                                    P.track = { x: P.rect.x + U, y: P.rect.y + Q, w: v, h: nt }, P.thumb = { x: P.rect.x + U, y: P.rect.y + Et, w: v, h: Tt }, N.rect(U, Q, v, nt), N.fill({ color: 0, alpha: .06 }), N.rect(U, Et, v, Tt), N.fill({ color: 0, alpha: .25 }), l.iframeScroll.set(T, P);
                                }
                            }
                            catch (e_60_1) { e_60 = { error: e_60_1 }; }
                            finally {
                                try {
                                    if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
                                }
                                finally { if (e_60) throw e_60.error; }
                            }
                        }, rt_1 = function () { if (x_1) {
                            if (re("PIXI_PAINT_STEP", 96, "[trueos pixi widgets] paint-step clamp begin"), wt("main:paint:clamp"), m_2(), re("PIXI_PAINT_STEP", 96, "[trueos pixi widgets] paint-step render-to-pixi begin"), wt("main:paint:render-to-pixi"), Ps(i_1, x_1, o_3), re("PIXI_PAINT_STEP", 96, "[trueos pixi widgets] paint-step render-to-pixi done"), wt("main:paint:scrollbar"), re("PIXI_PAINT_STEP", 96, "[trueos pixi widgets] paint-step scrollbar begin"), D_2(), wt("main:paint:renderer-render"), re("PIXI_PAINT_STEP", 96, "[trueos pixi widgets] paint-step renderer-render begin"), i_1.renderer.render(i_1.stage), re("PIXI_PAINT_STEP", 96, "[trueos pixi widgets] paint-step publish-stats begin"), ki(E_1, W_1, Ii(w_2), Mi(x_1), Si(x_1), j_1), re("PIXI_PAINT_STEP", 96, "[trueos pixi widgets] paint-step publish-stats done"), Pt()) {
                                re("PIXI_PAINT_STEP", 96, "[trueos pixi widgets] paint-step overlays begin");
                                var f = ps(x_1);
                                window.__TRUEOS_PIXI_LAYOUT_TEXT_OVERLAYS__ = f, V_1 < 4 && (V_1 += 1, console.log("[trueos pixi widgets] layout-text-overlays count=".concat(f.length, " samples=").concat(gs(f)))), re("PIXI_PAINT_STEP", 96, "[trueos pixi widgets] paint-step overlays done");
                            }
                            wt("main:paint:done"), re("PIXI_PAINT_STEP", 96, "[trueos pixi widgets] paint-step done");
                        } }, ot_2 = function () { var f = window.__TRUEOS_PIXI_SCROLL_REPAINT_OWNER__ || "", O = window.__TRUEOS_PIXI_SCROLL_REPAINT_IFRAME_KEY__ || ""; wt("main:scroll-paint:clamp"), m_2(), wt("main:scroll-paint:content-position"); var v = le(o_3, "__contentRoot"); if (qt(v, 0, -l.scroll.y), wt("main:scroll-paint:scrollbar"), D_2(), window.__TRUEOS_PIXI_LAST_SCROLL_FAST_PATH__ = null, f === "root") {
                            var T = window.__pixiCapture, C = T && typeof T.objectId == "function" ? T.objectId.bind(T) : null;
                            C && (window.__TRUEOS_PIXI_LAST_SCROLL_FAST_PATH__ = { owner: "root", rootNode: C(i_1.stage), contentNode: C(v), contentY: -l.scroll.y, scrollbarNode: C(a_2), scrollbarVisible: l.scroll.track.h > 0 ? 1 : 0, trackX: l.scroll.track.x, trackY: l.scroll.track.y, trackW: l.scroll.track.w, trackH: l.scroll.track.h, thumbX: l.scroll.thumb.x, thumbY: l.scroll.thumb.y, thumbW: l.scroll.thumb.w, thumbH: l.scroll.thumb.h });
                        } if (wt("main:scroll-paint:iframe-scrollbars"), Y_3(), f === "iframe" && O) {
                            var T = window.__pixiCapture, C = T && typeof T.objectId == "function" ? T.objectId.bind(T) : null, P = l.iframeScrollRoots.get(O), A = l.iframeScrollbarGraphics.get(O), N = l.iframeScroll.get(O);
                            C && P && A && N && (window.__TRUEOS_PIXI_LAST_SCROLL_FAST_PATH__ = { owner: "iframe", rootNode: C(i_1.stage), contentNode: C(P), contentY: -N.y, scrollbarNode: C(A), scrollbarVisible: N.track.h > 0 ? 1 : 0, trackX: N.track.h > 0 ? N.track.x - N.rect.x : 0, trackY: N.track.h > 0 ? N.track.y - N.rect.y : 0, trackW: N.track.w, trackH: N.track.h, thumbX: N.thumb.h > 0 ? N.thumb.x - N.rect.x : 0, thumbY: N.thumb.h > 0 ? N.thumb.y - N.rect.y : 0, thumbW: N.thumb.w, thumbH: N.thumb.h });
                        } wt("main:scroll-paint:renderer-render"), i_1.renderer.render(i_1.stage), ki(E_1, W_1, Ii(w_2), x_1 ? Mi(x_1) : "", x_1 ? Si(x_1) : ""), wt("main:scroll-paint:done"); };
                        Pt() && (window.__TRUEOS_REPAINT_NOW__ = function () { var v; var f = window.__TRUEOS_PIXI_SCROLL_REPAINT_REQUIRED__ === !0; window.__TRUEOS_PIXI_DIRTY__ = !1, window.__TRUEOS_PIXI_REPAINT_REQUIRED__ = !1, window.__TRUEOS_PIXI_SCROLL_REPAINT_REQUIRED__ = !1, f || (window.__TRUEOS_PIXI_LAST_SCROLL_FAST_PATH__ = null), f || (window.__TRUEOS_PIXI_LAST_GRAPHICS_FAST_PATH__ = null); var O = Number((v = window.__TRUEOS_REPAINT_NOW_LOG_COUNT__) != null ? v : 0) || 0; O < 24 && (window.__TRUEOS_REPAINT_NOW_LOG_COUNT__ = O + 1, console.log("[trueos pixi widgets] repaint-now scrollOnly=".concat(f ? 1 : 0, " begin"))), f ? ot_2() : rt_1(), window.__TRUEOS_PIXI_SCROLL_REPAINT_OWNER__ = "", window.__TRUEOS_PIXI_SCROLL_REPAINT_IFRAME_KEY__ = "", O < 24 && console.log("[trueos pixi widgets] repaint-now scrollOnly=".concat(f ? 1 : 0, " done")); });
                        q_3 = function () { wt("main:layout-build"), Mt("[trueos pixi widgets] rerender layout-build begin"); var f = Ss(w_2, window.innerWidth, window.innerHeight); Mt("[trueos pixi widgets] rerender layout-build done"), Mt("[trueos pixi widgets] rerender prepixi begin"), j_1 = jo($_1.source, w_2, f, window.innerWidth, window.innerHeight), Mt("[trueos pixi widgets] rerender prepixi done"), wt("main:layout-commit"), x_1 = f, Pt() && (window.__TRUEOS_PIXI_LAST_LAYOUT__ = f, window.__TRUEOS_PIXI_LAYOUT_TEXT_OVERLAYS__ = []), Mt("[trueos pixi widgets] rerender stats begin"), W_1 = ts(f), Mt("[trueos pixi widgets] rerender stats done"), Mt("[trueos pixi widgets] rerender scroll-height begin"), l.scroll.contentHeight = xs(f), l.scroll.viewportHeight = window.innerHeight, Mt("[trueos pixi widgets] rerender paint begin"), rt_1(), Mt("[trueos pixi widgets] rerender paint done"); };
                        pn = function () { q_3(); };
                        K_1 = !1, et_1 = !1, k_4 = function () { if (Pt()) {
                            window.__TRUEOS_PIXI_DIRTY__ = !0;
                            return;
                        } et_1 || K_1 || (et_1 = !0, requestAnimationFrame(function () { et_1 = !1, i_1.renderer.render(i_1.stage); })); };
                        ct = function () { if (!K_1) {
                            if (Pt()) {
                                window.__TRUEOS_PIXI_DIRTY__ = !0, window.__TRUEOS_PIXI_REPAINT_REQUIRED__ = !0;
                                return;
                            }
                            K_1 = !0, requestAnimationFrame(function () { K_1 = !1, rt_1(); });
                        } }, wt("main:first-rerender"), q_3(), wt("main:cursor-setup");
                        H = 2, Z = 10, G_2 = Pt();
                        h(p_1, { half: Z, strokeWidth: H, color: Vt(Ft) }), h(d_1, { half: Z, strokeWidth: H, color: Vt(Ut) }), h(b_1, { half: Z, strokeWidth: H, color: Vt(Yt) });
                        at_2 = 2;
                        if (h(y_1, { half: Z, strokeWidth: H, color: Vt(at_2) }), l.userCursorPos.set(Ft, { x: i_1.renderer.width * .25, y: i_1.renderer.height * .5 }), l.userCursorPos.set(Ut, { x: i_1.renderer.width * .25 + 40, y: i_1.renderer.height * .5 + 20 }), l.userCursorPos.set(Yt, { x: i_1.renderer.width * .25 + 80, y: i_1.renderer.height * .5 + 40 }), p_1.visible = !G_2, d_1.visible = !G_2, b_1.visible = !G_2, !G_2) {
                            f = l.userCursorPos.get(Ft), O = l.userCursorPos.get(Ut), v = l.userCursorPos.get(Yt);
                            p_1.position.set(f.x, f.y), d_1.position.set(O.x, O.y), b_1.position.set(v.x, v.y);
                        }
                        y_1.visible = !G_2 && l.virtualCursor.enabled;
                        L_2 = function () { if (G_2) {
                            p_1.visible = !1, d_1.visible = !1, b_1.visible = !1, y_1.visible = !1;
                            return;
                        } var f = l.userCursorPos.get(Ft), O = l.userCursorPos.get(Ut), v = l.userCursorPos.get(Yt); f && (p_1.visible = !0, p_1.position.set(f.x, f.y)), O && (d_1.visible = !0, d_1.position.set(O.x, O.y)), v && (b_1.visible = !0, b_1.position.set(v.x, v.y)); var T = function (C, P) { var A = null, N = null; for (var J = l.hoverRects.length - 1; J >= 0; J--) {
                            var z = l.hoverRects[J];
                            if (C >= z.x && C <= z.x + z.w && P >= z.y && P <= z.y + z.h) {
                                A = z.key, N = z.cursor;
                                break;
                            }
                        } return { hitKey: A, hitCursor: N }; }; if (f) {
                            var _a = T(f.x, f.y), C = _a.hitKey, P = _a.hitCursor;
                            l.hoveredKeyByPointer.set(Ft, C), l.hoveredCursorByPointer.set(Ft, P);
                            var A = l.textDrags.has(Ft) || l.sliderDrags.has(Ft) || l.dialogDrags.has(Ft);
                            p_1.rotation = P != null || A ? Math.PI / 4 : 0;
                        } if (O) {
                            var _b = T(O.x, O.y), C = _b.hitKey, P = _b.hitCursor;
                            l.hoveredKeyByPointer.set(Ut, C), l.hoveredCursorByPointer.set(Ut, P);
                            var A = l.textDrags.has(Ut) || l.sliderDrags.has(Ut) || l.dialogDrags.has(Ut);
                            d_1.rotation = P != null || A ? Math.PI / 4 : 0;
                        } if (v) {
                            var _c = T(v.x, v.y), C = _c.hitKey, P = _c.hitCursor;
                            l.hoveredKeyByPointer.set(Yt, C), l.hoveredCursorByPointer.set(Yt, P);
                            var A = l.textDrags.has(Yt) || l.sliderDrags.has(Yt) || l.dialogDrags.has(Yt);
                            b_1.rotation = P != null || A ? Math.PI / 4 : 0;
                        } k_4(); };
                        l.harness.enabled && setInterval(function () {
                            var e_61, _a, e_62, _b;
                            var f = l.harness.activeUserPointerId, O = f === Ft ? Ut : f === Ut ? Yt : Ft;
                            if (l.harness.activeUserPointerId = O, l.lastMouse.has) {
                                var z = l.userCursorPos.get(f), U = l.userCursorPos.get(O);
                                l.userCursorPos.set(O, { x: l.lastMouse.x, y: l.lastMouse.y }), U ? l.userCursorPos.set(f, { x: U.x, y: U.y }) : z && l.userCursorPos.set(f, { x: z.x, y: z.y });
                            }
                            var v = l.textDrags.size > 0, T = l.sliderDrags.size > 0, C = l.dialogDrags.size > 0, P = l.scroll.draggingPointerId != null, A = l.color.draggingPointerId != null, N = !1;
                            try {
                                for (var _c = __values(l.iframeScroll.values()), _d = _c.next(); !_d.done; _d = _c.next()) {
                                    var z = _d.value;
                                    if (z.draggingPointerId != null) {
                                        N = !0;
                                        break;
                                    }
                                }
                            }
                            catch (e_61_1) { e_61 = { error: e_61_1 }; }
                            finally {
                                try {
                                    if (_d && !_d.done && (_a = _c.return)) _a.call(_c);
                                }
                                finally { if (e_61) throw e_61.error; }
                            }
                            var J = v || T || C || P || A || N;
                            l.textDrags.delete(Ft), l.textDrags.delete(Ut), l.textDrags.delete(Yt), l.sliderDrags.delete(Ft), l.sliderDrags.delete(Ut), l.sliderDrags.delete(Yt), l.dialogDrags.delete(Ft), l.dialogDrags.delete(Ut), l.dialogDrags.delete(Yt);
                            try {
                                for (var _f = __values([Ft, Ut, Yt]), _g = _f.next(); !_g.done; _g = _f.next()) {
                                    var z = _g.value;
                                    var U = l.numberHolds.get(z);
                                    U && (U.timeoutId != null && window.clearTimeout(U.timeoutId), U.intervalId != null && window.clearInterval(U.intervalId), l.numberHolds.delete(z));
                                }
                            }
                            catch (e_62_1) { e_62 = { error: e_62_1 }; }
                            finally {
                                try {
                                    if (_g && !_g.done && (_b = _f.return)) _b.call(_f);
                                }
                                finally { if (e_62) throw e_62.error; }
                            }
                            (l.scroll.draggingPointerId === Ft || l.scroll.draggingPointerId === Ut || l.scroll.draggingPointerId === Yt) && (l.scroll.draggingPointerId = null), (l.color.draggingPointerId === Ft || l.color.draggingPointerId === Ut || l.color.draggingPointerId === Yt) && (l.color.draggingPointerId = null), L_2(), J && (ct == null || ct());
                        }, l.harness.periodMs), !G_2 && l.virtualCursor.enabled && i_1.ticker.add(function () { var P, A, N, J, z; var f = Math.max(0, i_1.ticker.deltaMS) / 1e3; y_1.visible = !0, l.virtualCursor.t += f; var O = i_1.renderer.width * .75, v = i_1.renderer.height * .25, T = l.virtualCursor.t * l.virtualCursor.speed, C = l.virtualCursor.radius; l.virtualCursor.x = O + Math.cos(T) * C, l.virtualCursor.y = v + Math.sin(T) * C, y_1.position.set(l.virtualCursor.x, l.virtualCursor.y); {
                            var U = at_2, Q = l.virtualCursor.x, nt = l.virtualCursor.y, ht = null, yt = null;
                            for (var Rt = l.hoverRects.length - 1; Rt >= 0; Rt--) {
                                var Et = l.hoverRects[Rt];
                                if (Q >= Et.x && Q <= Et.x + Et.w && nt >= Et.y && nt <= Et.y + Et.h) {
                                    ht = Et.key, yt = Et.cursor;
                                    break;
                                }
                            }
                            var Tt = (P = l.hoveredKeyByPointer.get(U)) != null ? P : null;
                            Tt !== ht && (Tt && ((N = (A = l.hoverHandlers.get(Tt)) == null ? void 0 : A.out) == null || N.call(A)), ht && ((z = (J = l.hoverHandlers.get(ht)) == null ? void 0 : J.over) == null || z.call(J)), l.hoveredKeyByPointer.set(U, ht)), l.hoveredCursorByPointer.set(U, yt);
                            var Ot = l.textDrags.has(U) || l.sliderDrags.has(U) || l.dialogDrags.has(U);
                            y_1.rotation = yt != null || Ot ? Math.PI / 4 : 0;
                        } }), l.virtualCursor.x = i_1.renderer.width * .75 + l.virtualCursor.radius, l.virtualCursor.y = i_1.renderer.height * .25, y_1.position.set(l.virtualCursor.x, l.virtualCursor.y), Pt() && rt_1(), i_1.stage.on("pointerup", function (f) {
                            var e_63, _a;
                            var T, C, P;
                            var O = Wt(f), v = (C = (T = l.sliderDrags.get(O)) == null ? void 0 : T.key) != null ? C : null;
                            l.textDrags.delete(O), l.sliderDrags.delete(O), l.dialogDrags.delete(O), l.scroll.draggingPointerId === O && (l.scroll.draggingPointerId = null), l.color.draggingPointerId === O && (l.color.draggingPointerId = null);
                            try {
                                for (var _b = __values(l.iframeScroll.values()), _c = _b.next(); !_c.done; _c = _b.next()) {
                                    var A = _c.value;
                                    A.draggingPointerId === O && (A.draggingPointerId = null);
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
                                var A = l.numberHolds.get(O);
                                A && (A.timeoutId != null && window.clearTimeout(A.timeoutId), A.intervalId != null && window.clearInterval(A.intervalId), l.numberHolds.delete(O));
                            }
                            if (v) {
                                var A = (P = l.temporalYearOwners.get(v)) != null ? P : null;
                                if (A) {
                                    var N = l.temporals.get(A);
                                    N && N.openYear && (N.openYear = !1, l.temporals.set(A, N), ct == null || ct());
                                }
                            }
                            k_4();
                        }), i_1.stage.on("pointerupoutside", function (f) {
                            var e_64, _a;
                            var T, C, P;
                            var O = Wt(f), v = (C = (T = l.sliderDrags.get(O)) == null ? void 0 : T.key) != null ? C : null;
                            l.textDrags.delete(O), l.sliderDrags.delete(O), l.dialogDrags.delete(O), l.scroll.draggingPointerId === O && (l.scroll.draggingPointerId = null), l.color.draggingPointerId === O && (l.color.draggingPointerId = null);
                            try {
                                for (var _b = __values(l.iframeScroll.values()), _c = _b.next(); !_c.done; _c = _b.next()) {
                                    var A = _c.value;
                                    A.draggingPointerId === O && (A.draggingPointerId = null);
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
                                var A = l.numberHolds.get(O);
                                A && (A.timeoutId != null && window.clearTimeout(A.timeoutId), A.intervalId != null && window.clearInterval(A.intervalId), l.numberHolds.delete(O));
                            }
                            if (v) {
                                var A = (P = l.temporalYearOwners.get(v)) != null ? P : null;
                                if (A) {
                                    var N = l.temporals.get(A);
                                    N && N.openYear && (N.openYear = !1, l.temporals.set(A, N), ct == null || ct());
                                }
                            }
                            k_4();
                        }), a_2.on("pointerdown", function (f) { var nt, ht, yt, Tt, Ot, Rt; if ((f == null ? void 0 : f.button) === 2)
                            return; var O = Wt(f); if (O <= 0)
                            return; var v = (ht = (nt = f.global) == null ? void 0 : nt.x) != null ? ht : 0, T = (Tt = (yt = f.global) == null ? void 0 : yt.y) != null ? Tt : 0, C = l.scroll.track, P = l.scroll.thumb; if (!(v >= C.x && v <= C.x + C.w && T >= C.y && T <= C.y + C.h))
                            return; var N = Math.max(0, l.scroll.contentHeight - l.scroll.viewportHeight); if (N <= .5)
                            return; if (v >= P.x && v <= P.x + P.w && T >= P.y && T <= P.y + P.h) {
                            l.scroll.draggingPointerId = O, l.scroll.dragOffsetY = T - P.y, (Ot = f.stopPropagation) == null || Ot.call(f);
                            return;
                        } var z = Math.max(1, C.h - P.h), U = Math.max(C.y, Math.min(C.y + z, T - P.h / 2)), Q = (U - C.y) / z; l.scroll.y = Math.max(0, Math.min(N, Q * N)), l.scroll.draggingPointerId = O, l.scroll.dragOffsetY = T - U, Pt() && (window.__TRUEOS_PIXI_SCROLL_REPAINT_REQUIRED__ = !0, window.__TRUEOS_PIXI_SCROLL_REPAINT_OWNER__ = "root", window.__TRUEOS_PIXI_SCROLL_REPAINT_IFRAME_KEY__ = ""), ct == null || ct(), (Rt = f.stopPropagation) == null || Rt.call(f); }), i_1.stage.on("pointermove", function (f) {
                            var e_65, _a;
                            var N, J, z, U, Q, nt, ht, yt, Tt, Ot, Rt, Et, Ht, Ct, tt, lt, ut, dt, Bt, Lt, $t, Jt, ee, me, fe, De, Le, ve;
                            var O = Number((z = (J = f == null ? void 0 : f.pointerId) != null ? J : (N = f == null ? void 0 : f.data) == null ? void 0 : N.pointerId) != null ? z : 1);
                            if (String((nt = (Q = f == null ? void 0 : f.pointerType) != null ? Q : (U = f == null ? void 0 : f.data) == null ? void 0 : U.pointerType) != null ? nt : "").toLowerCase() === "mouse" || O === 1) {
                                var St = (yt = (ht = f.global) == null ? void 0 : ht.x) != null ? yt : 0, F = (Ot = (Tt = f.global) == null ? void 0 : Tt.y) != null ? Ot : 0;
                                l.lastMouse.x = St, l.lastMouse.y = F, l.lastMouse.has = !0, l.primaryMousePointerId = O;
                                var it = l.harness.enabled ? l.harness.activeUserPointerId : O;
                                l.userCursorPos.set(it, { x: St, y: F }), L_2();
                            }
                            var C = Wt(f);
                            if (C <= 0)
                                return;
                            var P = !1, A = !1;
                            {
                                var St = l.textDrags.get(C);
                                if (St) {
                                    var F = St.key, it = l.fieldBounds.get(F), It = l.inputs.get(F);
                                    if (it && It && typeof It.value == "string") {
                                        var mt = it.isPassword ? "\u2022".repeat(It.value.length) : It.value, vt = ue(ce(mt, Math.max(0, it.innerWidth), _1), it.maxLines), Gt = ((Et = (Rt = f.global) == null ? void 0 : Rt.x) != null ? Et : 0) - it.x - it.innerLeft, pe = ((Ct = (Ht = f.global) == null ? void 0 : Ht.y) != null ? Ct : 0) - it.y - it.innerTop, En = Se({ fullText: mt, lines: vt, localX: Gt, localY: pe, lineHeight: g_2, measure: _1 });
                                        It.selections || (It.selections = new Map), It.selections.set(C, { start: St.anchor, end: En }), P = !0;
                                    }
                                }
                            }
                            {
                                var St = l.sliderDrags.get(C);
                                if (St) {
                                    var F = St.key, it = l.sliderBounds.get(F);
                                    if (it) {
                                        var mt = ((lt = (tt = f.global) == null ? void 0 : tt.x) != null ? lt : 0) - it.x, vt = Math.max(1, it.w - it.innerPad * 2), Gt = (mt - it.innerPad) / vt, pe = xe(l.sliders, F, void 0);
                                        pe.value = Math.max(0, Math.min(1, Gt)), P = !0;
                                    }
                                }
                            }
                            {
                                var St = l.color.draggingPointerId;
                                if (St != null && St === C) {
                                    var F = l.color.bounds;
                                    if (F) {
                                        var it = (dt = (ut = f.global) == null ? void 0 : ut.x) != null ? dt : 0, It = (Lt = (Bt = f.global) == null ? void 0 : Bt.y) != null ? Lt : 0, mt = it - F.x, vt = It - F.y, Gt = Ln({ lx: mt, ly: vt, w: F.w, h: F.h });
                                        Gt && (l.color.rgb = Gt, l.color.pick = { x: mt, y: vt }, P = !0);
                                    }
                                }
                            }
                            {
                                var St = l.scroll.draggingPointerId;
                                if (St != null && St === C) {
                                    var F = l.scroll.track, it = l.scroll.thumb, It = Math.max(0, l.scroll.contentHeight - l.scroll.viewportHeight);
                                    if (It > .5 && F.h > 0 && it.h > 0) {
                                        var mt = (Jt = ($t = f.global) == null ? void 0 : $t.y) != null ? Jt : 0, vt = Math.max(1, F.h - it.h), pe = (Math.max(F.y, Math.min(F.y + vt, mt - l.scroll.dragOffsetY)) - F.y) / vt;
                                        l.scroll.y = Math.max(0, Math.min(It, pe * It)), P = !0, A = !0, Pt() && (window.__TRUEOS_PIXI_SCROLL_REPAINT_OWNER__ = "root", window.__TRUEOS_PIXI_SCROLL_REPAINT_IFRAME_KEY__ = "");
                                    }
                                }
                            }
                            try {
                                for (var _b = __values(l.iframeScroll.entries()), _c = _b.next(); !_c.done; _c = _b.next()) {
                                    var _d = __read(_c.value, 2), St = _d[0], F = _d[1];
                                    if (F.draggingPointerId == null || F.draggingPointerId !== C)
                                        continue;
                                    var it = Math.max(0, F.contentHeight - F.viewportHeight);
                                    if (it <= .5 || F.track.h <= 0 || F.thumb.h <= 0)
                                        continue;
                                    var It = (me = (ee = f.global) == null ? void 0 : ee.y) != null ? me : 0, mt = Math.max(1, F.track.h - F.thumb.h), Gt = (Math.max(F.track.y, Math.min(F.track.y + mt, It - F.dragOffsetY)) - F.track.y) / mt;
                                    F.y = Math.max(0, Math.min(it, Gt * it)), P = !0, A = !0, Pt() && (window.__TRUEOS_PIXI_SCROLL_REPAINT_OWNER__ = "iframe", window.__TRUEOS_PIXI_SCROLL_REPAINT_IFRAME_KEY__ = St);
                                }
                            }
                            catch (e_65_1) { e_65 = { error: e_65_1 }; }
                            finally {
                                try {
                                    if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
                                }
                                finally { if (e_65) throw e_65.error; }
                            }
                            {
                                var St = l.dialogDrags.get(C);
                                if (St) {
                                    var F = nn(l.dialogs, St.key), it = (De = (fe = f.global) == null ? void 0 : fe.x) != null ? De : 0, It = (ve = (Le = f.global) == null ? void 0 : Le.y) != null ? ve : 0;
                                    F.x = St.originX + (it - St.startGX), F.y = St.originY + (It - St.startGY);
                                    var mt = l.dialogDragBounds.get(St.key);
                                    mt && (F.x = Math.max(mt.minX, Math.min(mt.maxX, F.x)), F.y = Math.max(mt.minY, Math.min(mt.maxY, F.y))), P = !0;
                                }
                            }
                            P && (A && Pt() && (window.__TRUEOS_PIXI_SCROLL_REPAINT_REQUIRED__ = !0), ct == null || ct());
                        }), wt("main:input-listeners"), window.addEventListener("keydown", function (f) {
                            var yt, Tt, Ot, Rt, Et, Ht, Ct;
                            var O = l.keyboardOwnerPointerId, v = (yt = l.focusedKeyByPointer.get(O)) != null ? yt : null;
                            if (!v)
                                return;
                            var T = l.inputs.get(v);
                            if (!T || typeof T.value != "string")
                                return;
                            if (T.selections || (T.selections = new Map), !T.selections.has(O)) {
                                var tt = T.value.length;
                                T.selections.set(O, { start: tt, end: tt });
                            }
                            var C = T.selections.get(O), P = T.value.length, A = function (tt) { return Math.max(0, Math.min(P, tt)); }, N = A((Tt = C.start) != null ? Tt : P), J = A((Ot = C.end) != null ? Ot : N);
                            C.start = N, C.end = J;
                            var z = Math.min(N, J), U = Math.max(N, J), Q = z !== U, nt = function (tt) { var lt = Math.max(0, Math.min(T.value.length, tt)); C.start = lt, C.end = lt; }, ht = function (tt, lt) { C.start = Math.max(0, Math.min(T.value.length, tt)), C.end = Math.max(0, Math.min(T.value.length, lt)); };
                            if (f.key.toLowerCase() === "a" && (f.ctrlKey || f.metaKey)) {
                                ht(0, T.value.length), f.preventDefault(), rt_1();
                                return;
                            }
                            if (f.key === "ArrowLeft" || f.key === "ArrowRight") {
                                var tt = f.key === "ArrowLeft" ? -1 : 1;
                                if (f.shiftKey) {
                                    var lt = (Rt = C.start) != null ? Rt : P, ut = ((Et = C.end) != null ? Et : lt) + tt;
                                    ht(lt, ut);
                                }
                                else
                                    nt((Q ? z : U) + tt);
                                f.preventDefault(), q_3();
                                return;
                            }
                            if (f.key === "Home") {
                                f.shiftKey ? ht((Ht = C.start) != null ? Ht : P, 0) : nt(0), f.preventDefault(), q_3();
                                return;
                            }
                            if (f.key === "End") {
                                f.shiftKey ? ht((Ct = C.start) != null ? Ct : 0, T.value.length) : nt(T.value.length), f.preventDefault(), q_3();
                                return;
                            }
                            if (f.key === "Backspace") {
                                if (Q)
                                    T.value = T.value.slice(0, z) + T.value.slice(U), nt(z);
                                else {
                                    var tt = U;
                                    tt > 0 && (T.value = T.value.slice(0, tt - 1) + T.value.slice(tt), nt(tt - 1));
                                }
                                f.preventDefault(), q_3();
                                return;
                            }
                            if (f.key === "Enter") {
                                var tt = "\n";
                                if (Q)
                                    T.value = T.value.slice(0, z) + tt + T.value.slice(U), nt(z + tt.length);
                                else {
                                    var lt = U;
                                    T.value = T.value.slice(0, lt) + tt + T.value.slice(lt), nt(lt + tt.length);
                                }
                                f.preventDefault(), q_3();
                                return;
                            }
                            if (f.key === "Delete") {
                                if (Q)
                                    T.value = T.value.slice(0, z) + T.value.slice(U), nt(z);
                                else {
                                    var tt = U;
                                    tt < T.value.length && (T.value = T.value.slice(0, tt) + T.value.slice(tt + 1), nt(tt));
                                }
                                f.preventDefault(), q_3();
                                return;
                            }
                            if (f.key === "Escape") {
                                l.focusedKeyByPointer.set(O, null), q_3();
                                return;
                            }
                            if (f.key.length === 1 && !f.ctrlKey && !f.metaKey && !f.altKey) {
                                if (Q)
                                    T.value = T.value.slice(0, z) + f.key + T.value.slice(U), nt(z + 1);
                                else {
                                    var tt = U;
                                    T.value = T.value.slice(0, tt) + f.key + T.value.slice(tt), nt(tt + 1);
                                }
                                f.preventDefault(), q_3();
                            }
                        }), window.addEventListener("resize", function () { q_3(), y_1.visible = l.virtualCursor.enabled; }), wt("main:done"), r && (window.__TRUEOS_PIXI_APP_READY__ = !0, window.__TRUEOS_PIXI_APP_ERROR__ = "", window.__TRUEOS_PIXI_APP_PHASE__ = "ready");
                        return [3 /*break*/, 10];
                    case 9:
                        n_3 = _d.sent();
                        window.__TRUEOS_PIXI_APP_READY__ = !1, window.__TRUEOS_PIXI_APP_ERROR__ = Wi(n_3);
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
    ks().then(function () { window.__TRUEOS_PIXI_APP_ERROR__ || (window.__TRUEOS_PIXI_APP_READY__ = !0, window.__TRUEOS_PIXI_APP_ERROR__ = "", window.__TRUEOS_PIXI_APP_PHASE__ = "ready"); }).catch(function (t) { var n; window.__TRUEOS_PIXI_APP_READY__ = !1, window.__TRUEOS_PIXI_APP_ERROR__ = Wi(t), console.error(t); var e = document.createElement("pre"); e.textContent = String((n = t == null ? void 0 : t.stack) != null ? n : t), document.body.appendChild(e); });
})();
