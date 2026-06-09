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
    var Xn = Object.defineProperty, Ii = Object.defineProperties;
    var Ci = Object.getOwnPropertyDescriptors;
    var Un = Object.getOwnPropertySymbols;
    var Oi = Object.prototype.hasOwnProperty, Ri = Object.prototype.propertyIsEnumerable;
    var pn = function (t, e, n) { return e in t ? Xn(t, e, { enumerable: !0, configurable: !0, writable: !0, value: n }) : t[e] = n; }, ee = function (t, e) {
        var e_1, _a;
        for (var n in e || (e = {}))
            Oi.call(e, n) && pn(t, n, e[n]);
        if (Un)
            try {
                for (var _b = __values(Un(e)), _c = _b.next(); !_c.done; _c = _b.next()) {
                    var n = _c.value;
                    Ri.call(e, n) && pn(t, n, e[n]);
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
    }, pe = function (t, e) { return Ii(t, Ci(e)); };
    var Di = function (t, e) { return function () { return (t && (e = t(t = 0)), e); }; };
    var Ai = function (t, e) { for (var n in e)
        Xn(t, n, { get: e[n], enumerable: !0 }); };
    var Z = function (t, e, n) { return pn(t, typeof e != "symbol" ? e + "" : e, n); };
    var Ye = function (t, e, n) { return new Promise(function (r, i) { var o = function (a) { try {
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
    var ui = {};
    Ai(ui, { default: function () { return Co; } });
    var Co, di = Di(function () { Co = {}; });
    var Oe = /** @class */ (function () {
        function Oe(e, n) {
            if (e === void 0) { e = 0; }
            if (n === void 0) { n = 0; }
            Z(this, "x");
            Z(this, "y");
            this.x = Number(e) || 0, this.y = Number(n) || 0;
        }
        Oe.prototype.set = function (e, n) {
            if (e === void 0) { e = 0; }
            if (n === void 0) { n = e; }
            this.x = Number(e) || 0, this.y = Number(n) || 0;
        };
        return Oe;
    }()), pt = /** @class */ (function () {
        function pt(e, n, r, i) {
            if (e === void 0) { e = 0; }
            if (n === void 0) { n = 0; }
            if (r === void 0) { r = 0; }
            if (i === void 0) { i = 0; }
            Z(this, "x");
            Z(this, "y");
            Z(this, "width");
            Z(this, "height");
            this.x = Number(e) || 0, this.y = Number(n) || 0, this.width = Number(r) || 0, this.height = Number(i) || 0;
        }
        return pt;
    }()), gn = /** @class */ (function () {
        function gn() {
            Z(this, "parent");
            Z(this, "children");
            Z(this, "label");
            Z(this, "name");
            Z(this, "position");
            Z(this, "scale");
            Z(this, "pivot");
            Z(this, "visible");
            Z(this, "alpha");
            Z(this, "mask");
            Z(this, "rotation");
            Z(this, "zIndex");
            Z(this, "eventMode");
            Z(this, "cursor");
            Z(this, "hitArea");
            Z(this, "listeners");
            this.parent = null, this.position = new Oe, this.scale = new Oe(1, 1), this.pivot = new Oe, this.visible = !0, this.alpha = 1, this.mask = null, this.rotation = 0, this.zIndex = 0, this.eventMode = null, this.cursor = null, this.hitArea = null, this.listeners = {};
        }
        Object.defineProperty(gn.prototype, "x", {
            get: function () { return this.position.x; },
            set: function (e) { this.position.x = Number(e) || 0; },
            enumerable: false,
            configurable: true
        });
        Object.defineProperty(gn.prototype, "y", {
            get: function () { return this.position.y; },
            set: function (e) { this.position.y = Number(e) || 0; },
            enumerable: false,
            configurable: true
        });
        gn.prototype.on = function (e, n) { return this; };
        gn.prototype.removeAllListeners = function (e) { return e == null ? this.listeners = {} : delete this.listeners[String(e)], this; };
        gn.prototype.removeFromParent = function () { var e; return (e = this.parent) == null || e.removeChild(this), this; };
        gn.prototype.destroy = function (e) { this.removeFromParent(), this.removeAllListeners(); };
        gn.prototype.toLocal = function (e) { var n = e || {}; return { x: (Number(n.x) || 0) - this.getGlobalX(), y: (Number(n.y) || 0) - this.getGlobalY() }; };
        gn.prototype.getGlobalPosition = function () { return { x: this.getGlobalX(), y: this.getGlobalY() }; };
        gn.prototype.getGlobalX = function () { return (this.parent ? this.parent.getGlobalX() : 0) + this.x; };
        gn.prototype.getGlobalY = function () { return (this.parent ? this.parent.getGlobalY() : 0) + this.y; };
        return gn;
    }()), _t = /** @class */ (function (_super) {
        __extends(_t, _super);
        function _t() {
            var _this = _super.call(this) || this;
            Z(_this, "children");
            Z(_this, "sortableChildren");
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
    }(gn)), wt = /** @class */ (function (_super) {
        __extends(wt, _super);
        function wt() {
            var _this = _super.call(this) || this;
            Z(_this, "commands");
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
    }(_t)), Zt = /** @class */ (function (_super) {
        __extends(Zt, _super);
        function Zt(n) {
            if (n === void 0) { n = ""; }
            var r, i;
            var _this = _super.call(this) || this;
            Z(_this, "_text");
            Z(_this, "_style");
            Z(_this, "_resolution");
            _this._text = "", _this._style = {}, _this._resolution = 1, typeof n == "string" ? _this._text = n : (_this._text = String((r = n.text) != null ? r : ""), _this._style = ee({}, (i = n.style) != null ? i : {}));
            return _this;
        }
        Object.defineProperty(Zt.prototype, "text", {
            get: function () { return this._text; },
            set: function (n) { this._text = String(n != null ? n : ""); },
            enumerable: false,
            configurable: true
        });
        Object.defineProperty(Zt.prototype, "style", {
            get: function () { return this._style; },
            set: function (n) { this._style = n != null ? n : {}; },
            enumerable: false,
            configurable: true
        });
        Object.defineProperty(Zt.prototype, "resolution", {
            get: function () { return this._resolution; },
            set: function (n) { this._resolution = Math.max(1, Number(n) || 1); },
            enumerable: false,
            configurable: true
        });
        Object.defineProperty(Zt.prototype, "width", {
            get: function () { var n = Number(this._style.fontSize) || 16; return this._text.length * n * .58; },
            enumerable: false,
            configurable: true
        });
        Object.defineProperty(Zt.prototype, "height", {
            get: function () { var n = Number(this._style.fontSize) || 16; return Number(this._style.lineHeight) || n * 1.25; },
            enumerable: false,
            configurable: true
        });
        Zt.prototype.setSize = function (n, r) { return this; };
        return Zt;
    }(_t)), _e = /** @class */ (function () {
        function _e(e) {
            if (e === void 0) { e = {}; }
            Z(this, "options");
            this.options = e;
        }
        _e.prototype.addAttribute = function (e, n) { return this; };
        _e.prototype.destroy = function () { };
        return _e;
    }()), Ke = /** @class */ (function (_super) {
        __extends(Ke, _super);
        function Ke(n) {
            if (n === void 0) { n = {}; }
            var r, i;
            var _this = _super.call(this) || this;
            Z(_this, "geometry");
            Z(_this, "shader");
            _this.geometry = (r = n.geometry) != null ? r : new _e, _this.shader = (i = n.shader) != null ? i : new Re;
            return _this;
        }
        return Ke;
    }(_t)), ze = /** @class */ (function () {
        function ze(e) {
            if (e === void 0) { e = {}; }
            Z(this, "options");
            this.options = e;
        }
        return ze;
    }()), bn = { VERTEX: 1, COPY_DST: 2 }, Re = /** @class */ (function () {
        function Re(e) {
            if (e === void 0) { e = {}; }
            Z(this, "options");
            this.options = e;
        }
        return Re;
    }());
    var Yn = "", Kn = "", zn = "", je = /** @class */ (function () {
        function je() {
            var _this = this;
            Z(this, "stage");
            Z(this, "screen");
            Z(this, "canvas");
            Z(this, "renderer");
            Z(this, "ticker");
            var e = Math.max(1, Number(globalThis.innerWidth || 1920) | 0), n = Math.max(1, Number(globalThis.innerHeight || 1080) | 0);
            this.stage = new _t, this.screen = new pt(0, 0, e, n), this.canvas = document.createElement("canvas"), this.ticker = { stop: function () { }, add: function () { }, remove: function () { } }, this.renderer = { width: e, height: n, screen: this.screen, render: function (r) { return r; }, resize: function (r, i) { var o = Math.max(1, Number(r || e) | 0), s = Math.max(1, Number(i || n) | 0); _this.renderer.width = o, _this.renderer.height = s, _this.screen.width = o, _this.screen.height = s; } };
        }
        je.prototype.init = function (e) { return Ye(this, null, function () { return __generator(this, function (_a) {
            return [2 /*return*/];
        }); }); };
        return je;
    }());
    var ge = { fontFamily: "system-ui, -apple-system, Segoe UI, Arial", fontSize: 16, background: 16777215, text: 1118481, mutedText: 6710886, boxBorder: 14540253, hr: 13421772, control: { border: 0, focusBorder: 3900150, background: 16777215, accent: 3900150, radius: 0, button: { fill: 15921906, hoverFill: 15395562, activeFill: 14737632, border: 6710886, text: 1118481, radius: 0 }, progress: { border: 10066329, background: 16777215, fill: 6990335 }, table: { border: 10066329, cellBorder: 11579568, headerFill: 16250871 } } };
    var Te = 24, bt = 1;
    function Kt(t) { var i, o; var e = t.wrapWidth, n = (i = t.wordWrap) != null ? i : e != null, r = (o = t.wordWrapWidth) != null ? o : e == null ? void 0 : Math.max(0, Math.ceil(e) + Te); return new Zt({ text: t.text, style: { fontFamily: t.fontFamily, fontSize: t.fontSize, fill: t.fill, fontWeight: t.bold ? "700" : "400", wordWrap: n, wordWrapWidth: r } }); }
    function yn(t, e) { var n = t.children; if (!Array.isArray(n))
        return null; for (var r = 0; r < n.length; r += 1) {
        var i = n[r];
        if (i && i.label === e)
            return i;
    } return null; }
    function be(t, e) { var n = yn(t, e); if (n)
        return n; var r = new _t; return r.label = e, t.addChild(r), r; }
    function Et(t, e) { var n = yn(t, e); if (n)
        return n; var r = new wt; return r.label = e, t.addChild(r), r; }
    function St(t, e, n) { var r = yn(t, e); if (r)
        return r; var i = new Zt({ text: "" }); return i.label = e, n == null || n(i), t.addChild(i), i; }
    function Mt(t) { t.clear(), t.removeAllListeners(), t.hitArea = null; }
    function Ht(t) { t.removeAllListeners(); }
    function ce(t, e, n) {
        var r = String(t != null ? t : ""), i = [], o = 0;
        for (var s = 0; s <= r.length; s++) {
            if (!(s === r.length || r[s] === "\n"))
                continue;
            var a = o, d = s;
            if (a === d)
                i.push({ start: a, end: d, text: "" });
            else {
                var f = a, h = -1;
                for (var b = f; b < d; b++) {
                    r[b] === " " && (h = b);
                    var E = r.slice(f, b + 1);
                    if (n(E) <= e || b === f)
                        continue;
                    var g = h >= f ? h + 1 : b;
                    g <= f && (g = Math.min(d, f + 1)), i.push({ start: f, end: g, text: r.slice(f, g) }), f = g, b = f - 1, h = -1;
                }
                f <= d && i.push({ start: f, end: d, text: r.slice(f, d) });
            }
            o = s + 1;
        }
        return i;
    }
    function ue(t, e) { return e <= 0 ? [] : t.length <= e ? t : t.slice(0, e); }
    function Me(t) { var e = t.fullText, n = t.lines, r = t.localX, i = t.localY, o = t.lineHeight, s = t.measure; if (n.length === 0)
        return 0; var l = Math.max(0, r), a = Math.max(0, i), d = Math.max(1, o), f = Math.max(0, Math.min(n.length - 1, Math.floor(a / d))), h = n[f], b = h.start, c = Number.POSITIVE_INFINITY; for (var E = h.start; E <= h.end; E++) {
        var g = s(e.slice(h.start, E)), x = Math.abs(g - l);
        x < c && (c = x, b = E);
    } return b; }
    function jn(t) { var E, g, x, k; var e = t.node, n = t.graphics, r = t.w, i = t.h, o = t.theme, s = Math.max(0, Math.round(r)), l = Math.max(0, Math.round(i)); n.rect(.5, .5, Math.max(0, s - 1), Math.max(0, l - 1)), n.fill(o.control.progress.background), n.stroke({ width: 1, color: o.control.progress.border }); var a = Number((g = (E = e.attrs) == null ? void 0 : E.value) != null ? g : "0"), d = Number((k = (x = e.attrs) == null ? void 0 : x.max) != null ? k : "1"), f = d > 0 ? Math.max(0, Math.min(1, a / d)) : 0, h = 3, b = Math.max(0, s - h * 2), c = Math.max(0, l - h * 2); n.rect(h, h, Math.max(0, b * f), c), n.fill(o.control.progress.fill); }
    function Vn(t, e) { t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0), t.setHeight(14), t.setMinWidth(240); }
    function ye(t, e, n) { var l; var r = t.get(e); if (r)
        return r; var i = Number((l = n == null ? void 0 : n.value) != null ? l : "0"), o = Number.isFinite(i) ? i : 0, s = { value: Math.max(0, Math.min(1, o)) }; return t.set(e, s), s; }
    function Jn(t, e) { t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0), t.setHeight(14), t.setMinWidth(240); }
    function Zn(t) { var e = t.node, n = t.Yoga, r = t.measurer, i = n.Node.create(); return i.setPadding(n.EDGE_LEFT, 0), i.setPadding(n.EDGE_RIGHT, 0), i.setPadding(n.EDGE_TOP, 0), i.setPadding(n.EDGE_BOTTOM, 0), i.setMargin(n.EDGE_RIGHT, 6), i.setMeasureFunc(function () { var o = r.measure("100"); return { width: o.width, height: o.height }; }), { yogaNode: i, buildBox: function () { return ({ kind: "block", key: e.key, tagName: e.tagName, attrs: e.attrs, x: i.getComputedLeft(), y: i.getComputedTop(), width: i.getComputedWidth(), height: i.getComputedHeight(), children: [] }); } }; }
    function Qn(t) { var d, f; var e = t.node, n = t.container, r = t.theme, i = t.sliderStates, o = (d = e.attrs) == null ? void 0 : d["data-slider-key"], s = null; if (o) {
        var h = i.get(o);
        if (h)
            s = h;
        else {
            var b = (f = e.attrs) == null ? void 0 : f["data-slider-init"];
            s = ye(i, o, b != null ? { value: String(b) } : void 0);
        }
    } var l = s ? Math.round(s.value * 100) : 0, a = St(n, "__pct", function (h) { h.style = { fontFamily: r.fontFamily, fontSize: r.fontSize, fill: r.text, fontWeight: "400", wordWrap: !1 }; }); a.text = String(l), a.position.set(0, bt); }
    function Ve(t) { var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.absX, l = t.absY, a = t.theme, d = t.sliderStates, f = t.sliderBounds, h = t.sliderDrags, b = t.requestPaint, c = e.key, E = c ? ye(d, c, e.attrs) : null, g = Math.max(0, Math.round(i)), x = Math.max(0, Math.round(o)), k = 3; c && f.set(c, { x: s, y: l, w: g, h: x, innerPad: k }), r.rect(.5, .5, Math.max(0, g - 1), Math.max(0, x - 1)), r.fill(a.control.progress.background), r.stroke({ width: 1, color: a.control.progress.border }); var _ = E ? Math.max(0, Math.min(1, E.value)) : 0, T = Math.max(0, g - k * 2), U = Math.max(0, x - k * 2); r.rect(k, k, Math.max(0, T * _), U), r.fill(a.control.progress.fill); var D = k + T * _, $ = U / 2; r.moveTo(D, k - $), r.lineTo(D, k + U + $), r.stroke({ width: 2, color: a.text }), c && (Ht(n), n.eventMode = "static", n.cursor = "pointer", n.hitArea = new pt(0, 0, Math.max(0, g), Math.max(0, x)), n.on("pointerdown", function (v) {
        var e_5, _a;
        var G, j, K, q, z, p;
        if ((v == null ? void 0 : v.button) === 2)
            return;
        var O = t.getPointerId ? t.getPointerId(v) : Number((K = (j = v == null ? void 0 : v.pointerId) != null ? j : (G = v == null ? void 0 : v.data) == null ? void 0 : G.pointerId) != null ? K : 0);
        if (O <= 0)
            return;
        try {
            for (var _b = __values(h.entries()), _c = _b.next(); !_c.done; _c = _b.next()) {
                var _d = __read(_c.value, 2), w = _d[0], C = _d[1];
                C.key === c && w !== O && h.delete(w);
            }
        }
        catch (e_5_1) { e_5 = { error: e_5_1 }; }
        finally {
            try {
                if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
            }
            finally { if (e_5) throw e_5.error; }
        }
        h.set(O, { key: c });
        var M = f.get(c), W = (z = (q = v.global) == null ? void 0 : q.x) != null ? z : 0, N = M ? W - M.x : 0, B = M ? Math.max(1, M.w - M.innerPad * 2) : 1, m = (N - ((p = M == null ? void 0 : M.innerPad) != null ? p : 0)) / B, R = ye(d, c, e.attrs);
        R.value = Math.max(0, Math.min(1, m)), b == null || b();
    })); }
    function qn(t) { var U; var e = t.node, n = t.container, r = t.w, i = t.h, o = t.theme, s = t.detailsOpen, l = t.requestRerender, a = (U = e.attrs) == null ? void 0 : U["data-details-key"], d = e.attrs ? Object.prototype.hasOwnProperty.call(e.attrs, "data-details-open") : !1, f = a && s.has(a) ? s.get(a) === !0 : d, h = function (D) { var O; if (!a || (D == null ? void 0 : D.button) === 2)
        return; var v = !(s.has(a) ? s.get(a) === !0 : d); s.set(a, v), l == null || l(), (O = D == null ? void 0 : D.stopPropagation) == null || O.call(D); }, b = 16, c = Et(n, "__arrow"); Mt(c); var E = 2, g = 3, x = g, k = g, _ = b - g, T = b - g; f ? (c.moveTo(x, k), c.lineTo((x + _) / 2, T), c.lineTo(_, k)) : (c.moveTo(x, k), c.lineTo(_, (k + T) / 2), c.lineTo(x, T)), c.stroke({ width: E, color: o.text }), c.position.set(4, Math.max(0, (i - b) / 2)), c.eventMode = "static", c.cursor = "pointer", c.hitArea = new pt(0, 0, b + 8, b + 8), c.on("pointerdown", h), a && (Ht(n), n.eventMode = "static", n.cursor = "pointer", n.hitArea = new pt(0, 0, Math.max(0, r), Math.max(0, i)), n.on("pointerdown", h)); }
    function tr(t, e) { t.setFlexDirection(e.FLEX_DIRECTION_ROW), t.setAlignItems(e.ALIGN_CENTER), t.setJustifyContent(e.JUSTIFY_SPACE_BETWEEN), t.setPadding(e.EDGE_TOP, 6), t.setPadding(e.EDGE_BOTTOM, 6), t.setPadding(e.EDGE_LEFT, 26), t.setPadding(e.EDGE_RIGHT, 12), t.setMinHeight(36); }
    function er(t, e) { t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0); }
    function nr(t, e) { var i, o, s; if (!t || t.tagName !== "details" || !t.key)
        return (i = t == null ? void 0 : t.children) != null ? i : []; var n = t.attrs ? Object.prototype.hasOwnProperty.call(t.attrs, "open") : !1; return (e.has(t.key) ? e.get(t.key) === !0 : n) ? (o = t.children) != null ? o : [] : ((s = t.children) != null ? s : []).filter(function (l) { return l && l.kind === "block" && l.tagName === "summary"; }); }
    function rr(t) { var e = t.graphics, n = t.w, r = t.theme; e.rect(0, 0, Math.round(n), 1), e.fill(r.hr); }
    function ir(t, e) { t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0), t.setMargin(e.EDGE_TOP, 2), t.setMargin(e.EDGE_BOTTOM, 2), t.setHeight(1); }
    function or(t) { var x, k; var e = t.container, n = t.graphics, r = t.w, i = t.h, o = t.label, s = t.theme, l = t.registerHoverHandlers, a = function (_) { n.clear(); var T = 1, U = T / 2; s.control.button.radius > 0 ? n.roundRect(U, U, Math.max(0, r - T), Math.max(0, i - T), s.control.button.radius) : n.rect(U, U, Math.max(0, r - T), Math.max(0, i - T)), n.fill(_), n.stroke({ width: T, color: s.control.button.border }); }; a(s.control.button.fill); var d = St(e, "__label", function (_) { _.style = { fontFamily: s.fontFamily, fontSize: s.fontSize, fill: s.control.button.text, fontWeight: "400", wordWrap: !1, wordWrapWidth: 0 }; }), f = String(o != null ? o : "").trim(); d.text = f, d.visible = f.length > 0, d.style = pe(ee({}, d.style), { fontFamily: s.fontFamily, fontSize: s.fontSize, fill: s.control.button.text, wordWrap: !1, wordWrapWidth: Math.max(0, Math.ceil(r - 16)) }); var h = Number((x = d.width) != null ? x : 0), b = Number((k = d.height) != null ? k : 0), c = s.fontSize * 1.25; d.position.set(h > 0 ? Math.max(8, Math.floor((r - h) / 2)) : 8, Math.max(0, Math.floor((i - (b > 0 ? b : c)) / 2)) + bt); var E = function () { return a(s.control.button.hoverFill); }, g = function () { return a(s.control.button.fill); }; l == null || l({ over: E, out: g }), Ht(e), e.eventMode = "static", e.cursor = "pointer", e.on("pointerover", E), e.on("pointerout", g), e.on("pointerdown", function () { return a(s.control.button.activeFill); }), e.on("pointerup", function () { return a(s.control.button.hoverFill); }); }
    function sr(t, e) { t.setFlexDirection(e.FLEX_DIRECTION_ROW), t.setPadding(e.EDGE_TOP, 6), t.setPadding(e.EDGE_BOTTOM, 6), t.setMinHeight(36), t.setMinWidth(100), t.setAlignItems(e.ALIGN_CENTER), t.setJustifyContent(e.JUSTIFY_CENTER); }
    function ar(t) { var e = t.graphics, n = t.w, r = t.h, i = t.boxBorder, o = Math.max(0, Math.round(n)), s = Math.max(0, Math.round(r)); e.rect(0, 0, o, s), e.stroke({ width: 1, color: i, alignment: 0 }); }
    function lr(t) { var e = t.nodeTag, n = t.graphics, r = t.w, i = t.h, o = t.theme; e === "th" && (n.rect(0, 0, r, i), n.fill(o.control.table.headerFill)), n.rect(0, 0, r, i), n.stroke({ width: 1, color: o.control.table.cellBorder }); }
    function cr(t, e) { t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0); }
    function ur(t, e) { t.setFlexDirection(e.FLEX_DIRECTION_ROW), t.setFlexWrap(e.WRAP_NO_WRAP), t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0), t.setMargin(e.EDGE_BOTTOM, 0); }
    function dr(t, e) { t.setFlexGrow(1), t.setFlexShrink(1), t.setMinWidth(80), t.setPadding(e.EDGE_LEFT, 8), t.setPadding(e.EDGE_RIGHT, 8), t.setPadding(e.EDGE_TOP, 6), t.setPadding(e.EDGE_BOTTOM, 6), t.setMargin(e.EDGE_BOTTOM, 0); }
    function xn(t) { var e = String(t != null ? t : "").toLowerCase(); if (e.length !== 2 || e.charAt(0) !== "h")
        return !1; var n = e.charCodeAt(1); return n >= 49 && n <= 54; }
    function mr(t, e) { t.setPadding(e.EDGE_TOP, 6), t.setPadding(e.EDGE_BOTTOM, 6), t.setMinHeight(36), t.setJustifyContent(e.JUSTIFY_CENTER); }
    function hr(t, e) {
        var n = Math.max(1, Math.floor(t)), r = Math.max(1, Math.floor(e));
        return "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<svg viewBox=\"0 0 ".concat(n, " ").concat(r, "\" xmlns=\"http://www.w3.org/2000/svg\">\n  <rect x=\"0\" y=\"0\" width=\"").concat(n, "\" height=\"").concat(r, "\" fill=\"#f6f6f6\"/>\n  <rect x=\"0.5\" y=\"0.5\" width=\"").concat(Math.max(0, n - 1), "\" height=\"").concat(Math.max(0, r - 1), "\" fill=\"none\" stroke=\"#999\"/>\n  <path d=\"M2 2 L").concat(Math.max(2, n - 2), " ").concat(Math.max(2, r - 2), "\" stroke=\"#c8c8c8\"/>\n  <path d=\"M").concat(Math.max(2, n - 2), " 2 L2 ").concat(Math.max(2, r - 2), "\" stroke=\"#c8c8c8\"/>\n</svg>");
    }
    function fr(_a) {
        var _b = _a === void 0 ? {} : _a, _c = _b.ring, t = _c === void 0 ? 34 : _c, _d = _b.core, e = _d === void 0 ? 14 : _d, _f = _b.hueA, n = _f === void 0 ? "#00e5ff" : _f, _g = _b.hueB, r = _g === void 0 ? "#ff2bd6" : _g;
        var i = Math.max(0, t - 10), o = Math.max(0, e * .35);
        return "\n<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 100 100\">\n  <rect width=\"100\" height=\"100\" fill=\"#ffffff\"/>\n  <rect width=\"100\" height=\"100\" fill=\"".concat(n, "\" opacity=\"0.08\"/>\n\n  <circle cx=\"50\" cy=\"50\" r=\"").concat(t, "\" fill=\"none\" stroke=\"").concat(r, "\" stroke-width=\"4\" opacity=\"0.95\"/>\n  <circle cx=\"50\" cy=\"50\" r=\"").concat(i, "\" fill=\"none\" stroke=\"").concat(n, "\" stroke-width=\"1\" opacity=\"0.35\"/>\n\n  <circle cx=\"50\" cy=\"50\" r=\"").concat(e, "\" fill=\"").concat(n, "\" opacity=\"0.9\"/>\n  <circle cx=\"43\" cy=\"43\" r=\"").concat(o, "\" fill=\"#ffffff\" opacity=\"0.55\"/>\n\n  <path d=\"M50 16 L52 22 L58 24 L52 26 L50 32 L48 26 L42 24 L48 22 Z\" fill=\"#ffffff\" opacity=\"0.85\"/>\n  <path d=\"M82 52 L85 56 L90 57 L85 58 L82 62 L79 58 L74 57 L79 56 Z\" fill=\"#ffffff\" opacity=\"0.70\"/>\n  <path d=\"M20 70 L22 74 L27 75 L22 76 L20 80 L18 76 L13 75 L18 74 Z\" fill=\"#ffffff\" opacity=\"0.65\"/>\n</svg>\n");
    }
    var pr = new Map;
    function De() { var t = globalThis; return !0; }
    function yr(t) { var e = globalThis, n = String(t != null ? t : "").trim(); if (!n)
        return null; var r = e.__TRUEOS_READY_IMAGE_TEXTURE_CACHE__; (!r || typeof r.get != "function" || typeof r.set != "function") && (r = new Map, e.__TRUEOS_READY_IMAGE_TEXTURE_CACHE__ = r); var i = r.get(n); if (i)
        return i; var o = e.__trueosResolveReadyImageTexture; if (typeof o != "function")
        return null; var s = o(n); if (s && typeof s.then == "function") {
        var l = s.then(function (a) { return (r.set(n, a), a); }).catch(function (a) { throw r.delete(n), a; });
        return r.set(n, l), l;
    } return r.set(n, s), s; }
    function vi(t) { var s; var e = globalThis, n = String(t != null ? t : "").trim(); if (!n)
        return null; var r = e.__trueosPeekReadyImageTexture; if (typeof r != "function")
        return null; var i = r(n); return !i || typeof i.then == "function" ? null : Math.max(0, Number((s = i == null ? void 0 : i.texId) != null ? s : 0) | 0) > 0 ? i : null; }
    function Ni(t, e) { var n = globalThis, r = String(t != null ? t : "").trim(); if (!r || !e)
        return; var i = n.__TRUEOS_READY_IMAGE_TEXTURE_CACHE__; (!i || typeof i.get != "function" || typeof i.set != "function") && (i = new Map, n.__TRUEOS_READY_IMAGE_TEXTURE_CACHE__ = i), i.set(r, e); }
    function xr(t, e) { var r, i, o, s, l; var n = Math.max(0, Number((r = e == null ? void 0 : e.texId) != null ? r : 0) | 0); if (n <= 0)
        throw new Error("image texture missing"); t.state = "ready", t.texId = n, t.width = Math.max(0, Number((o = (i = e == null ? void 0 : e.pixelWidth) != null ? i : e == null ? void 0 : e.width) != null ? o : 0) | 0), t.height = Math.max(0, Number((l = (s = e == null ? void 0 : e.pixelHeight) != null ? s : e == null ? void 0 : e.height) != null ? l : 0) | 0); }
    function Gi(t, e) { var n = vi(t) || yr(t); return !n || typeof n.then == "function" ? !1 : (xr(e, n), Ni(t, n), !0); }
    function gr(t, e) { var n = String(t != null ? t : "").trim(); if (!n)
        return null; var r = pr.get(n); if (r) {
        if (De() && r.state === "loading")
            try {
                Gi(n, r);
            }
            catch (l) {
                r.state = "error";
            }
        return r;
    } if (De())
        return null; var i = { state: "loading", texId: 0, width: 0, height: 0 }; pr.set(n, i); var o = function (l) { xr(i, l), De() || e == null || e(); }, s = function () { i.state = "error", De() || e == null || e(); }; try {
        var l = yr(n);
        if (!l)
            return i;
        if (l && typeof l.then == "function") {
            if (De())
                return i;
            l.then(o).catch(s);
        }
        else
            o(l);
    }
    catch (l) {
        s();
    } return i; }
    function Li(t) { var e = String(t != null ? t : ""); if (!e.startsWith("data:image/svg+xml"))
        return null; var n = e.indexOf(","); if (n === -1)
        return null; var r = e.slice(0, n).toLowerCase(), i = e.slice(n + 1); try {
        return r.includes(";base64") ? atob(i) : decodeURIComponent(i);
    }
    catch (o) {
        return null;
    } }
    function Bi(t) { return br(br(String(t), "tspan"), "text"); }
    function Fi(t) { return "data:image/svg+xml;charset=utf-8,".concat(encodeURIComponent(t)); }
    function br(t, e) { var n = "", r = 0, i = t.toLowerCase(), o = "<" + e, s = "</" + e; for (; r < t.length;) {
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
    function wr(t) { var U, D, $, v; var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.theme, l = t.requestRerender, a = (D = (U = e.attrs) == null ? void 0 : U.alt) != null ? D : "", d = (v = ($ = e.attrs) == null ? void 0 : $.src) != null ? v : "", f = d.trim().length > 0, h = a.trim().length > 0 ? a : d.trim().length > 0 ? d : "img", b = r.image, c = f ? gr(d, l) : null; if ((c == null ? void 0 : c.state) === "ready" && c.texId > 0 && typeof b == "function") {
        b.call(r, c.texId, 0, 0, Math.max(0, i), Math.max(0, o));
        var O = n.getChildByLabel ? n.getChildByLabel("__label") : n.children.find(function (M) { return (M == null ? void 0 : M.label) === "__label"; });
        O && (O.visible = !1);
        return;
    } var E = f ? Li(d) : null, g = Bi(E != null ? E : f ? hr(i, o) : fr({ ring: 34, core: 14 })), x = Et(n, "__svg"), k = gr(Fi(g), l); if ((k == null ? void 0 : k.state) === "ready" && k.texId > 0 && typeof x.image == "function") {
        var O = "texture:".concat(k.texId, ":").concat(Math.round(i), "x").concat(Math.round(o));
        if (x.__key !== O && (Mt(x), x.image(k.texId, 0, 0, Math.max(0, i), Math.max(0, o)), x.__key = O), x.scale.set(1), x.position.set(0, 0), !f) {
            var M = n.getChildByLabel ? n.getChildByLabel("__label") : n.children.find(function (W) { return (W == null ? void 0 : W.label) === "__label"; });
            M && (M.visible = !1);
            return;
        }
        if (h.trim().length > 0) {
            var M = St(n, "__label", function (W) { W.style = { fontFamily: s.fontFamily, fontSize: s.fontSize, fill: s.mutedText, fontWeight: "400", wordWrap: !0, wordWrapWidth: 0 }; });
            M.text = h, M.style.wordWrapWidth = Math.max(0, Math.ceil(i - 16) + Te), M.position.set(8, 8 + bt), M.visible = !0;
        }
        return;
    }
    else
        Mt(x); var _ = x.svg; if (0 && x.__key !== O)
        try { }
        catch (W) { } r.rect(.5, .5, Math.max(0, i - 1), Math.max(0, o - 1)), r.fill(16185078), r.stroke({ width: 1, color: s.control.border }); var T = St(n, "__label", function (O) { O.style = { fontFamily: s.fontFamily, fontSize: s.fontSize, fill: s.mutedText, fontWeight: "400", wordWrap: !0, wordWrapWidth: 0 }; }); T.text = h, T.style.wordWrapWidth = Math.max(0, Math.ceil(i - 16) + Te), T.position.set(8, 8 + bt); }
    function _r(t, e, n) { var d, f, h, b; t.setPadding(n.EDGE_LEFT, 0), t.setPadding(n.EDGE_RIGHT, 0), t.setPadding(n.EDGE_TOP, 0), t.setPadding(n.EDGE_BOTTOM, 0); var r = Number((f = (d = e.attrs) == null ? void 0 : d.width) != null ? f : "0"), i = Number((b = (h = e.attrs) == null ? void 0 : h.height) != null ? b : "0"), o = Number.isFinite(r) && r > 0, s = Number.isFinite(i) && i > 0, l = o ? r : 240, a = s ? i : 140; (o || s) && (t.setAlignSelf(n.ALIGN_FLEX_START), t.setFlexGrow(0), t.setFlexShrink(0)), t.setWidth(l), t.setHeight(a), t.setMinWidth(120), t.setMinHeight(80); }
    var Tr = new Map;
    function Ae() { var t = globalThis; return !0; }
    function kr(t) { var e = globalThis, n = String(t != null ? t : "").trim(); if (!n)
        return null; var r = e.__TRUEOS_READY_IMAGE_TEXTURE_CACHE__; (!r || typeof r.get != "function" || typeof r.set != "function") && (r = new Map, e.__TRUEOS_READY_IMAGE_TEXTURE_CACHE__ = r); var i = r.get(n); if (i)
        return i; var o = e.__trueosResolveReadyImageTexture; if (typeof o != "function")
        return null; var s = o(n); if (s && typeof s.then == "function") {
        var l = s.then(function (a) { return (r.set(n, a), a); }).catch(function (a) { throw r.delete(n), a; });
        return r.set(n, l), l;
    } return r.set(n, s), s; }
    function Hi(t) { var s; var e = globalThis, n = String(t != null ? t : "").trim(); if (!n)
        return null; var r = e.__trueosPeekReadyImageTexture; if (typeof r != "function")
        return null; var i = r(n); return !i || typeof i.then == "function" ? null : Math.max(0, Number((s = i == null ? void 0 : i.texId) != null ? s : 0) | 0) > 0 ? i : null; }
    function Wi(t, e) { var n = globalThis, r = String(t != null ? t : "").trim(); if (!r || !e)
        return; var i = n.__TRUEOS_READY_IMAGE_TEXTURE_CACHE__; (!i || typeof i.get != "function" || typeof i.set != "function") && (i = new Map, n.__TRUEOS_READY_IMAGE_TEXTURE_CACHE__ = i), i.set(r, e); }
    function Er(t, e) { var r, i, o, s, l; var n = Math.max(0, Number((r = e == null ? void 0 : e.texId) != null ? r : 0) | 0); if (n <= 0)
        throw new Error("svg texture missing"); t.state = "ready", t.texId = n, t.width = Math.max(0, Number((o = (i = e == null ? void 0 : e.pixelWidth) != null ? i : e == null ? void 0 : e.width) != null ? o : 0) | 0), t.height = Math.max(0, Number((l = (s = e == null ? void 0 : e.pixelHeight) != null ? s : e == null ? void 0 : e.height) != null ? l : 0) | 0); }
    function $i(t, e) { var n = Hi(t) || kr(t); return !n || typeof n.then == "function" ? !1 : (Er(e, n), Wi(t, n), !0); }
    function Ui(t) { return Mr(Mr(String(t), "tspan"), "text"); }
    function Mr(t, e) { var n = "", r = 0, i = t.toLowerCase(), o = "<" + e, s = "</" + e; for (; r < t.length;) {
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
    function Sr(t) { var e = String(t), r = e.toLowerCase().indexOf("viewbox"); if (r < 0)
        return null; var i = e.indexOf("=", r + 7); if (i < 0)
        return null; var o = i + 1; for (; o < e.length;) {
        var c = e.charCodeAt(o);
        if (c !== 32 && c !== 9 && c !== 10 && c !== 13 && c !== 12)
            break;
        o += 1;
    } var s = e.charAt(o); if (s !== '"' && s !== "'")
        return null; var l = e.indexOf(s, o + 1); if (l < 0)
        return null; var a = Xi(e.slice(o + 1, l)); if (a.length < 4)
        return null; var d = Number(a[0]), f = Number(a[1]), h = Number(a[2]), b = Number(a[3]); return ![d, f, h, b].every(function (c) { return Number.isFinite(c); }) || h <= 0 || b <= 0 ? null : { minX: d, minY: f, w: h, h: b }; }
    function Xi(t) { var e = [], n = ""; for (var r = 0; r < t.length; r += 1) {
        var i = t.charCodeAt(r);
        i === 32 || i === 9 || i === 10 || i === 13 || i === 12 ? n.length > 0 && (e.push(n), n = "") : n += t.charAt(r);
    } return n.length > 0 && e.push(n), e; }
    function Yi(t, e) { var n = String(t != null ? t : ""); if (!n.trim())
        return null; var r = Tr.get(n), i = "data:image/svg+xml;charset=utf-8,".concat(encodeURIComponent(n)); if (r) {
        if (Ae() && r.state === "loading")
            try {
                $i(i, r);
            }
            catch (a) {
                r.state = "error";
            }
        return r;
    } if (Ae())
        return null; var o = { state: "loading", texId: 0, width: 0, height: 0 }; Tr.set(n, o); var s = function (a) { Er(o, a), Ae() || e == null || e(); }, l = function () { o.state = "error", Ae() || e == null || e(); }; try {
        var a = kr(i);
        if (!a)
            return o;
        if (a && typeof a.then == "function") {
            if (Ae())
                return o;
            a.then(s).catch(l);
        }
        else
            s(a);
    }
    catch (a) {
        l();
    } return o; }
    function Ki(t, e, n) { var r = Math.max(0, e), i = Math.max(0, n), o = Sr(t); if (!o || r <= 0 || i <= 0)
        return { x: 0, y: 0, w: r, h: i }; var s = r / o.w, l = i / o.h, a = Math.min(s, l), d = Math.max(0, o.w * a), f = Math.max(0, o.h * a); return { x: Math.max(0, (r - d) / 2), y: Math.max(0, (i - f) / 2), w: d, h: f }; }
    function Pr(t, e, n) { var d, f, h, b; t.setPadding(n.EDGE_LEFT, 0), t.setPadding(n.EDGE_RIGHT, 0), t.setPadding(n.EDGE_TOP, 0), t.setPadding(n.EDGE_BOTTOM, 0); var r = Number((f = (d = e.attrs) == null ? void 0 : d.width) != null ? f : "0"), i = Number((b = (h = e.attrs) == null ? void 0 : h.height) != null ? b : "0"), o = Number.isFinite(r) && r > 0, s = Number.isFinite(i) && i > 0, l = o ? r : 300, a = s ? i : 150; (o || s) && (t.setAlignSelf(n.ALIGN_FLEX_START), t.setFlexGrow(0), t.setFlexShrink(0)), t.setWidth(l), t.setHeight(a), t.setMinWidth(Math.min(120, l)), t.setMinHeight(Math.min(80, a)); }
    function Ir(t) { var e = t.svgMarkup, n = t.container, r = t.w, i = t.h, o = t.requestRerender, s = Ui(e), l = Et(n, "__svg"), a = l.__svgString, d = l.__w, f = l.__h, h = a !== s, b = Yi(s, o); if (l.scale.set(1), l.position.set(0, 0), (b == null ? void 0 : b.state) === "ready" && b.texId > 0 && typeof l.image == "function") {
        if (h || d !== r || f !== i || l.__texId !== b.texId) {
            var E = Ki(s, r, i);
            Mt(l), l.image(b.texId, E.x, E.y, E.w, E.h), l.__svgString = s, l.__w = r, l.__h = i, l.__texId = b.texId;
        }
        return;
    } Mt(l); return; if (typeof c == "function") {
        if (h || d !== r || f !== i) {
            Mt(l);
            var g = void 0;
            try {
                g = c.call(l, s);
            }
            catch (x) {
                g = null;
            }
            g && typeof g.then == "function" && g.then(function () { return o == null ? void 0 : o(); }).catch(function () { }), l.__svgString = s, l.__w = r, l.__h = i;
        }
        var E = Sr(s);
        if (E) {
            var g = r / E.w, x = i / E.h, k = Math.min(g, x), _ = E.w * k, T = E.h * k;
            l.scale.set(k), l.position.set(-E.minX * k + (r - _) / 2, -E.minY * k + (i - T) / 2);
        }
        return;
    } }
    function Cr(t, e, n) { var d, f, h, b; t.setPadding(n.EDGE_LEFT, 0), t.setPadding(n.EDGE_RIGHT, 0), t.setPadding(n.EDGE_TOP, 0), t.setPadding(n.EDGE_BOTTOM, 0); var r = Number((f = (d = e.attrs) == null ? void 0 : d.width) != null ? f : "0"), i = Number((b = (h = e.attrs) == null ? void 0 : h.height) != null ? b : "0"), o = Number.isFinite(r) && r > 0, s = Number.isFinite(i) && i > 0, l = o ? r : 300, a = s ? i : 150; (o || s) && (t.setAlignSelf(n.ALIGN_FLEX_START), t.setFlexGrow(0), t.setFlexShrink(0)), t.setWidth(l), t.setHeight(a), t.setMinWidth(Math.min(120, l)), t.setMinHeight(Math.min(80, a)); }
    function Or(t) { var e = t.graphics, n = t.container, r = t.w, i = t.h, o = t.theme, s = 1, l = s / 2; e.rect(l, l, Math.max(0, r - s), Math.max(0, i - s)), e.fill(16777215), e.stroke({ width: s, color: o.control.border, alignment: 0 }), e.moveTo(6, i - 6), e.lineTo(r - 6, 6), e.stroke({ width: 1, color: 0, alpha: .1 }); var a = Kt({ text: "canvas", fontFamily: o.fontFamily, fontSize: Math.max(10, Math.floor(o.fontSize * .85)), fill: o.mutedText, wordWrap: !1 }); a.position.set(8, 8 + bt), n.addChild(a); }
    function Rr(t, e, n) { var f, h, b, c, E, g; var r = String((h = (f = e.attrs) == null ? void 0 : f["data-root"]) != null ? h : "") === "1"; if (t.setFlexDirection(n.FLEX_DIRECTION_COLUMN), t.setAlignItems(n.ALIGN_STRETCH), r) {
        t.setPadding(n.EDGE_LEFT, 0), t.setPadding(n.EDGE_RIGHT, 0), t.setPadding(n.EDGE_TOP, 0), t.setPadding(n.EDGE_BOTTOM, 0), t.setAlignSelf(n.ALIGN_STRETCH), t.setFlexGrow(1), t.setFlexShrink(1), t.setMinWidth(0), t.setMinHeight(0);
        return;
    } t.setPadding(n.EDGE_LEFT, 8), t.setPadding(n.EDGE_RIGHT, 8), t.setPadding(n.EDGE_BOTTOM, 8), t.setPadding(n.EDGE_TOP, 34); var i = Number((c = (b = e.attrs) == null ? void 0 : b.width) != null ? c : "0"), o = Number((g = (E = e.attrs) == null ? void 0 : E.height) != null ? g : "0"), s = Number.isFinite(i) && i > 0, l = Number.isFinite(o) && o > 0, a = s ? i : 420, d = l ? o : 240; (s || l) && (t.setAlignSelf(n.ALIGN_FLEX_START), t.setFlexGrow(0), t.setFlexShrink(0)), t.setWidth(a), t.setHeight(d), t.setMinWidth(Math.min(200, a)), t.setMinHeight(Math.min(160, d)); }
    function Dr(t) { var c, E, g, x; var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.theme; if (String((E = (c = e.attrs) == null ? void 0 : c["data-root"]) != null ? E : "") === "1")
        return; var a = 1, d = a / 2; r.rect(d, d, Math.max(0, i - a), Math.max(0, o - a)), r.fill(16777215), r.stroke({ width: a, color: s.control.border, alignment: 0 }), r.rect(d, d, Math.max(0, i - a), 26), r.fill({ color: 0, alpha: .04 }); var h = String((x = (g = e.attrs) == null ? void 0 : g.srcdoc) != null ? x : "").trim().length > 0 ? "srcdoc" : "empty", b = St(n, "__title", function (k) { k.style = { fontFamily: s.fontFamily, fontSize: Math.max(10, Math.floor(s.fontSize * .85)), fill: s.mutedText, fontWeight: "400", wordWrap: !1 }; }); b.text = "iframe (".concat(h, ")"), b.position.set(8, 6 + bt), n.eventMode = "static", n.cursor = "default", n.hitArea = new pt(0, 0, Math.max(0, i), Math.max(0, o)); }
    function Ar(t, e, n) { var i, o; var r = ((o = (i = e.attrs) == null ? void 0 : i.type) != null ? o : "text").toLowerCase(); r === "checkbox" || r === "radio" ? (t.setPadding(n.EDGE_LEFT, 0), t.setPadding(n.EDGE_RIGHT, 0), t.setPadding(n.EDGE_TOP, 0), t.setPadding(n.EDGE_BOTTOM, 0), t.setWidth(16), t.setHeight(16), t.setMinWidth(16), t.setMargin(n.EDGE_RIGHT, 6)) : (t.setPadding(n.EDGE_TOP, 6), t.setPadding(n.EDGE_BOTTOM, 6), t.setHeight(36), t.setMinHeight(36), t.setMinWidth(220)); }
    function vr(t) {
        var e_6, _a, e_7, _b;
        var N, B, m, R, G, j, K, q, z, p;
        var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.absX, l = t.absY, a = t.theme, d = t.textMeasure, f = t.uiState, h = t.getOrInitInputState, b = t.clamp, c = t.radioGroups, E = t.textDrags, g = t.requestPaint, x = ((B = (N = e.attrs) == null ? void 0 : N.type) != null ? B : "text").toLowerCase(), k = e.key, _ = k ? h(k, e.attrs) : void 0, T = (m = t.showCaret) != null ? m : !1, U = (R = t.caretPointerId) != null ? R : null, D = t.focusColor, $ = t.getCursorColor;
        n.sortableChildren = !0;
        try {
            for (var _c = __values(n.children), _d = _c.next(); !_d.done; _d = _c.next()) {
                var w = _d.value;
                var C = w.label;
                C && (C.startsWith("__sel:") || C === "__caret") && (w.visible = !1);
            }
        }
        catch (e_6_1) { e_6 = { error: e_6_1 }; }
        finally {
            try {
                if (_d && !_d.done && (_a = _c.return)) _a.call(_c);
            }
            finally { if (e_6) throw e_6.error; }
        }
        var v = 8, O = 6 + bt, M = 5, W = a.fontSize * 1.25;
        if (x === "checkbox")
            r.rect(.5, .5, Math.max(0, i - 1), Math.max(0, o - 1)), r.fill(a.control.background), r.stroke({ width: 1, color: a.control.border }), _ != null && _.indeterminate ? (r.moveTo(4, 4), r.lineTo(Math.max(4, i - 4), Math.max(4, o - 4)), r.stroke({ width: 2, color: a.control.accent }), r.moveTo(Math.max(4, i - 4), 4), r.lineTo(4, Math.max(4, o - 4)), r.stroke({ width: 2, color: a.control.accent })) : _ != null && _.checked && (r.rect(3, 3, Math.max(0, i - 3 * 2), Math.max(0, o - 3 * 2)), r.fill(a.control.accent));
        else if (x === "radio") {
            {
                var S = Math.max(0, Math.min(i, o) / 2 - .5);
                r.circle(i / 2, o / 2, S), r.fill(a.control.background), r.stroke({ width: 1, color: a.control.border });
            }
            if (_ != null && _.checked) {
                var w = Math.max(0, Math.min(i, o) / 2 - 4.5);
                r.circle(i / 2, o / 2, w), r.fill(a.control.accent);
            }
        }
        else {
            var w = D != null ? 2 : 1, C = w / 2;
            a.control.radius > 0 ? r.roundRect(C, C, Math.max(0, i - w), Math.max(0, o - w), a.control.radius) : r.rect(C, C, Math.max(0, i - w), Math.max(0, o - w)), r.fill(a.control.background), r.stroke({ width: w, color: D != null ? D : a.control.border });
            var S = x === "password" ? "\u2022".repeat(((G = _ == null ? void 0 : _.value) != null ? G : "").length) : (j = _ == null ? void 0 : _.value) != null ? j : "", P = Math.max(0, i - v * 2);
            k && f.fieldBounds.set(k, { x: s, y: l, w: i, h: o, innerLeft: v, innerTop: O, innerWidth: P, maxLines: M, isPassword: x === "password" });
            var F = ce(S, P, d), y = ue(F, M), I = y.length > 0 ? y[y.length - 1].end : 0;
            if (k && _ && typeof _.value == "string") {
                var X = _.selections;
                if (X && X.size > 0)
                    try {
                        for (var _f = __values(X.entries()), _g = _f.next(); !_g.done; _g = _f.next()) {
                            var _h = __read(_g.value, 2), it = _h[0], et = _h[1];
                            var Y = b((K = et.start) != null ? K : 0, 0, S.length), V = b((q = et.end) != null ? q : Y, 0, S.length), lt = b(Math.min(Y, V), 0, I), tt = b(Math.max(Y, V), 0, I);
                            if (lt === tt)
                                continue;
                            var nt = Et(n, "__sel:".concat(it));
                            Mt(nt), nt.zIndex = 0, nt.visible = !0;
                            for (var ot = 0; ot < y.length; ot++) {
                                var gt = y[ot], It = Math.max(lt, gt.start), at = Math.min(tt, gt.end);
                                if (It >= at)
                                    continue;
                                var Tt = v + d(S.slice(gt.start, It)), Lt = d(S.slice(It, at));
                                nt.rect(Tt, O + ot * W, Lt, W);
                            }
                            nt.fill({ color: $(it), alpha: .22 });
                        }
                    }
                    catch (e_7_1) { e_7 = { error: e_7_1 }; }
                    finally {
                        try {
                            if (_g && !_g.done && (_b = _f.return)) _b.call(_f);
                        }
                        finally { if (e_7) throw e_7.error; }
                    }
                if (T && U != null) {
                    var it = (z = _.selections) == null ? void 0 : z.get(U), et = it ? it.end : 0, Y = b(et, 0, I), V = Math.max(0, y.length - 1);
                    for (var ot = 0; ot < y.length; ot++) {
                        var gt = y[ot];
                        if (Y >= gt.start && Y <= gt.end) {
                            V = ot;
                            break;
                        }
                    }
                    var lt = (p = y[V]) != null ? p : { start: 0, end: 0, text: "" }, tt = v + d(S.slice(lt.start, Y)), nt = Et(n, "__caret");
                    Mt(nt), nt.zIndex = 2, nt.visible = !0, nt.moveTo(tt, O + V * W), nt.lineTo(tt, O + V * W + W), nt.stroke({ width: 1, color: D != null ? D : a.control.focusBorder });
                }
            }
            var H = y.map(function (X) { return X.text; }).join("\n"), A = St(n, "__valueText", function (X) { X.style = { fontFamily: a.fontFamily, fontSize: a.fontSize, fill: a.text, fontWeight: "400", wordWrap: !1 }, X.zIndex = 1; });
            A.text = H, A.position.set(v, O);
        }
        k && (Ht(n), n.eventMode = "static", n.cursor = "text", n.on("pointerdown", function (w) {
            var e_8, _a, e_9, _b, e_10, _c;
            var S, P, F, y, I, H, A, X, it, et, Y, V, lt;
            if ((w == null ? void 0 : w.button) === 2)
                return;
            var C = t.getPointerId ? t.getPointerId(w) : Number((F = (P = w == null ? void 0 : w.pointerId) != null ? P : (S = w == null ? void 0 : w.data) == null ? void 0 : S.pointerId) != null ? F : 0);
            if (!(C <= 0)) {
                if (f.focusedKeyByPointer.set(C, k), f.keyboardOwnerPointerId = C, x === "checkbox") {
                    var tt = h(k, e.attrs), nt = tt.indeterminate === !0, ot = tt.checked === !0;
                    !ot && !nt ? (tt.checked = !0, tt.indeterminate = !1) : ot && !nt ? (tt.checked = !1, tt.indeterminate = !0) : (tt.checked = !1, tt.indeterminate = !1);
                }
                else if (x === "radio") {
                    var nt = "radio:".concat((I = (y = e.attrs) == null ? void 0 : y.name) != null ? I : "__default__"), ot = (H = c.get(nt)) != null ? H : [];
                    try {
                        for (var ot_1 = __values(ot), ot_1_1 = ot_1.next(); !ot_1_1.done; ot_1_1 = ot_1.next()) {
                            var gt = ot_1_1.value;
                            var It = h(gt, void 0);
                            It.checked = gt === k;
                        }
                    }
                    catch (e_8_1) { e_8 = { error: e_8_1 }; }
                    finally {
                        try {
                            if (ot_1_1 && !ot_1_1.done && (_a = ot_1.return)) _a.call(ot_1);
                        }
                        finally { if (e_8) throw e_8.error; }
                    }
                }
                else {
                    var tt = h(k, e.attrs);
                    if (typeof tt.value == "string") {
                        try {
                            for (var _d = __values(f.inputs.entries()), _f = _d.next(); !_f.done; _f = _d.next()) {
                                var _g = __read(_f.value, 2), $t = _g[0], Nt = _g[1];
                                $t !== k && ((A = Nt.selections) == null || A.delete(C));
                            }
                        }
                        catch (e_9_1) { e_9 = { error: e_9_1 }; }
                        finally {
                            try {
                                if (_f && !_f.done && (_b = _d.return)) _b.call(_d);
                            }
                            finally { if (e_9) throw e_9.error; }
                        }
                        var nt = x === "password" ? "\u2022".repeat(tt.value.length) : tt.value, ot = f.fieldBounds.get(k), gt = (X = ot == null ? void 0 : ot.innerWidth) != null ? X : Math.max(0, i - v * 2), It = ue(ce(nt, gt, d), M), at = ((et = (it = w.global) == null ? void 0 : it.x) != null ? et : 0) - s - v, Tt = ((V = (Y = w.global) == null ? void 0 : Y.y) != null ? V : 0) - l - O, Lt = Me({ fullText: nt, lines: It, localX: at, localY: Tt, lineHeight: W, measure: d });
                        tt.selections || (tt.selections = new Map), tt.selections.set(C, { start: Lt, end: Lt });
                        try {
                            for (var _h = __values(E.entries()), _j = _h.next(); !_j.done; _j = _h.next()) {
                                var _k = __read(_j.value, 2), $t = _k[0], Nt = _k[1];
                                Nt.key === k && $t !== C && E.delete($t);
                            }
                        }
                        catch (e_10_1) { e_10 = { error: e_10_1 }; }
                        finally {
                            try {
                                if (_j && !_j.done && (_c = _h.return)) _c.call(_h);
                            }
                            finally { if (e_10) throw e_10.error; }
                        }
                        E.set(C, { key: k, anchor: Lt });
                    }
                }
                (x === "checkbox" || x === "radio") && ((lt = w.stopPropagation) == null || lt.call(w)), g == null || g();
            }
        }), (x === "checkbox" || x === "radio") && (n.cursor = "pointer"), n.hitArea = new pt(0, 0, Math.max(0, i), Math.max(0, o)));
    }
    function Nr(t, e) { t.setPadding(e.EDGE_TOP, 6), t.setPadding(e.EDGE_BOTTOM, 6), t.setHeight(108), t.setMinHeight(108), t.setMinWidth(220); }
    function Gr(t) {
        var e_11, _a, e_12, _b;
        var q, z, p, w, C, S, P, F;
        var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.absX, l = t.absY, a = t.theme, d = t.textMeasure, f = t.uiState, h = t.getOrInitInputState, b = t.clamp, c = t.textDrags, E = t.requestPaint, g = e.key, x = g ? h(g, pe(ee({}, (q = e.attrs) != null ? q : {}), { type: "text" })) : void 0, k = (z = t.showCaret) != null ? z : !1, _ = (p = t.caretPointerId) != null ? p : null, T = t.focusColor, U = t.getCursorColor;
        n.sortableChildren = !0;
        try {
            for (var _c = __values(n.children), _d = _c.next(); !_d.done; _d = _c.next()) {
                var y = _d.value;
                var I = y.label;
                I && (I.startsWith("__sel:") || I === "__caret") && (y.visible = !1);
            }
        }
        catch (e_11_1) { e_11 = { error: e_11_1 }; }
        finally {
            try {
                if (_d && !_d.done && (_a = _c.return)) _a.call(_c);
            }
            finally { if (e_11) throw e_11.error; }
        }
        var D = 8, $ = 6 + bt, v = 5, O = a.fontSize * 1.25, M = T != null ? 2 : 1, W = M / 2;
        a.control.radius > 0 ? r.roundRect(W, W, Math.max(0, i - M), Math.max(0, o - M), a.control.radius) : r.rect(W, W, Math.max(0, i - M), Math.max(0, o - M)), r.fill(a.control.background), r.stroke({ width: M, color: T != null ? T : a.control.border });
        var N = (w = x == null ? void 0 : x.value) != null ? w : "", B = Math.max(0, i - D * 2);
        g && f.fieldBounds.set(g, { x: s, y: l, w: i, h: o, innerLeft: D, innerTop: $, innerWidth: B, maxLines: v, isPassword: !1 });
        var m = ce(N, B, d), R = ue(m, v), G = R.length > 0 ? R[R.length - 1].end : 0;
        if (g && x && typeof x.value == "string") {
            var y = x.selections;
            if (y && y.size > 0)
                try {
                    for (var _f = __values(y.entries()), _g = _f.next(); !_g.done; _g = _f.next()) {
                        var _h = __read(_g.value, 2), I = _h[0], H = _h[1];
                        var A = b((C = H.start) != null ? C : 0, 0, N.length), X = b((S = H.end) != null ? S : A, 0, N.length), it = b(Math.min(A, X), 0, G), et = b(Math.max(A, X), 0, G);
                        if (it === et)
                            continue;
                        var Y = Et(n, "__sel:".concat(I));
                        Mt(Y), Y.zIndex = 0, Y.visible = !0;
                        for (var V = 0; V < R.length; V++) {
                            var lt = R[V], tt = Math.max(it, lt.start), nt = Math.min(et, lt.end);
                            if (tt >= nt)
                                continue;
                            var ot = D + d(N.slice(lt.start, tt)), gt = d(N.slice(tt, nt));
                            Y.rect(ot, $ + V * O, gt, O);
                        }
                        Y.fill({ color: U(I), alpha: .22 });
                    }
                }
                catch (e_12_1) { e_12 = { error: e_12_1 }; }
                finally {
                    try {
                        if (_g && !_g.done && (_b = _f.return)) _b.call(_f);
                    }
                    finally { if (e_12) throw e_12.error; }
                }
            if (k && _ != null) {
                var I = (P = x.selections) == null ? void 0 : P.get(_), H = I ? I.end : 0, A = b(H, 0, G), X = Math.max(0, R.length - 1);
                for (var V = 0; V < R.length; V++) {
                    var lt = R[V];
                    if (A >= lt.start && A <= lt.end) {
                        X = V;
                        break;
                    }
                }
                var it = (F = R[X]) != null ? F : { start: 0, end: 0, text: "" }, et = D + d(N.slice(it.start, A)), Y = Et(n, "__caret");
                Mt(Y), Y.zIndex = 2, Y.visible = !0, Y.moveTo(et, $ + X * O), Y.lineTo(et, $ + X * O + O), Y.stroke({ width: 1, color: T != null ? T : a.control.focusBorder });
            }
        }
        var j = R.map(function (y) { return y.text; }).join("\n"), K = St(n, "__valueText", function (y) { y.style = { fontFamily: a.fontFamily, fontSize: a.fontSize, fill: a.text, fontWeight: "400", wordWrap: !1 }, y.zIndex = 1; });
        K.text = j, K.position.set(D, $), g && (Ht(n), n.eventMode = "static", n.cursor = "text", n.hitArea = new pt(0, 0, Math.max(0, i), Math.max(0, o)), n.on("pointerdown", function (y) {
            var e_13, _a, e_14, _b;
            var A, X, it, et, Y, V, lt, tt, nt, ot;
            if ((y == null ? void 0 : y.button) === 2)
                return;
            var I = t.getPointerId ? t.getPointerId(y) : Number((it = (X = y == null ? void 0 : y.pointerId) != null ? X : (A = y == null ? void 0 : y.data) == null ? void 0 : A.pointerId) != null ? it : 0);
            if (I <= 0)
                return;
            f.focusedKeyByPointer.set(I, g), f.keyboardOwnerPointerId = I;
            var H = h(g, pe(ee({}, (et = e.attrs) != null ? et : {}), { type: "text" }));
            if (typeof H.value == "string") {
                try {
                    for (var _c = __values(f.inputs.entries()), _d = _c.next(); !_d.done; _d = _c.next()) {
                        var _f = __read(_d.value, 2), Ot = _f[0], Dt = _f[1];
                        Ot !== g && ((Y = Dt.selections) == null || Y.delete(I));
                    }
                }
                catch (e_13_1) { e_13 = { error: e_13_1 }; }
                finally {
                    try {
                        if (_d && !_d.done && (_a = _c.return)) _a.call(_c);
                    }
                    finally { if (e_13) throw e_13.error; }
                }
                var gt = f.fieldBounds.get(g), It = (V = gt == null ? void 0 : gt.innerWidth) != null ? V : Math.max(0, i - D * 2), at = H.value, Tt = ue(ce(at, It, d), v), Lt = ((tt = (lt = y.global) == null ? void 0 : lt.x) != null ? tt : 0) - s - D, $t = ((ot = (nt = y.global) == null ? void 0 : nt.y) != null ? ot : 0) - l - $, Nt = Me({ fullText: at, lines: Tt, localX: Lt, localY: $t, lineHeight: O, measure: d });
                H.selections || (H.selections = new Map), H.selections.set(I, { start: Nt, end: Nt });
                try {
                    for (var _g = __values(c.entries()), _h = _g.next(); !_h.done; _h = _g.next()) {
                        var _j = __read(_h.value, 2), Ot = _j[0], Dt = _j[1];
                        Dt.key === g && Ot !== I && c.delete(Ot);
                    }
                }
                catch (e_14_1) { e_14 = { error: e_14_1 }; }
                finally {
                    try {
                        if (_h && !_h.done && (_b = _g.return)) _b.call(_g);
                    }
                    finally { if (e_14) throw e_14.error; }
                }
                c.set(I, { key: g, anchor: Nt });
            }
            E == null || E();
        }));
    }
    function Lr(t, e) { t.setFlexDirection(e.FLEX_DIRECTION_ROW), t.setAlignItems(e.ALIGN_CENTER), t.setJustifyContent(e.JUSTIFY_FLEX_START), t.setPadding(e.EDGE_LEFT, 8), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0); }
    function zi(t, e, n, r, i) { t.circle(e, n, r), t.stroke({ width: 2, color: i }); var o = e + r * .65, s = n + r * .65, l = e + r * 1.55, a = n + r * 1.55; t.moveTo(o, s), t.lineTo(l, a), t.stroke({ width: 2, color: i }); }
    function Br(t, e) { t.setFlexDirection(e.FLEX_DIRECTION_ROW), t.setFlexWrap(e.WRAP_NO_WRAP), t.setAlignItems(e.ALIGN_CENTER), t.setJustifyContent(e.JUSTIFY_FLEX_START), t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0); }
    function Fr(t, e) { t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0), t.setWidth(36), t.setHeight(36), t.setMinWidth(36), t.setMinHeight(36), t.setFlexGrow(0), t.setFlexShrink(0), t.setMargin(e.EDGE_RIGHT, 6); }
    function Hr(t) { var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.theme, l = t.uiState, a = t.getPointerId, d = t.focusInputKey, f = t.requestPaint, h = function (c) { r.clear(); var E = 1, g = E / 2; s.control.button.radius > 0 ? r.roundRect(g, g, Math.max(0, i - E), Math.max(0, o - E), s.control.button.radius) : r.rect(g, g, Math.max(0, i - E), Math.max(0, o - E)), r.fill(c), r.stroke({ width: E, color: s.control.button.border }); var x = i / 2 - 2, k = o / 2 - 2, _ = Math.max(5, Math.min(7, Math.min(i, o) * .22)); zi(r, x, k, _, s.text); }; h(s.control.button.fill), Ht(n), n.eventMode = "static", n.cursor = "pointer", n.hitArea = new pt(0, 0, Math.max(0, i), Math.max(0, o)), n.on("pointerover", function () { return h(s.control.button.hoverFill); }), n.on("pointerout", function () { return h(s.control.button.fill); }), n.on("pointerdown", function (c) { var E; if ((c == null ? void 0 : c.button) !== 2) {
        if (h(s.control.button.activeFill), d) {
            var g = a(c);
            g > 0 && (l.focusedKeyByPointer.set(g, d), l.keyboardOwnerPointerId = g);
        }
        f == null || f(), (E = c.stopPropagation) == null || E.call(c);
    } }), n.on("pointerup", function () { return h(s.control.button.hoverFill); }); var b = e.attrs; }
    function Je(t, e) { var n = t.get(e); if (n)
        return n; var r = { x: 0, y: 0 }; return t.set(e, r), r; }
    function Wr(t, e) { t.setPositionType(e.POSITION_TYPE_ABSOLUTE), t.setPosition(e.EDGE_LEFT, 0), t.setPosition(e.EDGE_TOP, 0), t.setAlignSelf(e.ALIGN_FLEX_START), t.setFlexGrow(0), t.setFlexShrink(0), t.setPadding(e.EDGE_LEFT, 12), t.setPadding(e.EDGE_RIGHT, 12), t.setPadding(e.EDGE_TOP, 12), t.setPadding(e.EDGE_BOTTOM, 12), t.setWidth(540), t.setMinWidth(360), t.setMinHeight(148); }
    function $r(t) { var U, D; var e = t.node, n = t.container, r = t.w, i = t.h, o = t.theme, s = t.selectedBy, l = t.getCursorColor, a = t.dialogStates, d = t.dialogDrags, f = t.bringToFront, h = t.requestPaint, b = e.key; if (!b)
        return; var c = s.get(b), E = c == null ? o.boxBorder : l(c), g = Math.max(0, Math.round(r)), x = Math.max(0, Math.round(i)), k = Et(n, "__dialogBorder"); Mt(k), k.rect(0, 0, g, x), k.fill({ color: 16777215, alpha: .8 }); var _ = c == null ? 1 : 2, T = _ / 2; k.rect(T, T, Math.max(0, g - _), Math.max(0, x - _)), k.stroke({ width: _, color: E, alignment: 0 }), k.eventMode = "static", k.cursor = "move", k.hitArea = new pt(0, 0, g, x), k.on("pointerdown", function ($) {
        var e_15, _a;
        var W, N, B, m, R, G, j, K;
        var v = function (q) { try {
            typeof console != "undefined" && typeof console.log == "function" && console.log("[dialog pointerdown] ".concat(q));
        }
        catch (z) { } };
        if (v("start"), ($ == null ? void 0 : $.button) === 2)
            return;
        v("pointer-id");
        var O = t.getPointerId ? t.getPointerId($) : Number((B = (N = $ == null ? void 0 : $.pointerId) != null ? N : (W = $ == null ? void 0 : $.data) == null ? void 0 : W.pointerId) != null ? B : 0);
        if (O <= 0 || O <= 0)
            return;
        v("clear-other-drags");
        try {
            for (var _b = __values(d.entries()), _c = _b.next(); !_c.done; _c = _b.next()) {
                var _d = __read(_c.value, 2), q = _d[0], z = _d[1];
                z.key === b && q !== O && d.delete(q);
            }
        }
        catch (e_15_1) { e_15 = { error: e_15_1 }; }
        finally {
            try {
                if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
            }
            finally { if (e_15) throw e_15.error; }
        }
        v("select"), s.set(b, O), v("bring-to-front"), f == null || f(b), v("state");
        var M = Je(a, b);
        v("set-drag"), d.set(O, { key: b, startGX: (R = (m = $.global) == null ? void 0 : m.x) != null ? R : 0, startGY: (j = (G = $.global) == null ? void 0 : G.y) != null ? j : 0, originX: M.x, originY: M.y }), v("request-paint"), h == null || h(), v("stop-propagation"), (K = $.stopPropagation) == null || K.call($), v("done");
    }); {
        var $ = n.getChildByLabel, v = (D = (U = $ == null ? void 0 : $.call(n, "__children")) != null ? U : n.children.find(function (O) { return O && O.label === "__children"; })) != null ? D : null;
        if (v && k.parent === n) {
            var O = n.getChildIndex(v), M = Math.max(0, n.children.length - 1), W = Math.max(0, Math.min(O - 1, M));
            n.getChildIndex(k) > W && n.setChildIndex(k, W);
        }
    } }
    function _n(t, e, n) { var l; var r = t.get(e); if (r)
        return r; var i = Number((l = n == null ? void 0 : n.value) != null ? l : "0"), s = { value: Number.isFinite(i) ? i : 0 }; return t.set(e, s), s; }
    function Ur(t, e) { t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0), t.setHeight(36), t.setMinHeight(36), t.setMinWidth(140), t.setFlexGrow(0), t.setFlexShrink(0); }
    function ji(t, e, n) { return Math.max(e, Math.min(n, t)); }
    function wn(t, e, n) { var i; var r = Number((i = t == null ? void 0 : t[e]) != null ? i : ""); return Number.isFinite(r) ? r : n; }
    function Vi(t, e, n, r, i, o) { var l = e + 3, a = e + r - 3, d = n + 3, f = n + i - 3; t.moveTo(l, f), t.lineTo((l + a) / 2, d), t.lineTo(a, f), t.stroke({ width: 2, color: o }); }
    function Ji(t, e, n, r, i, o) { var l = e + 3, a = e + r - 3, d = n + 3, f = n + i - 3; t.moveTo(l, d), t.lineTo((l + a) / 2, f), t.lineTo(a, d), t.stroke({ width: 2, color: o }); }
    function Xr(t) { var B; var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.theme, l = t.getValue, a = t.setValue, d = t.requestPaint, f = e.key, h = e.attrs, b = wn(h, "min", 0), c = wn(h, "max", 255), E = Math.max(1e-9, wn(h, "step", 1)), g = l(), x = 1, k = x / 2; r.rect(k, k, Math.max(0, i - x), Math.max(0, o - x)), r.fill(s.control.background), r.stroke({ width: x, color: s.control.border }); var _ = 22, T = Math.max(0, i - _); r.moveTo(T + .5, 0), r.lineTo(T + .5, o), r.stroke({ width: 1, color: s.control.border, alignment: 0 }); var U = Et(n, "__arrows"); Mt(U), Vi(U, T, 0, _, o / 2, s.text), Ji(U, T, o / 2, _, o / 2, s.text); var D = ((B = h == null ? void 0 : h.channel) != null ? B : "").toLowerCase(), $ = D === "r" ? "R" : D === "g" ? "G" : D === "b" ? "B" : D === "a" ? "A" : "", v = St(n, "__valueText", function (m) { m.style = { fontFamily: s.fontFamily, fontSize: s.fontSize, fill: s.text, fontWeight: "400", wordWrap: !1 }; }); if (v.text = $ ? "".concat($, ": ").concat(Math.round(g)) : String(Math.round(g)), v.position.set(8, 9 + bt), !f)
        return; var O = new pt(T, 0, _, o / 2), M = new pt(T, o / 2, _, o / 2), W = function (m) { var R = l(), G = ji(R + m * E, b, c); a(G), d == null || d(); }, N = Et(n, "__hit"); Mt(N), N.eventMode = "static", N.cursor = "default", N.hitArea = new pt(0, 0, Math.max(0, i), Math.max(0, o)), N.on("pointerdown", function (m) {
        var e_16, _a;
        var p, w, C, S, P, F;
        if ((m == null ? void 0 : m.button) === 2)
            return;
        var R = t.getPointerId ? t.getPointerId(m) : Number((C = (w = m == null ? void 0 : m.pointerId) != null ? w : (p = m == null ? void 0 : m.data) == null ? void 0 : p.pointerId) != null ? C : 0);
        if (R <= 0)
            return;
        var G = n.toLocal(m.global), j = (S = G == null ? void 0 : G.x) != null ? S : 0, K = (P = G == null ? void 0 : G.y) != null ? P : 0, q = O.contains(j, K) ? 1 : M.contains(j, K) ? -1 : null;
        if (!q)
            return;
        W(q);
        var z = t.numberHolds;
        if (z && f) {
            try {
                for (var _b = __values(z.entries()), _c = _b.next(); !_c.done; _c = _b.next()) {
                    var _d = __read(_c.value, 2), H = _d[0], A = _d[1];
                    H !== R && (A.timeoutId != null && window.clearTimeout(A.timeoutId), A.intervalId != null && window.clearInterval(A.intervalId), z.delete(H));
                }
            }
            catch (e_16_1) { e_16 = { error: e_16_1 }; }
            finally {
                try {
                    if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
                }
                finally { if (e_16) throw e_16.error; }
            }
            var y = z.get(R);
            y && (y.timeoutId != null && window.clearTimeout(y.timeoutId), y.intervalId != null && window.clearInterval(y.intervalId));
            var I_1 = { key: f, timeoutId: null, intervalId: null };
            I_1.timeoutId = window.setTimeout(function () { I_1.timeoutId = null, I_1.intervalId = window.setInterval(function () { W(q); }, 250); }, 500), z.set(R, I_1);
        }
        (F = m.stopPropagation) == null || F.call(m);
    }); }
    var Ze = null;
    function Yr() { return Ze || (Ze = new ze({ data: re, label: "attribute-color-picker-colors", shrinkToFit: !1, usage: bn.VERTEX | bn.COPY_DST }), Ze); }
    function Kr(t, e, n) { var d, f, h, b; t.setPadding(n.EDGE_LEFT, 0), t.setPadding(n.EDGE_RIGHT, 0), t.setPadding(n.EDGE_TOP, 0), t.setPadding(n.EDGE_BOTTOM, 0); var r = Number((f = (d = e.attrs) == null ? void 0 : d.width) != null ? f : "0"), i = Number((b = (h = e.attrs) == null ? void 0 : h.height) != null ? b : "0"), o = Number.isFinite(r) && r > 0, s = Number.isFinite(i) && i > 0, l = o ? r : 240, a = s ? i : 200; (o || s) && (t.setAlignSelf(n.ALIGN_FLEX_START), t.setFlexGrow(0), t.setFlexShrink(0)), t.setWidth(l), t.setHeight(a), t.setMinWidth(Math.min(240, l)), t.setMinHeight(Math.min(200, a)); }
    function oe(t) { return Number.isFinite(t) ? Math.max(0, Math.min(255, Math.round(t))) : 0; }
    function Qe(t) { return oe(t).toString(16).padStart(2, "0"); }
    function Zi(t, e, n, r, i, o, s, l) { var a = s - n, d = l - r, f = i - n, h = o - r, b = t - n, c = e - r, E = a * a + d * d, g = a * f + d * h, x = a * b + d * c, k = f * f + h * h, _ = f * b + h * c, T = 1 / (E * k - g * g), U = (k * x - g * _) * T, D = (E * _ - g * x) * T; return U >= 0 && D >= 0 && U + D <= 1; }
    function Qi(t, e, n, r, i, o, s, l) { var a = i - n, d = o - r, f = s - n, h = l - r, b = t - n, c = e - r, E = a * h - f * d; if (Math.abs(E) < 1e-9)
        return { w0: 1, w1: 0, w2: 0 }; var g = (b * h - f * c) / E, x = (a * c - b * d) / E; return { w0: 1 - g - x, w1: g, w2: x }; }
    var qi = { name: "solid-out", fragment: { main: "\n      outColor = vec4(1.0);\n    " } }, qe = null;
    function to() { if (qe)
        return qe; var t = { name: "color-picker-vertex-color", bits: [Kn, zn, Yn, qi] }; return qe = new Re({ glProgram: t, resources: {} }), qe; }
    function zr(t, e, n) { var r = new Float32Array(12), i = [-90, -30, 30, 90, 150, 210]; for (var o = 0; o < 6; o++) {
        var s = i[o] * Math.PI / 180;
        r[o * 2 + 0] = t + Math.cos(s) * n, r[o * 2 + 1] = e + Math.sin(s) * n;
    } return r; }
    var re = new Uint8Array([255, 0, 0, 255, 128, 128, 0, 255, 0, 255, 0, 255, 0, 128, 128, 255, 0, 0, 255, 255, 128, 0, 128, 255]), ke = new Uint32Array([0, 1, 2, 0, 2, 3, 0, 3, 4, 0, 4, 5]);
    function Tn(t) { var e = t.lx, n = t.ly, r = t.w, i = t.h, o = 10, s = Math.max(0, r - o * 2), l = Math.max(0, i - o * 2), a = o + s / 2, d = o + l / 2, f = Math.max(0, Math.min(s, l) / 2 - 2), h = zr(a, d, f); for (var b = 0; b < ke.length; b += 3) {
        var c = ke[b + 0], E = ke[b + 1], g = ke[b + 2], x = h[c * 2 + 0], k = h[c * 2 + 1], _ = h[E * 2 + 0], T = h[E * 2 + 1], U = h[g * 2 + 0], D = h[g * 2 + 1];
        if (!Zi(e, n, x, k, _, T, U, D))
            continue;
        var $ = Qi(e, n, x, k, _, T, U, D), v = c * 4, O = E * 4, M = g * 4, W = $.w0 * re[v + 0] + $.w1 * re[O + 0] + $.w2 * re[M + 0], N = $.w0 * re[v + 1] + $.w1 * re[O + 1] + $.w2 * re[M + 1], B = $.w0 * re[v + 2] + $.w1 * re[O + 2] + $.w2 * re[M + 2];
        return { r: oe(W), g: oe(N), b: oe(B) };
    } return null; }
    function jr(t) { var z, p; var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.theme, l = t.rgb, a = t.setRgb, d = t.alpha, f = t.setAlpha, h = t.pick, b = t.setPick, c = t.requestPaint, E = t.getPointerId, g = t.setDraggingPointerId, x = 1, k = x / 2; r.rect(k, k, Math.max(0, i - x), Math.max(0, o - x)), r.fill(16777215), r.stroke({ width: x, color: s.control.border, alignment: 0 }); var _ = 10, T = Math.max(0, i - _ * 2), U = Math.max(0, o - _ * 2), D = _ + T / 2, $ = _ + U / 2, v = Math.max(0, Math.min(T, U) / 2 - 2), O = zr(D, $, v), M = "".concat(Math.round(i), "x").concat(Math.round(o)), W = n.getChildByLabel, N = W ? W.call(n, "__mesh") : n.children.find(function (w) { return (w == null ? void 0 : w.label) === "__mesh"; }); if (N) {
        if (N.__sizeKey !== M) {
            var w = new Float32Array(O.length), C = new _e({ positions: O, uvs: w, indices: ke });
            C.addAttribute("aColor", { buffer: Yr(), format: "unorm8x4", stride: 4, offset: 0 });
            try {
                (p = (z = N.geometry) == null ? void 0 : z.destroy) == null || p.call(z);
            }
            catch (S) { }
            N.geometry = C, N.__sizeKey = M;
        }
    }
    else {
        var w = new Float32Array(O.length), C = new _e({ positions: O, uvs: w, indices: ke });
        C.addAttribute("aColor", { buffer: Yr(), format: "unorm8x4", stride: 4, offset: 0 }), N = new Ke({ geometry: C, shader: to() }), N.label = "__mesh", n.addChild(N), N.__sizeKey = M;
    } N.removeAllListeners(), N.eventMode = "static", N.cursor = "crosshair", N.hitArea = new pt(_, _, T, U), N.on("pointerdown", function (w) { var I, H, A; if ((w == null ? void 0 : w.button) === 2)
        return; var C = E(w); if (C <= 0)
        return; var S = n.toLocal(w.global), P = (I = S == null ? void 0 : S.x) != null ? I : 0, F = (H = S == null ? void 0 : S.y) != null ? H : 0, y = Tn({ lx: P, ly: F, w: i, h: o }); y && (b({ x: P, y: F }), a(y), g(C), c == null || c(), (A = w.stopPropagation) == null || A.call(w)); }); {
        var w = Et(n, "__border");
        Mt(w), w.moveTo(O[0], O[1]);
        for (var C = 1; C < 6; C++)
            w.lineTo(O[C * 2 + 0], O[C * 2 + 1]);
        w.closePath(), w.stroke({ width: 2, color: 0 });
    } var B = Et(n, "__overlay"); Mt(B); var m = 44, R = 18, G = Math.max(_, i - _ - m), j = _; B.rect(G, j, m, R), B.fill({ color: oe(l.r) << 16 | oe(l.g) << 8 | oe(l.b), alpha: Math.max(0, Math.min(1, oe(d) / 255)) }), B.rect(G + .5, j + .5, m - 1, R - 1), B.stroke({ width: 1, color: s.control.border, alignment: 0 }), h && (B.circle(h.x, h.y, 4), B.stroke({ width: 2, color: 16777215 }), B.circle(h.x, h.y, 4), B.stroke({ width: 1, color: 0 })); var K = "#".concat(Qe(l.r)).concat(Qe(l.g)).concat(Qe(l.b)).concat(Qe(d)).toUpperCase(), q = St(n, "__label", function (w) { w.style = { fontFamily: s.fontFamily, fontSize: Math.max(10, Math.floor(s.fontSize * .75)), fill: s.mutedText, fontWeight: "400", wordWrap: !1 }; }); q.text = K, q.position.set(_, Math.max(_, o - _ - q.height)), f && f(oe(d)); }
    function de(t, e, n) { var r = t.get(e); if (r)
        return r; var i = { selectedIndex: Math.max(0, n | 0), open: !1 }; return t.set(e, i), i; }
    function Vr(t, e) { t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0), t.setHeight(36), t.setMinHeight(36), t.setMinWidth(220); }
    function eo(t, e, n, r, i, o) { var l = e + 4, a = e + r - 4, d = n + 4, f = n + i - 4; t.moveTo(l, (d + f) / 2 - 2), t.lineTo((l + a) / 2, (d + f) / 2 + 2), t.lineTo(a, (d + f) / 2 - 2), t.stroke({ width: 2, color: o }); }
    function no(t) {
        var r;
        var n = String((r = t == null ? void 0 : t["data-options"]) != null ? r : "").split("\n").map(function (i) { return i.trim(); }).filter(function (i) { return i.length > 0; });
        return n.length > 0 ? n : ["(empty)"];
    }
    function ro(t) { var n; var e = Number((n = t == null ? void 0 : t["data-selected-index"]) != null ? n : "0"); return Number.isFinite(e) ? Math.max(0, e | 0) : 0; }
    function tn(t) { var N; var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.absX, l = t.absY, a = t.theme, d = t.selectStates, f = t.uiState, h = t.getPointerId, b = t.getCursorColor, c = t.requestPaint, E = t.popupSink, g = e.key; if (!g)
        return; var x = no(e.attrs), k = ro(e.attrs), _ = de(d, g, k); _.selectedIndex = Math.max(0, Math.min(x.length - 1, _.selectedIndex | 0)); var T = (function () {
        var e_17, _a;
        var B = f.keyboardOwnerPointerId;
        if (f.focusedKeyByPointer.get(B) === g)
            return B;
        try {
            for (var _b = __values(f.focusedKeyByPointer.entries()), _c = _b.next(); !_c.done; _c = _b.next()) {
                var _d = __read(_c.value, 2), m = _d[0], R = _d[1];
                if (R === g)
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
    })(), U = T != null ? b(T) : null, D = U != null ? 2 : 1, $ = D / 2; a.control.radius > 0 ? r.roundRect($, $, Math.max(0, i - D), Math.max(0, o - D), a.control.radius) : r.rect($, $, Math.max(0, i - D), Math.max(0, o - D)), r.fill(a.control.background), r.stroke({ width: D, color: U != null ? U : a.control.border }); var v = 22, O = Math.max(0, i - v); r.moveTo(O + .5, 0), r.lineTo(O + .5, o), r.stroke({ width: 1, color: a.control.border, alignment: 0 }), eo(r, O, 0, v, o, a.text); var M = (N = x[_.selectedIndex]) != null ? N : "", W = St(n, "__label", function (B) { B.style = { fontFamily: a.fontFamily, fontSize: a.fontSize, fill: a.text, fontWeight: "400", wordWrap: !1 }; }); W.text = M, W.position.set(8, 9 + bt), Ht(n), n.eventMode = "static", n.cursor = "pointer", n.hitArea = new pt(0, 0, Math.max(0, i), Math.max(0, o)), n.on("pointerdown", function (B) { var R; if ((B == null ? void 0 : B.button) === 2)
        return; var m = h(B); m <= 0 || (f.focusedKeyByPointer.set(m, g), f.keyboardOwnerPointerId = m, _.open = !_.open, c == null || c(), (R = B.stopPropagation) == null || R.call(B)); }), _.open && E.push({ key: g, absX: s, absY: l, w: i, h: o, options: x, selectedIndex: _.selectedIndex }); }
    function Jr(t) { var _; var e = t.popup, n = t.stage, r = t.theme, i = t.selectStates, o = t.uiState, s = t.getPointerId, l = t.requestPaint, a = t.viewportW, d = t.viewportH, f = 30, b = Math.min(7, e.options.length), c = b * f, E = e.absX, g = e.absY + e.h; E = Math.max(0, Math.min(E, Math.max(0, a - e.w))), g + c > d - 4 && (g = e.absY - c), g = Math.max(0, Math.min(g, Math.max(0, d - c))); var x = new _t; x.position.set(E, g), n.addChild(x); var k = new wt; k.rect(0, 0, e.w, c), k.fill(16777215), k.rect(.5, .5, Math.max(0, e.w - 1), Math.max(0, c - 1)), k.stroke({ width: 1, color: r.control.border, alignment: 0 }), x.addChild(k), x.eventMode = "static", x.cursor = "pointer", x.hitArea = new pt(0, 0, e.w, c), x.on("pointerdown", function (T) { var W, N, B; if ((T == null ? void 0 : T.button) === 2)
        return; var U = s(T), D = x.toLocal(T.global), $ = (W = D == null ? void 0 : D.x) != null ? W : -1, v = (N = D == null ? void 0 : D.y) != null ? N : -1; if ($ < 0 || $ > e.w || v < 0 || v > c)
        return; var O = Math.max(0, Math.min(e.options.length - 1, Math.floor(v / f))), M = i.get(e.key); M && (M.selectedIndex = O, M.open = !1), U > 0 && (o.focusedKeyByPointer.set(U, e.key), o.keyboardOwnerPointerId = U), l == null || l(), (B = T.stopPropagation) == null || B.call(T); }); for (var T = 0; T < b; T++) {
        var U = T * f;
        if (T === e.selectedIndex) {
            var $ = new wt;
            $.rect(1, U + 1, Math.max(0, e.w - 2), f - 2), $.fill({ color: 0, alpha: .06 }), x.addChild($);
        }
        var D = Kt({ text: (_ = e.options[T]) != null ? _ : "", fontFamily: r.fontFamily, fontSize: r.fontSize, fill: r.text, wordWrap: !1 });
        D.position.set(8, U + 7 + bt), x.addChild(D);
    } }
    function Pt(t, e, n) { var r = Number.isFinite(t) ? t | 0 : 0; return Math.max(e, Math.min(n, r)); }
    function Xt(t) { var e = Pt(t, 0, 99); return e < 10 ? "0".concat(e) : String(e); }
    function Qt(t, e, n) { var r = Number(t); if (!Number.isFinite(r))
        return null; var i = Math.trunc(r); return i < e || i > n ? null : i; }
    function rn(t) { if (t.length !== 4)
        return null; for (var r = 0; r < t.length; r += 1) {
        var i = t.charCodeAt(r);
        if (i < 48 || i > 57)
            return null;
    } var e = Number(t); if (!Number.isFinite(e))
        return null; var n = e - 2e3; return n < 0 || n > 99 ? null : n; }
    function io(t) { var e = String(t != null ? t : "").trim().split(":"); if (e.length !== 2 && e.length !== 3)
        return null; var n = Qt(e[0], 0, 23), r = Qt(e[1], 0, 59), i = e.length === 3 ? Qt(e[2], 0, 59) : 0; return n == null || r == null || i == null ? null : { hour: n, minute: r, second: i }; }
    function oo(t) { var e = String(t != null ? t : "").trim().split("-"); if (e.length !== 2)
        return null; var n = rn(e[0]), r = Qt(e[1], 1, 12); return n == null || r == null ? null : { year2: n, month: r }; }
    function so(t) { var e = String(t != null ? t : "").trim().split("-"); if (e.length !== 3)
        return null; var n = rn(e[0]), r = Qt(e[1], 1, 12), i = Qt(e[2], 1, 31); if (n == null || r == null || i == null)
        return null; var o = Pt(Math.floor((i - 1) / 7) + 1, 1, 4); return { year2: n, month: r, weekIndex: o }; }
    function ao(t) { var e = String(t != null ? t : "").trim(), n = e.indexOf("-W"); if (n < 0)
        return null; var r = rn(e.slice(0, n)), i = Qt(e.slice(n + 2), 1, 48); if (r == null || i == null)
        return null; var o = Pt(Math.floor((i - 1) / 4) + 1, 1, 12), s = Pt((i - 1) % 4 + 1, 1, 4); return { year2: r, month: o, weekIndex: s }; }
    function lo(t) { var e = String(t != null ? t : "").trim(), n = e.indexOf("T"); if (n < 0 && (n = e.indexOf(" ")), n < 0)
        return null; var r = e.slice(0, n).split("-"), i = e.slice(n + 1).split(":"); if (r.length !== 3 || i.length !== 2 && i.length !== 3)
        return null; var o = rn(r[0]), s = Qt(r[1], 1, 12), l = Qt(r[2], 1, 31), a = Qt(i[0], 0, 23), d = Qt(i[1], 0, 59), f = i.length === 3 ? Qt(i[2], 0, 59) : 0; if (o == null || s == null || l == null || a == null || d == null || f == null)
        return null; var h = Pt(Math.floor((l - 1) / 7) + 1, 1, 4); return { year2: o, month: s, weekIndex: h, hour: a, minute: d, second: f }; }
    function en(t) { return "20".concat(Xt(t.year2), "-").concat(Xt(t.month)); }
    function co(t) { return (Pt(t.month, 1, 12) - 1) * 4 + Pt(t.weekIndex, 1, 4); }
    function nn(t) { return "20".concat(Xt(t.year2), "-W").concat(Xt(co(t))); }
    function Ee(t) { var e = (Pt(t.weekIndex, 1, 4) - 1) * 7 + 1; return "20".concat(Xt(t.year2), "-").concat(Xt(t.month), "-").concat(Xt(e)); }
    function Ge(t) { return "".concat(Xt(t.hour), ":").concat(Xt(t.minute), ":").concat(Xt(t.second)); }
    function ve(t) { return "".concat(Ee(t), "T").concat(Ge(t)); }
    function uo(t) { var f; var e = t.map, n = t.yearSliderOwners, r = t.inputKey, i = t.kind, o = t.attrs, s = e.get(r); if (s)
        return s.kind = i, s; var l = new Date, a = { kind: i, year2: Pt(l.getFullYear() - 2e3, 0, 99), month: Pt(l.getMonth() + 1, 1, 12), weekIndex: 1, hour: Pt(l.getHours(), 0, 23), minute: Pt(l.getMinutes(), 0, 59), second: Pt(l.getSeconds(), 0, 59), openPanel: null, openYear: !1, openMonthGrid: !1, yearSliderKey: "".concat(r, ":year-slider") }, d = String((f = o == null ? void 0 : o.value) != null ? f : ""); if (d.trim().length > 0) {
        if (i === "time") {
            var h = io(d);
            h && (a.hour = h.hour, a.minute = h.minute, a.second = h.second);
        }
        else if (i === "month") {
            var h = oo(d);
            h && (a.year2 = h.year2, a.month = h.month);
        }
        else if (i === "week") {
            var h = ao(d);
            h && (a.year2 = h.year2, a.month = h.month, a.weekIndex = h.weekIndex);
        }
        else if (i === "date") {
            var h = so(d);
            h && (a.year2 = h.year2, a.month = h.month, a.weekIndex = h.weekIndex);
        }
        else if (i === "datetime-local") {
            var h = lo(d);
            h && (a.year2 = h.year2, a.month = h.month, a.weekIndex = h.weekIndex, a.hour = h.hour, a.minute = h.minute, a.second = h.second);
        }
    } return e.set(r, a), n.set(a.yearSliderKey, r), a; }
    function Qr(t, e, n) { t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0), t.setHeight(36), t.setMinHeight(36), t.setMinWidth(n === "datetime-local" ? 340 : 220); }
    function mo(t, e, n, r, i) { var o = i != null ? 2 : 1, s = o / 2; e.control.radius > 0 ? t.roundRect(s, s, Math.max(0, n - o), Math.max(0, r - o), e.control.radius) : t.rect(s, s, Math.max(0, n - o), Math.max(0, r - o)), t.fill(e.control.background), t.stroke({ width: o, color: i != null ? i : e.control.border }); }
    function Zr(t, e, n, r, i) { var o = e + r / 2, s = n + r / 2; t.moveTo(e, s - 2), t.lineTo(o, s + 2), t.lineTo(e + r, s - 2), t.stroke({ width: 2, color: i }); }
    function qr(t) { var O, M; var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.absX, l = t.absY, a = t.theme, d = t.uiState, f = t.getPointerId, h = t.getCursorColor, b = t.temporalStates, c = t.yearSliderOwners, E = t.getOrInitInputValue, g = t.requestPaint, x = t.popupSink, k = e.key; if (!k || !e.tagName)
        return; var _ = e.tagName === "timeinput" ? "time" : e.tagName === "monthinput" ? "month" : e.tagName === "weekinput" ? "week" : e.tagName === "dateinput" ? "date" : "datetime-local", T = uo({ map: b, yearSliderOwners: c, inputKey: k, kind: _, attrs: e.attrs }), U = E(k, pe(ee({}, (O = e.attrs) != null ? O : {}), { type: "text" })); _ === "time" ? U.value = Ge(T) : _ === "month" ? U.value = en(T) : _ === "week" ? U.value = nn(T) : _ === "date" ? U.value = Ee(T) : U.value = ve(T); var D = (function () {
        var e_18, _a;
        var W = d.keyboardOwnerPointerId;
        if (d.focusedKeyByPointer.get(W) === k)
            return W;
        try {
            for (var _b = __values(d.focusedKeyByPointer.entries()), _c = _b.next(); !_c.done; _c = _b.next()) {
                var _d = __read(_c.value, 2), N = _d[0], B = _d[1];
                if (B === k)
                    return N;
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
    })(), $ = D != null ? h(D) : null; mo(r, a, i, o, $); var v = 8; if (_ !== "datetime-local") {
        var W = (M = U.value) != null ? M : "", N = St(n, "__shown", function (R) { R.style = { fontFamily: a.fontFamily, fontSize: a.fontSize, fill: a.text, fontWeight: "400", wordWrap: !1 }; });
        N.text = W, N.visible = !0, N.position.set(v, 9 + bt);
        var B = n.getChildByLabel ? n.getChildByLabel("__date") : n.children.find(function (R) { return (R == null ? void 0 : R.label) === "__date"; }), m = n.getChildByLabel ? n.getChildByLabel("__time") : n.children.find(function (R) { return (R == null ? void 0 : R.label) === "__time"; });
        B && (B.visible = !1), m && (m.visible = !1), Zr(r, Math.max(0, i - 18), 11, 10, a.text);
    }
    else {
        var W = Math.max(0, Math.round(i * .52));
        r.moveTo(W + .5, 0), r.lineTo(W + .5, o), r.stroke({ width: 1, color: a.control.border, alignment: 0 });
        var N = Ee(T), B = Ge(T), m = St(n, "__date", function (j) { j.style = { fontFamily: a.fontFamily, fontSize: a.fontSize, fill: a.text, fontWeight: "400", wordWrap: !1 }; });
        m.text = N, m.visible = !0, m.position.set(v, 9 + bt);
        var R = St(n, "__time", function (j) { j.style = { fontFamily: a.fontFamily, fontSize: a.fontSize, fill: a.text, fontWeight: "400", wordWrap: !1 }; });
        R.text = B, R.visible = !0, R.position.set(W + v, 9 + bt);
        var G = n.getChildByLabel ? n.getChildByLabel("__shown") : n.children.find(function (j) { return (j == null ? void 0 : j.label) === "__shown"; });
        G && (G.visible = !1), Zr(r, Math.max(W + 0, W + (i - W) - 18), 11, 10, a.text);
    } Ht(n), n.eventMode = "static", n.cursor = "pointer", n.hitArea = new pt(0, 0, Math.max(0, i), Math.max(0, o)), n.on("pointerdown", function (W) { var B, m, R; if ((W == null ? void 0 : W.button) === 2)
        return; var N = f(W); if (!(N <= 0)) {
        if (d.focusedKeyByPointer.set(N, k), d.keyboardOwnerPointerId = N, _ !== "datetime-local")
            T.openPanel = T.openPanel ? null : _ === "time" ? "time" : _ === "month" ? "month" : "week", T.openYear = !1, T.openMonthGrid = !1;
        else {
            var K = ((m = (B = W.global) == null ? void 0 : B.x) != null ? m : 0) - s <= i * .52;
            T.openPanel = K ? T.openPanel === "week" ? null : "week" : T.openPanel === "time" ? null : "time", T.openYear = !1, T.openMonthGrid = !1;
        }
        b.set(k, T), g == null || g(), (R = W.stopPropagation) == null || R.call(W);
    } }), T.openPanel === "month" ? x.push({ kind: "month-panel", inputKey: k, absX: s, absY: l, anchorW: i, anchorH: o }) : T.openPanel === "week" ? x.push({ kind: "week-panel", inputKey: k, absX: s, absY: l, anchorW: i, anchorH: o }) : T.openPanel === "time" && x.push({ kind: "time-panel", inputKey: k, absX: s, absY: l, anchorW: i, anchorH: o }); }
    function Ne(t, e, n, r) { t.rect(0, 0, n, r), t.fill(e.control.background), t.rect(.5, .5, Math.max(0, n - 1), Math.max(0, r - 1)), t.stroke({ width: 1, color: e.control.border, alignment: 0 }); }
    function ho(t) { var e = t.stage, n = t.theme, r = t.popup, i = t.st, o = t.viewportW, s = t.viewportH, l = t.getPointerId, a = t.requestPaint, d = t.onPick, f = 4, h = 3, b = 44, c = 34, E = 8, g = E * 2 + f * b, x = E * 2 + h * c, k = r.absX, _ = r.absY + r.anchorH; k = Math.max(0, Math.min(k, Math.max(0, o - g))), _ + x > s - 4 && (_ = r.absY - x), _ = Math.max(0, Math.min(_, Math.max(0, s - x))); var T = new _t; T.position.set(k, _), e.addChild(T); var U = new wt; Ne(U, n, g, x), T.addChild(U); for (var D = 0; D < 12; D++) {
        var $ = D + 1, v = E + D % f * b, O = E + Math.floor(D / f) * c;
        if ($ === i.month) {
            var W = new wt;
            W.rect(v + 1, O + 1, b - 2, c - 2), W.fill({ color: 0, alpha: .06 }), T.addChild(W);
        }
        var M = Kt({ text: String($), fontFamily: n.fontFamily, fontSize: n.fontSize, fill: n.text, wordWrap: !1 });
        M.position.set(v + 14, O + 8 + bt), T.addChild(M), U.rect(v, O, b, c), U.stroke({ width: 1, color: n.control.border, alignment: 0 });
    } T.eventMode = "static", T.cursor = "pointer", T.hitArea = new pt(0, 0, g, x), T.on("pointerdown", function (D) { var j, K, q; if ((D == null ? void 0 : D.button) === 2 || l(D) <= 0)
        return; var v = T.toLocal(D.global), O = (j = v == null ? void 0 : v.x) != null ? j : -1, M = (K = v == null ? void 0 : v.y) != null ? K : -1, W = O - E, N = M - E; if (W < 0 || N < 0)
        return; var B = Math.floor(W / b), m = Math.floor(N / c); if (B < 0 || B >= f || m < 0 || m >= h)
        return; var G = m * f + B + 1; G < 1 || G > 12 || (d(G), a == null || a(), (q = D.stopPropagation) == null || q.call(D)); }); }
    function fo(t) {
        var e_19, _a;
        var e = t.stage, n = t.theme, r = t.popup, i = t.st, o = t.viewportW, s = t.viewportH, l = t.sliders, a = t.sliderBounds, d = t.sliderDrags, f = t.getPointerId, h = t.requestPaint, b = t.onChange, c = 10, E = 250, g = 78, x = r.absX, k = r.absY;
        x = r.absX + r.anchorW + 6, k = r.absY, x = Math.max(0, Math.min(x, Math.max(0, o - E))), k = Math.max(0, Math.min(k, Math.max(0, s - g)));
        var _ = new _t;
        _.position.set(x, k), e.addChild(_);
        var T = new wt;
        Ne(T, n, E, g), _.addChild(T);
        var U = Kt({ text: "20".concat(Xt(i.year2)), fontFamily: n.fontFamily, fontSize: n.fontSize, fill: n.text, wordWrap: !1 });
        U.position.set(c, 8 + bt), _.addChild(U);
        var D = i.yearSliderKey, $ = Math.max(0, Math.min(1, Pt(i.year2, 0, 99) / 99)), v = ye(l, D, { value: String($) }), O = !1;
        try {
            for (var _b = __values(d.values()), _c = _b.next(); !_c.done; _c = _b.next()) {
                var B = _c.value;
                if (B.key === D) {
                    O = !0;
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
        O || (v.value = $);
        var M = new _t;
        M.position.set(c, 40), _.addChild(M);
        var W = new wt;
        M.addChild(W), Ve({ node: { key: D, attrs: { value: String(v.value) } }, container: M, graphics: W, w: E - c * 2, h: 14, absX: x + c, absY: k + 40, theme: { text: n.text, control: { progress: n.control.progress } }, sliderStates: l, sliderBounds: a, sliderDrags: d, requestPaint: h, getPointerId: f });
        var N = Pt(Math.round(v.value * 99), 0, 99);
        N !== i.year2 && b(N), _.eventMode = "static", _.hitArea = new pt(0, 0, E, g), _.on("pointerdown", function (B) { var m; (m = B.stopPropagation) == null || m.call(B); });
    }
    function po(t) { var e = t.panel, n = t.theme, r = t.x, i = t.y, o = t.w, s = t.st, l = t.onPick, a = 30, d = 6, f = []; for (var h = 0; h < 4; h++) {
        var b = h + 1, c = i + h * (a + d), E = new wt;
        E.rect(r, c, o, a), E.fill({ color: 0, alpha: b === s.weekIndex ? .06 : .03 }), E.rect(r + .5, c + .5, Math.max(0, o - 1), Math.max(0, a - 1)), E.stroke({ width: 1, color: n.control.border, alignment: 0 }), e.addChild(E);
        var g = (Pt(s.month, 1, 12) - 1) * 4 + b, x = Kt({ text: "".concat(b, " [").concat(Xt(g), "]"), fontFamily: n.fontFamily, fontSize: n.fontSize, fill: n.text, wordWrap: !1 });
        x.position.set(r + 10, c + 7 + bt), e.addChild(x), f.push({ x: r, y: c, w: o, h: a, weekIndex: b });
    } return { hitRects: f }; }
    function ti(t) {
        var e_20, _a, e_21, _b;
        var _, T, U, D, $, v;
        var e = t.popups, n = t.stage, r = t.theme, i = t.viewportW, o = t.viewportH, s = t.temporalStates, l = t.getOrInitInputValue, a = t.sliders, d = t.sliderBounds, f = t.sliderDrags, h = t.selects, b = t.selectPopups, c = t.getCursorColor, E = t.uiFocus, g = t.getPointerId, x = t.requestPaint, k = [];
        var _loop_1 = function (O) {
            var M = s.get(O.inputKey);
            if (M) {
                if (O.kind === "month-panel") {
                    var z = O.absX, p = O.absY + O.anchorH;
                    z = Math.max(0, Math.min(z, Math.max(0, i - 196))), p + 156 > o - 4 && (p = O.absY - 156), p = Math.max(0, Math.min(p, Math.max(0, o - 156)));
                    var w_1 = new _t;
                    w_1.position.set(z, p), n.addChild(w_1);
                    var C = new wt;
                    Ne(C, r, 196, 156), w_1.addChild(C);
                    var S_1 = { x: 10, y: 10, w: 132, h: 24 };
                    {
                        var y = new wt;
                        y.rect(S_1.x, S_1.y, S_1.w, S_1.h), y.fill({ color: 0, alpha: .03 }), y.rect(S_1.x + .5, S_1.y + .5, Math.max(0, S_1.w - 1), Math.max(0, S_1.h - 1)), y.stroke({ width: 1, color: r.control.border, alignment: 0 }), w_1.addChild(y);
                        var I = Kt({ text: "Year 20".concat(Xt(M.year2)), fontFamily: r.fontFamily, fontSize: r.fontSize, fill: r.text, wordWrap: !1 });
                        I.position.set(S_1.x + 8, S_1.y + 4 + bt), w_1.addChild(I);
                    }
                    var P_1 = 10, F_1 = 44;
                    for (var y = 0; y < 12; y++) {
                        var I = y + 1, H = P_1 + y % 4 * 44, A = F_1 + Math.floor(y / 4) * 34;
                        if (I === M.month) {
                            var it = new wt;
                            it.rect(H + 1, A + 1, 42, 32), it.fill({ color: 0, alpha: .06 }), w_1.addChild(it);
                        }
                        var X = Kt({ text: String(I), fontFamily: r.fontFamily, fontSize: r.fontSize, fill: r.text, wordWrap: !1 });
                        X.position.set(H + 14, A + 8 + bt), w_1.addChild(X), C.rect(H, A, 44, 34), C.stroke({ width: 1, color: r.control.border, alignment: 0 });
                    }
                    w_1.eventMode = "static", w_1.cursor = "pointer", w_1.hitArea = new pt(0, 0, 196, 156), w_1.on("pointerdown", function (y) { var gt, It, at, Tt; if ((y == null ? void 0 : y.button) === 2)
                        return; var I = g(y); if (I <= 0)
                        return; E.focusedKeyByPointer.set(I, O.inputKey), E.keyboardOwnerPointerId = I; var H = w_1.toLocal(y.global), A = (gt = H == null ? void 0 : H.x) != null ? gt : -1, X = (It = H == null ? void 0 : H.y) != null ? It : -1; if (A >= S_1.x && A <= S_1.x + S_1.w && X >= S_1.y && X <= S_1.y + S_1.h) {
                        M.openYear = !0, s.set(O.inputKey, M), x == null || x(), (at = y.stopPropagation) == null || at.call(y);
                        return;
                    } var et = A - P_1, Y = X - F_1; if (et < 0 || Y < 0)
                        return; var V = Math.floor(et / 44), lt = Math.floor(Y / 34); if (V < 0 || V >= 4 || lt < 0 || lt >= 3)
                        return; var nt = lt * 4 + V + 1; if (nt < 1 || nt > 12)
                        return; M.month = nt, M.openPanel = null, M.openYear = !1, M.openMonthGrid = !1, s.set(O.inputKey, M); var ot = l(O.inputKey, { type: "text" }); ot.value = en(M), x == null || x(), (Tt = y.stopPropagation) == null || Tt.call(y); }), w_1.on("pointerdown", function (y) { var I; (I = y.stopPropagation) == null || I.call(y); }), M.openYear && k.push({ kind: "year-panel", inputKey: O.inputKey, absX: z, absY: p, anchorW: 196, anchorH: 0 });
                }
                if (O.kind === "week-panel") {
                    var m = O.absX, R = O.absY + O.anchorH;
                    m = Math.max(0, Math.min(m, Math.max(0, i - 280))), R + 192 > o - 4 && (R = O.absY - 192), R = Math.max(0, Math.min(R, Math.max(0, o - 192)));
                    var G_1 = new _t;
                    G_1.position.set(m, R), n.addChild(G_1);
                    var j = new wt;
                    Ne(j, r, 280, 192), G_1.addChild(j);
                    var K_1 = { x: 10, y: 10, w: 104, h: 24 }, q_1 = { x: 10 + K_1.w + 10, y: 10, w: 120, h: 24 }, z = function (C, S) { var P = new wt; P.rect(C.x, C.y, C.w, C.h), P.fill({ color: 0, alpha: .03 }), P.rect(C.x + .5, C.y + .5, Math.max(0, C.w - 1), Math.max(0, C.h - 1)), P.stroke({ width: 1, color: r.control.border, alignment: 0 }), G_1.addChild(P); var F = Kt({ text: S, fontFamily: r.fontFamily, fontSize: r.fontSize, fill: r.text, wordWrap: !1 }); F.position.set(C.x + 8, C.y + 4 + bt), G_1.addChild(F); };
                    z(K_1, "Month ".concat(M.month)), z(q_1, "Year 20".concat(Xt(M.year2)));
                    var p = 44, w_2 = po({ panel: G_1, theme: r, x: 10, y: p, w: 280 - 10 * 2, st: M, onPick: function () { } }).hitRects;
                    G_1.eventMode = "static", G_1.cursor = "pointer", G_1.hitArea = new pt(0, 0, 280, 192), G_1.on("pointerdown", function (C) {
                        var e_23, _a;
                        var H, A, X, it, et;
                        if ((C == null ? void 0 : C.button) === 2)
                            return;
                        var S = g(C);
                        if (S <= 0)
                            return;
                        E.focusedKeyByPointer.set(S, O.inputKey), E.keyboardOwnerPointerId = S;
                        var P = G_1.toLocal(C.global), F = (H = P == null ? void 0 : P.x) != null ? H : -1, y = (A = P == null ? void 0 : P.y) != null ? A : -1, I = function (Y) { return F >= Y.x && F <= Y.x + Y.w && y >= Y.y && y <= Y.y + Y.h; };
                        if (I(K_1)) {
                            M.openMonthGrid = !M.openMonthGrid, s.set(O.inputKey, M), x == null || x(), (X = C.stopPropagation) == null || X.call(C);
                            return;
                        }
                        if (I(q_1)) {
                            M.openYear = !0, s.set(O.inputKey, M), x == null || x(), (it = C.stopPropagation) == null || it.call(C);
                            return;
                        }
                        try {
                            for (var w_3 = (e_23 = void 0, __values(w_2)), w_3_1 = w_3.next(); !w_3_1.done; w_3_1 = w_3.next()) {
                                var Y = w_3_1.value;
                                if (I(Y)) {
                                    M.weekIndex = Y.weekIndex;
                                    var V = l(O.inputKey, { type: "text" });
                                    M.kind === "week" ? V.value = nn(M) : M.kind === "date" ? V.value = Ee(M) : V.value = ve(M), M.openPanel = null, M.openYear = !1, M.openMonthGrid = !1, s.set(O.inputKey, M), x == null || x(), (et = C.stopPropagation) == null || et.call(C);
                                    return;
                                }
                            }
                        }
                        catch (e_23_1) { e_23 = { error: e_23_1 }; }
                        finally {
                            try {
                                if (w_3_1 && !w_3_1.done && (_a = w_3.return)) _a.call(w_3);
                            }
                            finally { if (e_23) throw e_23.error; }
                        }
                    }), M.openMonthGrid && k.push({ kind: "month-grid", inputKey: O.inputKey, absX: m, absY: R + K_1.y + K_1.h + 4, anchorW: 0, anchorH: 0 }), M.openYear && k.push({ kind: "year-panel", inputKey: O.inputKey, absX: m + q_1.x, absY: R + q_1.y, anchorW: q_1.w, anchorH: 0 });
                }
                if (O.kind === "time-panel") {
                    var m_1 = O.absX, R_1 = O.absY + O.anchorH;
                    m_1 = Math.max(0, Math.min(m_1, Math.max(0, i - 330))), R_1 + 80 > o - 4 && (R_1 = O.absY - 80), R_1 = Math.max(0, Math.min(R_1, Math.max(0, o - 80)));
                    var G_2 = new _t;
                    G_2.position.set(m_1, R_1), n.addChild(G_2);
                    var j = new wt;
                    Ne(j, r, 330, 80), G_2.addChild(j);
                    var K = Kt({ text: "Time", fontFamily: r.fontFamily, fontSize: r.fontSize, fill: r.text, wordWrap: !1 });
                    K.position.set(10, 8 + bt), G_2.addChild(K);
                    var q_2 = function (lt) { return Array.from({ length: lt }, function (tt, nt) { return Xt(nt); }).join("\n"); }, z = O.inputKey, p = "".concat(z, ":time-h"), w = "".concat(z, ":time-m"), C = "".concat(z, ":time-s"), S = de(h, p, Pt(M.hour, 0, 23)), P = de(h, w, Pt(M.minute, 0, 59)), F = de(h, C, Pt(M.second, 0, 59));
                    S.selectedIndex = Pt(M.hour, 0, 23), P.selectedIndex = Pt(M.minute, 0, 59), F.selectedIndex = Pt(M.second, 0, 59);
                    var y_1 = 96, I_2 = 36, H_1 = 32, A = 8, X = function (lt, tt, nt) { var ot = new _t; ot.position.set(tt, H_1), G_2.addChild(ot); var gt = new wt; ot.addChild(gt), tn({ node: { key: lt, attrs: { "data-options": q_2(nt), "data-selected-index": String(de(h, lt, 0).selectedIndex) } }, container: ot, graphics: gt, w: y_1, h: I_2, absX: m_1 + tt, absY: R_1 + H_1, theme: r, selectStates: h, uiState: E, getPointerId: g, getCursorColor: c, requestPaint: x, popupSink: b }); };
                    X(p, 10, 24), X(w, 10 + y_1 + A, 60), X(C, 10 + (y_1 + A) * 2, 60);
                    var it = Pt((T = (_ = h.get(p)) == null ? void 0 : _.selectedIndex) != null ? T : M.hour, 0, 23), et = Pt((D = (U = h.get(w)) == null ? void 0 : U.selectedIndex) != null ? D : M.minute, 0, 59), Y = Pt((v = ($ = h.get(C)) == null ? void 0 : $.selectedIndex) != null ? v : M.second, 0, 59);
                    M.hour = it, M.minute = et, M.second = Y, s.set(O.inputKey, M);
                    var V = l(O.inputKey, { type: "text" });
                    M.kind === "time" ? V.value = Ge(M) : V.value = ve(M), G_2.eventMode = "static", G_2.hitArea = new pt(0, 0, 330, 80), G_2.on("pointerdown", function (lt) { var tt; (tt = lt.stopPropagation) == null || tt.call(lt); });
                }
            }
        };
        try {
            for (var e_22 = __values(e), e_22_1 = e_22.next(); !e_22_1.done; e_22_1 = e_22.next()) {
                var O = e_22_1.value;
                _loop_1(O);
            }
        }
        catch (e_20_1) { e_20 = { error: e_20_1 }; }
        finally {
            try {
                if (e_22_1 && !e_22_1.done && (_a = e_22.return)) _a.call(e_22);
            }
            finally { if (e_20) throw e_20.error; }
        }
        var _loop_2 = function (O) {
            var M = s.get(O.inputKey);
            M && (O.kind === "month-grid" && ho({ stage: n, theme: r, popup: O, st: M, viewportW: i, viewportH: o, getPointerId: g, requestPaint: x, onPick: function (W) { M.month = W, M.openMonthGrid = !1, s.set(O.inputKey, M); var N = l(O.inputKey, { type: "text" }); M.kind === "month" ? N.value = en(M) : M.kind === "week" ? N.value = nn(M) : M.kind === "date" ? N.value = Ee(M) : N.value = ve(M); } }), O.kind === "year-panel" && fo({ stage: n, theme: r, popup: O, st: M, viewportW: i, viewportH: o, sliders: a, sliderBounds: d, sliderDrags: f, getPointerId: g, requestPaint: x, onChange: function (W) { M.year2 = W, s.set(O.inputKey, M); var N = l(O.inputKey, { type: "text" }); M.kind === "month" ? N.value = en(M) : M.kind === "week" ? N.value = nn(M) : M.kind === "date" ? N.value = Ee(M) : M.kind === "time" ? N.value = Ge(M) : N.value = ve(M); } }));
        };
        try {
            for (var k_1 = __values(k), k_1_1 = k_1.next(); !k_1_1.done; k_1_1 = k_1.next()) {
                var O = k_1_1.value;
                _loop_2(O);
            }
        }
        catch (e_21_1) { e_21 = { error: e_21_1 }; }
        finally {
            try {
                if (k_1_1 && !k_1_1.done && (_b = k_1.return)) _b.call(k_1);
            }
            finally { if (e_21) throw e_21.error; }
        }
    }
    function ei(t) {
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
    var ni = 5e4, Le = new WeakMap, ii = new Map, go = 1, oi = 0, bo = 0, ri = !1, xe = [], Mn = null;
    function Fe(t) { return t instanceof wt ? "Graphics" : t instanceof Zt ? "Text" : t instanceof _t ? "Container" : "Object"; }
    function yo(t) { var e = t && typeof t == "object" ? t.label : void 0, n = t && typeof t == "object" ? Fe(t) : "Object"; return e ? "".concat(n, ":").concat(String(e).slice(0, 80)) : n; }
    function me(t) { var e = Le.get(t); return e || (e = go++, Le.set(t, e)), ii.set(e, t), e; }
    function on(t) { var e, n, r, i, o, s; if (t == null || typeof t == "number" || typeof t == "string" || typeof t == "boolean")
        return t; if (Array.isArray(t))
        return t.slice(0, 16).map(on); if (typeof t == "object") {
        var l = t;
        return "color" in l || "alpha" in l || "width" in l && !("x" in l) && !("y" in l) && !("height" in l) ? { color: l.color, alpha: l.alpha, width: l.width } : "x" in l || "y" in l || "width" in l || "height" in l ? { x: Number((e = l.x) != null ? e : 0), y: Number((n = l.y) != null ? n : 0), w: Number((i = (r = l.width) != null ? r : l.w) != null ? i : 0), h: Number((s = (o = l.height) != null ? o : l.h) != null ? s : 0) } : Fe(l);
    } return String(t); }
    function En(t) { if (t != null)
        return typeof t == "symbol" ? t.toString() : String(t); }
    function si(t) { if (t != null)
        return typeof t == "function" ? { type: "function", name: t.name || void 0, arity: t.length } : typeof t == "object" ? { id: me(t), type: Fe(t) } : { type: typeof t }; }
    function xo(t) { if (t != null)
        return typeof t == "object" ? { id: me(t), type: Fe(t) } : typeof t == "function" ? { type: "function" } : { type: typeof t }; }
    function wo(t) { var e = { event: En(t[0]), listener: si(t[1]) }; return t.length > 2 && (e.context = xo(t[2])), [e]; }
    function _o(t) { return String(t != null ? t : "").slice(0, 240); }
    function To(t) {
        var e_25, _a;
        var r, i;
        if (!t || typeof t != "object")
            return on(t);
        var e = t, n = { type: (i = (r = t.constructor) == null ? void 0 : r.name) != null ? i : "object" };
        try {
            for (var _b = __values(["fontFamily", "fontSize", "fontStyle", "fontWeight", "fill", "align", "lineHeight", "letterSpacing", "wordWrap", "wordWrapWidth", "padding"]), _c = _b.next(); !_c.done; _c = _b.next()) {
                var o = _c.value;
                var s = e[o];
                s !== void 0 && (n[o] = on(s));
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
    function Mo(t) { var s, l, a, d, f, h; if (!t || typeof t != "object")
        return; var e = t, n = Number((s = e.x) != null ? s : 0), r = Number((l = e.y) != null ? l : 0), i = Number((d = (a = e.width) != null ? a : e.w) != null ? d : 0), o = Number((h = (f = e.height) != null ? f : e.h) != null ? h : 0); if (!(!Number.isFinite(n) || !Number.isFinite(r) || !Number.isFinite(i) || !Number.isFinite(o)) && !(i <= 0 || o <= 0))
        return { x: n, y: r, w: i, h: o }; }
    function ko(t, e) { if (e) {
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
        return t === "on" ? wo(e) : t === "snapshot" ? e : t === "text.text.set" ? e.length ? [_o(e[0])] : [] : t === "text.style.set" ? e.length ? [To(e[0])] : [] : e.map(on);
    } }
    function sn(t, e, n) { var r, i; try {
        window.__TRUEOS_PIXI_CAPTURE_STEP__ = "record:".concat(e, ":begin");
        var o = window.__pixiCapture;
        if (!(o != null && o.enabled))
            return;
        o.counts[e] = ((r = o.counts[e]) != null ? r : 0) + 1;
        var s = { frame: oi, seq: ++bo, op: e, id: t && typeof t == "object" ? me(t) : void 0, target: yo(t), event: e === "on" && (n != null && n.length) ? En(n[0]) : void 0, listener: e === "on" && (n != null && n.length) ? si(n[1]) : void 0, args: ko(e, n) };
        window.__TRUEOS_PIXI_CAPTURE_STEP__ = "record:".concat(e, ":push"), o.commands.push(s), o.persist && Eo(s), o.commands.length > ni && o.commands.splice(0, o.commands.length - ni), window.__TRUEOS_PIXI_CAPTURE_STEP__ = "record:".concat(e, ":done");
    }
    catch (o) {
        try {
            window.__TRUEOS_PIXI_CAPTURE_ERROR__ = "record:".concat(e, ":").concat(String((i = o == null ? void 0 : o.message) != null ? i : o));
        }
        catch (s) { }
    } }
    function Eo(t) { if (xe.push(t), t.op === "snapshot") {
        Be();
        return;
    } if (xe.length >= 512) {
        Be();
        return;
    } Mn == null && (Mn = window.setTimeout(function () { Mn = null, Be(); }, 50)); }
    function Be() {
        if (xe.length === 0)
            return;
        var t = xe;
        xe = [];
        var e = t.map(function (n) { return JSON.stringify(n); }).join("\n") + "\n";
        navigator.sendBeacon && navigator.sendBeacon("/__pixi_capture", new Blob([e], { type: "application/x-ndjson" })) || fetch("/__pixi_capture", { method: "POST", headers: { "Content-Type": "application/x-ndjson" }, body: e, keepalive: !0 }).catch(function () { xe = t.concat(xe); });
    }
    function So(t, e, n) {
        var e_26, _a, e_27, _b, e_28, _c;
        var r, i;
        if (e === "on") {
            var o = En(n[0]), s = n[1];
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
    function Po() { window.__TRUEOS_DISPATCH_PIXI_POINTER__ = function (t, e, n, r, i, o, s) {
        var e_29, _a;
        if (s === void 0) { s = 0; }
        var E, g, x, k, _, T, U, D, $, v, O, M, W, N;
        var l = function (B) { try {
            window.__TRUEOS_PIXI_POINTER_DISPATCH_STEP__ = B, typeof console != "undefined" && typeof console.log == "function" && console.log("[trueos pointer dispatch] ".concat(B));
        }
        catch (m) { } };
        l("start node=".concat(Number(t) || 0, " event=").concat(String(e || "")));
        var a = window.__TRUEOS_PIXI_APP;
        if (String(e || "") === "wheel") {
            var B = a == null ? void 0 : a.canvas;
            if (!B || typeof B.dispatchEvent != "function")
                return l("wheel-canvas-missing"), { handled: 0, listenerCount: 0, painted: 0, targetFound: 0 };
            var m = (x = (g = (E = window.__pixiCapture) == null ? void 0 : E.commands) == null ? void 0 : g.length) != null ? x : 0, R = { type: "wheel", deltaX: 0, deltaY: Number(s) || 0, deltaMode: 0, offsetX: Number(n) || 0, offsetY: Number(r) || 0, clientX: Number(n) || 0, clientY: Number(r) || 0, pointerId: Number(i) || 1, buttons: Number(o) || 0, defaultPrevented: !1, propagationStopped: !1, preventDefault: function () { this.defaultPrevented = !0; }, stopPropagation: function () { this.propagationStopped = !0; } };
            l("wheel-dispatch deltaY=".concat(R.deltaY)), B.dispatchEvent(R);
            var G = 0;
            if (window.__TRUEOS_CAPTURE_ONLY__) {
                var p = window.__TRUEOS_REPAINT_NOW__;
                window.__TRUEOS_PIXI_DIRTY__ && typeof p == "function" && (l("wheel-repaint-call"), p(), l("wheel-repaint-return"), G = 1);
            }
            else
                (k = a == null ? void 0 : a.renderer) != null && k.render && (a != null && a.stage) && (a.renderer.render(a.stage), G = 1);
            var j = (U = (T = (_ = window.__pixiCapture) == null ? void 0 : _.commands) == null ? void 0 : T.length) != null ? U : m, K = (D = B.listeners) == null ? void 0 : D.wheel, q = Array.isArray(K) ? K.length : typeof K == "function" ? 1 : 0, z = R.defaultPrevented || q > 0 ? 1 : 0;
            return l("wheel-done handled=".concat(z, " listeners=").concat(q, " painted=").concat(G)), { handled: z, listenerCount: q, painted: j > m || G ? 1 : 0, targetFound: 1 };
        }
        var d = ii.get(Number(t) || 0), f = 0, h = 0, b = 0;
        if (!d)
            return l("target-missing"), { handled: f, listenerCount: h, painted: b, targetFound: 0 };
        var c = { type: String(e || ""), button: Number(o) & 2 ? 2 : 0, buttons: Number(o) || 0, pointerId: Number(i) || 1, pointerType: "mouse", global: { x: Number(n) || 0, y: Number(r) || 0 }, data: { pointerId: Number(i) || 1, pointerType: "mouse", global: { x: Number(n) || 0, y: Number(r) || 0 } }, target: d, currentTarget: d, defaultPrevented: !1, propagationStopped: !1, preventDefault: function () { this.defaultPrevented = !0; }, stopPropagation: function () { this.propagationStopped = !0; } };
        l("target-found label=".concat(String(($ = d.label) != null ? $ : "")));
        for (var B = d; B; B = B.parent) {
            c.currentTarget = B;
            var m = (v = B.listeners) == null ? void 0 : v[c.type];
            if (!(!Array.isArray(m) || m.length === 0)) {
                h += m.length, l("listeners node=".concat((O = Le.get(B)) != null ? O : 0, " count=").concat(m.length));
                try {
                    for (var _b = (e_29 = void 0, __values(m.slice())), _c = _b.next(); !_c.done; _c = _b.next()) {
                        var R = _c.value;
                        if (typeof R == "function" && (f = 1, l("listener-call node=".concat((M = Le.get(B)) != null ? M : 0)), R.call(B, c), l("listener-return node=".concat((W = Le.get(B)) != null ? W : 0)), c.propagationStopped))
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
            var B = window.__TRUEOS_REPAINT_NOW__;
            window.__TRUEOS_PIXI_DIRTY__ && typeof B == "function" && (l("capture-repaint-call"), B(), l("capture-repaint-return"), b = 1);
        }
        else
            (N = a == null ? void 0 : a.renderer) != null && N.render && (a != null && a.stage) && (l("paint-call"), a.renderer.render(a.stage), l("paint-return"), b = 1);
        return l("done handled=".concat(f, " listeners=").concat(h, " painted=").concat(b)), { handled: f, listenerCount: h, painted: b, targetFound: 1 };
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
            if (sn(this, n, s), !window.__TRUEOS_CAPTURE_ONLY__)
                return r.apply(this, s);
            try {
                window.__TRUEOS_PIXI_CAPTURE_STEP__ = "invoke:".concat(n, ":begin");
                var a = So(this, n, s);
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
    function Io(t, e) { var n = t; for (; n;) {
        var r = Object.getOwnPropertyDescriptor(n, e);
        if (r)
            return r;
        n = Object.getPrototypeOf(n);
    } }
    function Se(t, e, n) { var o, s; if (!(t != null && t.constructor) || t.constructor["__pixiCapturePatched_".concat(n)])
        return; var r = Io(t, e); if ((r == null ? void 0 : r.configurable) === !1 || r && !r.set && !r.writable)
        return; var i = typeof Symbol == "function" ? Symbol("pixiCapture:".concat(n)) : "__pixiCaptureValue_".concat(n); Object.defineProperty(t, e, { configurable: (o = r == null ? void 0 : r.configurable) != null ? o : !0, enumerable: (s = r == null ? void 0 : r.enumerable) != null ? s : !0, get: r != null && r.get ? function () { var a; return (a = r.get) == null ? void 0 : a.call(this); } : function () { var a = this; return Object.prototype.hasOwnProperty.call(a, i) ? a[i] : r && "value" in r ? r.value : void 0; }, set: function (a) { if (sn(this, n, [a]), !window.__TRUEOS_CAPTURE_ONLY__) {
            r != null && r.set ? r.set.call(this, a) : Object.defineProperty(this, i, { configurable: !0, enumerable: !1, writable: !0, value: a });
            return;
        } var d = this; n === "text.text.set" ? d._text = String(a != null ? a : "") : n === "text.style.set" ? d._style = a != null ? a : {} : n === "text.resolution.set" ? d._resolution = Math.max(1, Number(a) || 1) : Object.defineProperty(d, i, { configurable: !0, enumerable: !1, writable: !0, value: a }); } }), t.constructor["__pixiCapturePatched_".concat(n)] = !0; }
    function ai(t, e) {
        if (e === void 0) { e = 0; }
        var s, l, a, d, f, h, b, c, E;
        if (!t || e > 64)
            return null;
        var n, r;
        try {
            var g = typeof t.getGlobalPosition == "function" ? t.getGlobalPosition() : null;
            g && Number.isFinite(Number(g.x)) && Number.isFinite(Number(g.y)) && (n = Number(g.x), r = Number(g.y));
        }
        catch (g) { }
        var i = { id: me(t), type: Fe(t), label: (s = t.label) != null ? s : void 0, x: (d = (a = (l = t.position) == null ? void 0 : l.x) != null ? a : t.x) != null ? d : 0, y: (b = (h = (f = t.position) == null ? void 0 : f.y) != null ? h : t.y) != null ? b : 0, globalX: n, globalY: r, scaleX: Number.isFinite(Number((c = t.scale) == null ? void 0 : c.x)) ? Number(t.scale.x) : 1, scaleY: Number.isFinite(Number((E = t.scale) == null ? void 0 : E.y)) ? Number(t.scale.y) : 1, visible: t.visible, alpha: Number.isFinite(Number(t.alpha)) ? Number(t.alpha) : 1, maskId: t.mask ? me(t.mask) : 0, zIndex: Number(t.zIndex) || 0, sortableChildren: t.sortableChildren === !0 }, o = Mo(t.hitArea);
        return o && (i.hitArea = o), typeof t.text == "string" && (i.text = t.text.slice(0, 120)), Array.isArray(t.children) && t.children.length && (i.children = t.children.map(function (g) { return ai(g, e + 1); })), i;
    }
    function li() {
        var e_30, _a, e_31, _b;
        if (window.__pixiCapture)
            return window.__pixiCapture;
        var t = { enabled: !0, persist: !window.__TRUEOS_CAPTURE_ONLY__, commands: [], counts: Object.create(null), clear: function () { this.commands.length = 0, this.counts = Object.create(null); }, dump: function (e) {
                if (e === void 0) { e = 200; }
                return this.commands.slice(-e);
            }, flush: function () { Be(); }, summary: function () { return ee({}, this.counts); } };
        if (window.__pixiCapture = t, Po(), window.addEventListener("beforeunload", function () { return Be(); }), !ri) {
            ri = !0, typeof wt.prototype.image != "function" && (wt.prototype.image = function () { return this; });
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
            Se(Zt.prototype, "text", "text.text.set"), Se(Zt.prototype, "style", "text.style.set"), Se(Zt.prototype, "resolution", "text.resolution.set"), kn(Zt.prototype, "setSize", "text.setSize"), Se(_t.prototype, "visible", "visible"), Se(_t.prototype, "alpha", "alpha"), Se(_t.prototype, "mask", "mask");
        }
        return t;
    }
    function ci(t) { var e = t.renderer, n = e == null ? void 0 : e.render; if (typeof n != "function" || n.__pixiCapturePatched)
        return; var r = function (o) { var s = o && typeof o == "object" && "container" in o ? o.container : o || t.stage; return oi++, sn(s, "render", []), sn(s, "snapshot", [ai(s)]), window.__TRUEOS_CAPTURE_ONLY__ ? s : n.call(this, o); }; r.__pixiCapturePatched = !0, e.render = r; }
    li();
    var Q = null, Pn = 6, Pe = 10, Bt = 1, Ft = 3, Wt = 4, In = 512, pi = new Map;
    var u = { focusedKeyByPointer: new Map, keyboardOwnerPointerId: 1, inputs: new Map, sliders: new Map, sliderDrags: new Map, sliderBounds: new Map, dialogs: new Map, dialogDrags: new Map, dialogSelectedBy: new Map, dialogZ: new Map, dialogZCounter: 1, numbers: new Map, numberHolds: new Map, selects: new Map, temporals: new Map, temporalYearOwners: new Map, color: { rgb: { r: 255, g: 0, b: 0 }, a: 255, pick: null, draggingPointerId: null, bounds: null }, cursorColors: new Map, primaryMousePointerId: 1, harness: { enabled: !0, activeUserPointerId: Bt, periodMs: 3e3 }, userCursorPos: new Map, lastMouse: { x: 0, y: 0, has: !1 }, scroll: { y: 0, contentHeight: 0, viewportHeight: 0, draggingPointerId: null, dragOffsetY: 0, track: { x: 0, y: 0, w: Pe, h: 0 }, thumb: { x: 0, y: 0, w: Pe, h: 0 } }, iframeScroll: new Map, iframeRects: [], hoverRects: [], hoverHandlers: new Map, hoveredKeyByPointer: new Map, hoveredCursorByPointer: new Map, virtualCursor: { enabled: !1, x: 0, y: 0, t: 0, radius: 120, speed: .9 }, textDrags: new Map, fieldBounds: new Map, dialogDragBounds: new Map, detailsOpen: new Map, contextMenus: new Map, clipboards: new Map }, an = null, Cn = 0;
    function Oo(t) { if (!an) {
        var n = document.createElement("canvas").getContext("2d");
        if (!n)
            throw new Error("2D canvas not available");
        an = n;
    } return an.font = "".concat(t.fontSize, "px ").concat(t.fontFamily), function (e) { return (Cn += 1, an.measureText(e).width); }; }
    function Sn(t, e) {
        if (e === void 0) { e = 16; }
        return Object.entries(t).sort(function (n, r) { return r[1] - n[1] || (n[0] < r[0] ? -1 : n[0] > r[0] ? 1 : 0); }).slice(0, e).map(function (_a) {
            var _b = __read(_a, 2), n = _b[0], r = _b[1];
            return "".concat(n, ":").concat(r);
        }).join(",");
    }
    function We(t) { var e = String(t != null ? t : ""); return e.indexOf("<truesurfer-") >= 0 && (e = e.replace(/<truesurfer-[A-Za-z0-9._-]+>/g, "")), e; }
    function Ro(t, e) { if (e >= t.length)
        return !0; var n = t.charCodeAt(e); return n === 95 || n === 40 || n === 91 || n === 123 || n === 34 || n === 39 || n >= 48 && n <= 57 || n >= 65 && n <= 90; }
    function gi(t) { var e = t, n = !0; for (; n;) {
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
        r >= 2 && Ro(e, r) && (e = e.slice(r), n = !0);
    } return e; }
    function Do(t) { var e = We(t), n = e.indexOf("__trueos") >= 0 || e.indexOf("tsNu") >= 0 || e.indexOf("tsNum") >= 0; return e.indexOf("__TRUEOS_HOST_READY__") >= 0 && (e = e.replace(/__TRUEOS_HOST_READY__/g, "")), e.indexOf("__trueos") >= 0 && (e = Ao(e), e = e.replace(/__trueosNumberValue/g, "").replace(/__trueosHostNum/g, "").replace(/__trueosNum/g, "").replace(/__trueosNu/g, "").replace(/__trueos/g, "")), (e.indexOf("tsNu") >= 0 || e.indexOf("tsNum") >= 0) && (e = e.replace(/tsNum/g, "").replace(/tsNutsNutsNutsNu/g, "").replace(/tsNutsNutsNu/g, "").replace(/tsNutsNu/g, "").replace(/tsNu/g, "")), n && (e = gi(e.trimStart())), e; }
    function Ao(t) { var e = "__trueosN", n = t, r = 0; for (; r < n.length;) {
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
    function bi(t) { return Do(t); }
    function yi(t) { return gi(bi(t).trimStart()); }
    function vo(t) { var e = se(yi(t)); return !(e.length === 0 || e === "true" || e === "false" || e === "N" || e === "Nu" || e === "Num" || e.startsWith("<truesurfer-") || e.startsWith("__trueo")); }
    function xi(t, e) { var r; var n = We(e) || "block"; t[n] = ((r = t[n]) != null ? r : 0) + 1; }
    function No(t) {
        var e_32, _a;
        var e = { nodes: 0, blocks: 0, text: 0, maxDepth: 0, tags: {} }, n = function (r, i) {
            var e_33, _a;
            if (e.nodes += 1, e.maxDepth = Math.max(e.maxDepth, i), r.kind === "text") {
                e.text += 1;
                return;
            }
            e.blocks += 1, xi(e.tags, r.tagName);
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
    function Go(t) { var e = { nodes: 0, blocks: 0, text: 0, maxDepth: 0, tags: {} }, n = function (r, i) {
        var e_34, _a;
        var o;
        e.nodes += 1, e.maxDepth = Math.max(e.maxDepth, i), r.kind === "text" ? e.text += 1 : (e.blocks += 1, xi(e.tags, (o = r.tagName) != null ? o : "block"));
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
    function On(t, e) {
        if (e === void 0) { e = 64; }
        var n = se(bi(t)), r = "";
        for (var i = 0; i < n.length && r.length < e; i += 1) {
            var o = n.charAt(i);
            r += o === "|" || o === '"' || o === "\\" ? "_" : o;
        }
        return r;
    }
    function Lo(t, e) {
        if (e === void 0) { e = 120; }
        var n = "";
        for (var r = 0; r < t.length && n.length < e; r += 1) {
            var i = t.charAt(r);
            n += i === "\r" || i === "\n" || i === "	" || i === "|" || i === '"' || i === "\\" ? "_" : i;
        }
        return n;
    }
    function Bo(t, e) {
        var e_35, _a;
        if (e === void 0) { e = 12; }
        var n = [], r = function (i, o, s) {
            var e_36, _a;
            if (n.length >= e)
                return;
            if (i.kind === "text") {
                n.push("#".concat(n.length, "@").concat(o, ":").concat(s, " chars=").concat(i.text.length, " sample=\"").concat(On(i.text), "\""));
                return;
            }
            var l = We(i.tagName || "block") || "block", a = i.key || "";
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
    function Fo(t, e) {
        if (e === void 0) { e = 12; }
        var n = [], r = function (i, o, s) {
            var e_37, _a;
            var d;
            if (n.length >= e)
                return;
            if (i.kind === "text") {
                var f = (d = i.text) != null ? d : "";
                n.push("#".concat(n.length, "@").concat(o, ":").concat(s, " chars=").concat(f.length, " box=").concat(Math.round(i.x), ",").concat(Math.round(i.y), ",").concat(Math.round(i.width), ",").concat(Math.round(i.height), " sample=\"").concat(On(f), "\""));
                return;
            }
            var l = We(i.tagName || "block") || "block", a = i.key || "";
            try {
                for (var _b = __values(i.children), _c = _b.next(); !_c.done; _c = _b.next()) {
                    var f = _c.value;
                    r(f, l, a);
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
    function Ho(t) { return String(t != null ? t : "").replace(/&quot;/g, '"').replace(/&#34;/g, '"').replace(/&#39;/g, "'").replace(/&apos;/g, "'").replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&amp;/g, "&"); }
    function mi(t) { return se(Ho(String(t != null ? t : "").replace(/<[^>]*>/g, " "))); }
    function un(t) {
        var e_38, _a;
        var e = [];
        try {
            for (var t_3 = __values(t), t_3_1 = t_3.next(); !t_3_1.done; t_3_1 = t_3.next()) {
                var n = t_3_1.value;
                var r = se(String(n != null ? n : ""));
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
    function Wo(t) { var o, s, l, a; var e = [], n = String(t != null ? t : "").replace(/<script[^]*?<\/script>/gi, " ").replace(/<style[^]*?<\/style>/gi, " "), r = /<(h[1-6]|p|label|button)\b[^>]*>([^]*?)<\/\1>|<input\b[^>]*>/gi, i; for (; (i = r.exec(n)) && e.length < In;) {
        var d = (o = i[0]) != null ? o : "", f = String((s = i[1]) != null ? s : "").toLowerCase();
        if (d.toLowerCase().startsWith("<input"))
            continue;
        var h = mi(f === "p" || f === "label" ? (l = i[2]) != null ? l : "" : (a = i[2]) != null ? a : "");
        h.length > 0 && e.push(h);
    } return e; }
    function $o(t) { var e = Wo(t), n = un(e); return un(n); }
    function Uo(t, e, n, r) {
        var e_39, _a;
        var a, d, f, h, b, c;
        var i = un((d = pi.get(String((a = t.key) != null ? a : ""))) != null ? d : []), o = un(String((h = (f = t.attrs) == null ? void 0 : f["data-trueos-srcdoc-text"]) != null ? h : "").split("\n").map(function (E) { return se(E); })), s = i.length > 0 ? i : o.length > 0 ? o : $o(String((c = (b = t.attrs) == null ? void 0 : b.srcdoc) != null ? c : "")), l = n + 48;
        try {
            for (var s_2 = __values(s), s_2_1 = s_2.next(); !s_2_1.done; s_2_1 = s_2.next()) {
                var E = s_2_1.value;
                if (r.length >= In)
                    return;
                r.push({ x: e + 16, y: l, text: E }), l += 32;
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
    function Rn(t) { var e; return t.kind === "text" ? (e = t.text) != null ? e : "" : t.children.map(Rn).join(" "); }
    function Xo(t) { var e = [], n = function (r, i, o, s) {
        var e_40, _a;
        var g, x, k;
        if (e.length >= In)
            return;
        var l = i + r.x, a = o + r.y, d = r.kind === "block" && r.tagName === "iframe" && String((x = (g = r.attrs) == null ? void 0 : g["data-root"]) != null ? x : "") !== "1", f = s + (d ? 1 : 0), h = r.kind === "block" && r.tagName === "button", b = r.kind === "text" ? (k = r.text) != null ? k : "" : h ? Rn(r) : "", c = se(yi(b)), E = e.length;
        if (vo(c)) {
            var _ = h ? l + 8 : l, T = h ? a + Math.max(0, Math.floor((r.height - ge.fontSize * 1.25) / 2)) : a;
            e.push({ x: _, y: T, text: c });
        }
        if (!h) {
            try {
                for (var _b = __values(r.children), _c = _b.next(); !_c.done; _c = _b.next()) {
                    var _ = _c.value;
                    n(_, l, a, f);
                }
            }
            catch (e_40_1) { e_40 = { error: e_40_1 }; }
            finally {
                try {
                    if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
                }
                finally { if (e_40) throw e_40.error; }
            }
            d && e.length === E && Uo(r, l, a, e);
        }
    }; return n(t, 0, 0, 0), e; }
    function Yo(t, e) {
        if (e === void 0) { e = 8; }
        var n = [];
        for (var r = 0; r < t.length && n.length < e; r += 1) {
            var i = t[r];
            n.push("#".concat(n.length, " x=").concat(Math.round(i.x), " y=").concat(Math.round(i.y), " text=\"").concat(On(i.text), "\""));
        }
        return n.join("|");
    }
    function Ko() {
        var e_41, _a;
        var i, o, s, l;
        var t = (o = (i = window.__pixiCapture) == null ? void 0 : i.commands) != null ? o : [], e = {}, n = {}, r = new Set(["addChild", "addChildAt", "setChildIndex", "removeChild", "removeChildren", "removeAllListeners", "on", "clear", "rect", "roundRect", "circle", "ellipse", "moveTo", "lineTo", "closePath", "poly", "fill", "stroke", "image", "visible", "alpha", "scale", "mask", "text.text.set", "text.style.set", "text.resolution.set", "text.setSize", "render", "snapshot"]);
        try {
            for (var t_4 = __values(t), t_4_1 = t_4.next(); !t_4_1.done; t_4_1 = t_4.next()) {
                var a = t_4_1.value;
                var d = We(a == null ? void 0 : a.op);
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
        return { total: t.length, ops: Sn(e, 24), unsupported: Sn(n, 24) };
    }
    function zo(t, e, n, r) { if (!jt())
        return; var i = Ko(); window.__TRUEOS_PIXI_BRIDGE_STATS__ = { renderNodes: t.nodes, renderBlocks: t.blocks, renderText: t.text, renderTags: Sn(t.tags, 24), renderTextSamples: n, layoutBoxes: e.nodes, layoutBlocks: e.blocks, layoutText: e.text, layoutMaxDepth: e.maxDepth, layoutTextSamples: r, measureTextCalls: Cn, scrollbarVisible: u.scroll.track.h > 0 ? 1 : 0, scrollbarTrack: "".concat(Math.round(u.scroll.track.x), ",").concat(Math.round(u.scroll.track.y), ",").concat(Math.round(u.scroll.track.w), ",").concat(Math.round(u.scroll.track.h)), scrollbarThumb: "".concat(Math.round(u.scroll.thumb.x), ",").concat(Math.round(u.scroll.thumb.y), ",").concat(Math.round(u.scroll.thumb.w), ",").concat(Math.round(u.scroll.thumb.h)), pixiCommands: i.total, pixiOps: i.ops, pixiUnsupported: i.unsupported }; }
    var hi = new WeakMap;
    function Dn(t, e) { var n = t; for (; n;) {
        if (n === e)
            return !0;
        n = n.parent;
    } return !1; }
    function wi(t) { return Array.isArray(t.children) || (t.children = []), t.children; }
    function ie(t, e, n) { var r = Number(e) || 0, i = Number(n) || 0; (!t.position || typeof t.position != "object") && (t.position = { x: 0, y: 0 }), t.position.x = r, t.position.y = i; }
    function He(t, e, n) { if (e === t || Dn(t, e))
        return; var r = wi(t); if (e.parent !== t) {
        var l = Math.max(0, Math.min(n, r.length));
        t.addChildAt(e, l);
        return;
    } var i = Math.max(0, r.length - 1), o = Math.max(0, Math.min(n, i)); t.getChildIndex(e) !== o && t.setChildIndex(e, o); }
    function fi(t, e, n) { if (e === t || Dn(t, e))
        return; var r = wi(t); if (e.parent !== t) {
        var l = Math.max(0, Math.min(n, r.length));
        t.addChildAt(e, l);
        return;
    } var i = Math.max(0, r.length - 1), o = Math.max(0, Math.min(n, i)); t.getChildIndex(e) !== o && t.setChildIndex(e, o); }
    var cn = null, st = null;
    function zt(t) { var e = u.cursorColors.get(t); if (e != null)
        return e; var n = [1118481, 2450411, 1483594, 14427686, 8141549, 959977, 16096779], r = Math.abs(Number(t) || 0) % n.length, i = n[r]; return u.cursorColors.set(t, i), i; }
    function Gt(t) { var i, o, s, l, a, d; var e = Number((s = (o = t == null ? void 0 : t.pointerId) != null ? o : (i = t == null ? void 0 : t.data) == null ? void 0 : i.pointerId) != null ? s : 0), r = String((d = (a = t == null ? void 0 : t.pointerType) != null ? a : (l = t == null ? void 0 : t.data) == null ? void 0 : l.pointerType) != null ? d : "").toLowerCase() === "mouse" || e === 1 || e === u.primaryMousePointerId; return u.harness.enabled && r ? u.harness.activeUserPointerId : e; }
    function jt() { return !!globalThis.__TRUEOS_CAPTURE_ONLY__; }
    function kt(t) { jt() && (window.__TRUEOS_PIXI_APP_PHASE__ = t); }
    function L(t) { jt() && (window.__TRUEOS_PIXI_LAYOUT_STEP__ = t); }
    function _i(t) { var l, a, d, f, h; var e = (l = window.__TRUEOS_PIXI_APP_PHASE__) != null ? l : "unknown", n = (a = window.__TRUEOS_PIXI_LAYOUT_STEP__) != null ? a : "", r = t, i = String((d = r == null ? void 0 : r.name) != null ? d : "Error"), o = String((f = r == null ? void 0 : r.message) != null ? f : t), s = String((h = r == null ? void 0 : r.stack) != null ? h : ""); return "phase=".concat(e, " layout=").concat(n, " name=").concat(i, " message=").concat(o, " stack=").concat(s); }
    function jo() { var t = Math.max(1, Number(window.innerWidth || 1920) | 0), e = Math.max(1, Number(window.innerHeight || 1080) | 0), n = new pt(0, 0, t, e), r = document.createElement("canvas"), i = { width: t, height: e, screen: n, render: function (o) { return o; }, resize: function (o, s) { var l = Math.max(1, Number(o || t) | 0), a = Math.max(1, Number(s || e) | 0); this.width = l, this.height = a, n.width = l, n.height = a; } }; return { stage: new _t, screen: n, canvas: r, renderer: i, ticker: { stop: function () { }, add: function () { }, remove: function () { } } }; }
    function Vo() { var l = /** @class */ (function () {
        function l() {
            Z(this, "children");
            Z(this, "measureFunc");
            Z(this, "paddingLeft");
            Z(this, "paddingTop");
            Z(this, "paddingRight");
            Z(this, "paddingBottom");
            Z(this, "marginLeft");
            Z(this, "marginTop");
            Z(this, "marginRight");
            Z(this, "marginBottom");
            Z(this, "width");
            Z(this, "height");
            Z(this, "minWidth");
            Z(this, "minHeight");
            Z(this, "flexDirection");
            Z(this, "computed");
            this.children = [], this.measureFunc = null, this.paddingLeft = 0, this.paddingTop = 0, this.paddingRight = 0, this.paddingBottom = 0, this.marginLeft = 0, this.marginTop = 0, this.marginRight = 0, this.marginBottom = 0, this.width = 0, this.height = 0, this.minWidth = 0, this.minHeight = 0, this.flexDirection = 0, this.computed = { left: 0, top: 0, width: 0, height: 0 };
        }
        l.create = function () { return new l; };
        l.prototype.setMeasureFunc = function (d) { this.measureFunc = d; };
        l.prototype.setMargin = function (d, f) { var h = Number(f) || 0; d === 0 ? this.marginLeft = h : d === 1 ? this.marginTop = h : d === 2 ? this.marginRight = h : d === 3 && (this.marginBottom = h); };
        l.prototype.setPadding = function (d, f) { var h = Number(f) || 0; d === 0 ? this.paddingLeft = h : d === 1 ? this.paddingTop = h : d === 2 ? this.paddingRight = h : d === 3 && (this.paddingBottom = h); };
        l.prototype.setFlexDirection = function (d) { this.flexDirection = d; };
        l.prototype.setAlignItems = function (d) { };
        l.prototype.setJustifyContent = function (d) { };
        l.prototype.setFlexWrap = function (d) { };
        l.prototype.setFlexGrow = function (d) { };
        l.prototype.setFlexShrink = function (d) { };
        l.prototype.setAlignSelf = function (d) { };
        l.prototype.setPositionType = function (d) { };
        l.prototype.setPosition = function (d, f) { };
        l.prototype.setWidth = function (d) { this.width = Math.max(0, Number(d) || 0); };
        l.prototype.setHeight = function (d) { this.height = Math.max(0, Number(d) || 0); };
        l.prototype.setMinWidth = function (d) { this.minWidth = Math.max(0, Number(d) || 0); };
        l.prototype.setMinHeight = function (d) { this.minHeight = Math.max(0, Number(d) || 0); };
        l.prototype.insertChild = function (d, f) { this.children.splice(Math.max(0, Math.min(f, this.children.length)), 0, d); };
        l.prototype.getChildCount = function () { return this.children.length; };
        l.prototype.getComputedLeft = function () { return this.computed.left; };
        l.prototype.getComputedTop = function () { return this.computed.top; };
        l.prototype.getComputedWidth = function () { return this.computed.width; };
        l.prototype.getComputedHeight = function () { return this.computed.height; };
        l.prototype.freeRecursive = function () { };
        l.prototype.calculateLayout = function (d, f) {
            if (d === void 0) { d = this.width; }
            if (f === void 0) { f = this.height; }
            this.layout(0, 0, Math.max(1, Number(d) || this.width || 1), Math.max(1, Number(f) || this.height || 1));
        };
        l.prototype.layout = function (d, f, h, b) {
            var e_42, _a, e_43, _b;
            var c = this.paddingLeft + this.paddingRight, E = this.paddingTop + this.paddingBottom, g = Math.max(this.minWidth, this.width || h), x = Math.max(this.minHeight, this.height || 0);
            if (this.computed.left = d, this.computed.top = f, this.computed.width = g, this.measureFunc) {
                var k = this.measureFunc(Math.max(0, g - c), 0);
                x = Math.max(x, Math.ceil(Number(k.height) || 0) + E), this.computed.height = x;
                return;
            }
            if (this.flexDirection === 1) {
                var k = this.paddingLeft, _ = 0;
                try {
                    for (var _c = __values(this.children), _d = _c.next(); !_d.done; _d = _c.next()) {
                        var T = _d.value;
                        var U = T.width || T.minWidth || Math.max(24, (g - c) / Math.max(1, this.children.length));
                        T.layout(k + T.marginLeft, this.paddingTop + T.marginTop, U, b), k += T.computed.width + T.marginLeft + T.marginRight, _ = Math.max(_, T.computed.height + T.marginTop + T.marginBottom);
                    }
                }
                catch (e_42_1) { e_42 = { error: e_42_1 }; }
                finally {
                    try {
                        if (_d && !_d.done && (_a = _c.return)) _a.call(_c);
                    }
                    finally { if (e_42) throw e_42.error; }
                }
                x = Math.max(x, _ + E);
            }
            else {
                var k = this.paddingTop;
                try {
                    for (var _f = __values(this.children), _g = _f.next(); !_g.done; _g = _f.next()) {
                        var _ = _g.value;
                        var T = Math.max(0, g - c - _.marginLeft - _.marginRight);
                        _.layout(this.paddingLeft + _.marginLeft, k + _.marginTop, T, b), k += _.computed.height + _.marginTop + _.marginBottom;
                    }
                }
                catch (e_43_1) { e_43 = { error: e_43_1 }; }
                finally {
                    try {
                        if (_g && !_g.done && (_b = _f.return)) _b.call(_f);
                    }
                    finally { if (e_43) throw e_43.error; }
                }
                x = Math.max(x, k + this.paddingBottom);
            }
            this.computed.height = Math.max(this.minHeight, x);
        };
        return l;
    }()); return { Node: l, EDGE_LEFT: 0, EDGE_TOP: 1, EDGE_RIGHT: 2, EDGE_BOTTOM: 3, FLEX_DIRECTION_COLUMN: 0, FLEX_DIRECTION_ROW: 1, FLEX_DIRECTION_ROW_REVERSE: 1, ALIGN_STRETCH: 0, ALIGN_CENTER: 1, ALIGN_FLEX_START: 2, JUSTIFY_CENTER: 0, JUSTIFY_FLEX_START: 1, JUSTIFY_SPACE_BETWEEN: 2, WRAP_WRAP: 1, WRAP_NO_WRAP: 0, POSITION_TYPE_ABSOLUTE: 1, DIRECTION_LTR: 0, MEASURE_MODE_UNDEFINED: 0 }; }
    function Jo(t) {
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
                        var f = _c.value;
                        n(f, l, a);
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
    function ln(t, e) { var o, s, l, a; var n = u.inputs.get(t); if (n)
        return n; var r = {}, i = ((o = e == null ? void 0 : e.type) != null ? o : "text").toLowerCase(); if (i === "checkbox" || i === "radio") {
        if (r.checked = e ? Object.prototype.hasOwnProperty.call(e, "checked") : !1, i === "checkbox") {
            var d = ((s = e == null ? void 0 : e["aria-checked"]) != null ? s : "").toLowerCase(), f = ((l = e == null ? void 0 : e["data-indeterminate"]) != null ? l : "").toLowerCase();
            r.indeterminate = (e ? Object.prototype.hasOwnProperty.call(e, "indeterminate") : !1) || d === "mixed" || f === "true" || f === "1" || f === "yes";
        }
    }
    else
        r.value = (a = e == null ? void 0 : e.value) != null ? a : ""; return u.inputs.set(t, r), r; }
    function Zo(t) { var e = new Map; function n(r) {
        var e_46, _a;
        var i, o, s, l, a;
        if (r.kind === "block" && r.tagName === "input" && ((o = (i = r.attrs) == null ? void 0 : i.type) != null ? o : "text").toLowerCase() === "radio") {
            var h = "radio:".concat((l = (s = r.attrs) == null ? void 0 : s.name) != null ? l : "__default__"), b = r.key;
            if (b) {
                var c = (a = e.get(h)) != null ? a : [];
                c.push(b), e.set(h, c);
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
    function se(t) { var e = "", n = !1, r = String(t != null ? t : ""); for (var i = 0; i < r.length; i += 1) {
        var o = r.charCodeAt(i);
        if (o === 32 || o === 9 || o === 10 || o === 13 || o === 12) {
            n = !0;
            continue;
        }
        n && e.length > 0 && (e += " "), e += r.charAt(i), n = !1;
    } return e; }
    function Qo(t) {
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
    function Ti(t, e) { var a, d, f, h; if (!t || typeof t != "object")
        return null; var n = t, r = String((a = n.kind) != null ? a : ""); if (r === "text") {
        var b = se(String((d = n.text) != null ? d : ""));
        return b.length > 0 ? { kind: "text", text: b } : null;
    } if (r !== "block")
        return null; var i = String((f = n.tagName) != null ? f : "").toLowerCase(); if (i.length === 0)
        return null; var o = String((h = n.key) != null ? h : "".concat(e, ":").concat(i)), s = [], l = Array.isArray(n.children) ? n.children : []; for (var b = 0; b < l.length; b += 1) {
        var c = Ti(l[b], "".concat(e, ".").concat(b));
        c && s.push(c);
    } return { kind: "block", key: o, tagName: i, attrs: Qo(n.attrs), children: s }; }
    function qo(t) { var e = Array.isArray(t) ? t : t && typeof t == "object" && Array.isArray(t.widgetRenderTree) ? t.widgetRenderTree : [], n = []; for (var r = 0; r < e.length; r += 1) {
        var i = Ti(e[r], "0.".concat(r));
        i && n.push(i);
    } return n; }
    function ts(t) { var n = document.createElement("canvas").getContext("2d"); if (!n)
        throw new Error("2D canvas not available"); n.font = t; var r = t.indexOf("px"), i = r; for (; i > 0;) {
        var l = t.charCodeAt(i - 1);
        if (l < 48 || l > 57)
            break;
        i -= 1;
    } var o = r > i ? Number(t.slice(i, r)) : 16, s = Math.ceil(o * 1.25); return { measure: function (l, a) {
            var e_48, _a;
            Cn += 1;
            var d = se(l).split(" ").filter(Boolean);
            if (d.length === 0)
                return { width: 0, height: s, lines: [""] };
            var f = [], h = "";
            try {
                for (var d_1 = __values(d), d_1_1 = d_1.next(); !d_1_1.done; d_1_1 = d_1.next()) {
                    var E = d_1_1.value;
                    var g = h ? "".concat(h, " ").concat(E) : E, x = n.measureText(g).width, k = a != null ? a : Number.POSITIVE_INFINITY;
                    x <= k || !h ? h = g : (f.push(h), h = E);
                }
            }
            catch (e_48_1) { e_48 = { error: e_48_1 }; }
            finally {
                try {
                    if (d_1_1 && !d_1_1.done && (_a = d_1.return)) _a.call(d_1);
                }
                finally { if (e_48) throw e_48.error; }
            }
            h && f.push(h);
            var b = Math.min(Math.max.apply(Math, __spreadArray([], __read(f.map(function (E) { return n.measureText(E).width; })), false)), a != null ? a : Number.POSITIVE_INFINITY), c = f.length * s;
            return { width: Math.ceil(b), height: Math.ceil(c), lines: f };
        }, lineHeight: s, font: t }; }
    function es(t, e, n) { var b; L("build:start nodes=".concat(t.length, " viewport=").concat(e, "x").concat(n)); var r = 12, i = 8, o = ge; L("build:measurer"); var s = ts("".concat(o.fontSize, "px ").concat(o.fontFamily)); function l(c) { return c.kind !== "block" || c.tagName === "hr" || c.tagName === "tr" || c.tagName === "td" || c.tagName === "th" ? 0 : i; } function a(c) { var E = c.kind === "text" ? "text:".concat(c.text.slice(0, 24)) : "".concat(c.tagName, ":").concat(c.key); if (L("node:".concat(E, ":start")), c.kind === "text") {
        var _1 = Q.Node.create();
        return L("node:".concat(E, ":measure-func")), _1.setMeasureFunc(function (T, U) { L("node:".concat(E, ":measure-call")); var D = U === Q.MEASURE_MODE_UNDEFINED ? void 0 : Math.max(0, T), $ = s.measure(c.text, D); return { width: $.width, height: $.height }; }), _1.setMargin(Q.EDGE_RIGHT, 6), _1.setMargin(Q.EDGE_BOTTOM, 0), { yogaNode: _1, buildBox: function () { return ({ kind: "text", text: c.text, x: _1.getComputedLeft(), y: _1.getComputedTop(), width: _1.getComputedWidth(), height: _1.getComputedHeight(), children: [] }); } };
    } if (c.tagName === "sliderlabel")
        return L("node:".concat(c.tagName, ":").concat(c.key, ":sliderlabel")), Zn({ node: c, Yoga: Q, measurer: s }); L("node:".concat(c.tagName, ":").concat(c.key, ":create")); var g = Q.Node.create(); if (L("node:".concat(c.tagName, ":").concat(c.key, ":base-defaults")), g.setFlexDirection(Q.FLEX_DIRECTION_COLUMN), g.setAlignItems(Q.ALIGN_STRETCH), g.setPadding(Q.EDGE_LEFT, r), g.setPadding(Q.EDGE_RIGHT, r), g.setPadding(Q.EDGE_TOP, r), g.setPadding(Q.EDGE_BOTTOM, r), g.setMargin(Q.EDGE_BOTTOM, 0), xn(c.tagName) && (L("node:".concat(c.tagName, ":").concat(c.key, ":heading-defaults")), mr(g, Q)), c.tagName === "hr" && (L("node:".concat(c.tagName, ":").concat(c.key, ":hr-defaults")), ir(g, Q)), (c.tagName === "p" || c.tagName === "label") && (L("node:".concat(c.tagName, ":").concat(c.key, ":inline-scan")), c.children.some(function (T) { return T.kind === "block" && (T.tagName === "input" || T.tagName === "button" || T.tagName === "select" || T.tagName === "textarea" || T.tagName === "timeinput" || T.tagName === "dateinput" || T.tagName === "monthinput" || T.tagName === "weekinput" || T.tagName === "datetimelocalinput" || T.tagName === "progress" || T.tagName === "meter" || T.tagName === "slider" || T.tagName === "number" || T.tagName === "color"); }) && (g.setFlexDirection(Q.FLEX_DIRECTION_ROW), g.setFlexWrap(Q.WRAP_WRAP), g.setAlignItems(Q.ALIGN_CENTER)), g.setPadding(Q.EDGE_TOP, 4), g.setPadding(Q.EDGE_BOTTOM, 4), g.setPadding(Q.EDGE_LEFT, 4), g.setPadding(Q.EDGE_RIGHT, 4)), c.tagName === "table" && (L("node:".concat(c.tagName, ":").concat(c.key, ":table-defaults")), cr(g, Q)), c.tagName === "tr" && (L("node:".concat(c.tagName, ":").concat(c.key, ":tr-defaults")), ur(g, Q)), (c.tagName === "td" || c.tagName === "th") && (L("node:".concat(c.tagName, ":").concat(c.key, ":cell-defaults")), dr(g, Q)), c.tagName === "input" && (L("node:".concat(c.tagName, ":").concat(c.key, ":input-defaults")), Ar(g, c, Q)), c.tagName === "textarea" && (L("node:".concat(c.tagName, ":").concat(c.key, ":textarea-defaults")), Nr(g, Q)), c.tagName === "select" && (L("node:".concat(c.tagName, ":").concat(c.key, ":select-defaults")), Vr(g, Q)), c.tagName === "timeinput" || c.tagName === "dateinput" || c.tagName === "monthinput" || c.tagName === "weekinput" || c.tagName === "datetimelocalinput") {
        var _ = c.tagName === "timeinput" ? "time" : c.tagName === "monthinput" ? "month" : c.tagName === "weekinput" ? "week" : c.tagName === "dateinput" ? "date" : "datetime-local";
        L("node:".concat(c.tagName, ":").concat(c.key, ":temporal-defaults")), Qr(g, Q, _);
    } c.tagName === "img" && (L("node:".concat(c.tagName, ":").concat(c.key, ":img-defaults")), _r(g, c, Q)), c.tagName === "svg" && (L("node:".concat(c.tagName, ":").concat(c.key, ":svg-defaults")), Pr(g, c, Q)), c.tagName === "canvas" && (L("node:".concat(c.tagName, ":").concat(c.key, ":canvas-defaults")), Cr(g, c, Q)), c.tagName === "iframe" && (L("node:".concat(c.tagName, ":").concat(c.key, ":iframe-defaults")), Rr(g, c, Q)), c.tagName === "button" && (L("node:".concat(c.tagName, ":").concat(c.key, ":button-defaults")), sr(g, Q)), c.tagName === "dialog" && (L("node:".concat(c.tagName, ":").concat(c.key, ":dialog-defaults")), Wr(g, Q)), c.tagName === "number" && (L("node:".concat(c.tagName, ":").concat(c.key, ":number-defaults")), Ur(g, Q)), c.tagName === "color" && (L("node:".concat(c.tagName, ":").concat(c.key, ":color-defaults")), Kr(g, c, Q)), c.tagName === "searchrow" && (L("node:".concat(c.tagName, ":").concat(c.key, ":searchrow-defaults")), Br(g, Q)), c.tagName === "searchbutton" && (L("node:".concat(c.tagName, ":").concat(c.key, ":searchbutton-defaults")), Fr(g, Q)), c.tagName === "summary" && (L("node:".concat(c.tagName, ":").concat(c.key, ":summary-defaults")), tr(g, Q)), c.tagName === "details" && (L("node:".concat(c.tagName, ":").concat(c.key, ":details-defaults")), er(g, Q)), c.tagName === "barrow" && (L("node:".concat(c.tagName, ":").concat(c.key, ":barrow-defaults")), Lr(g, Q)), (c.tagName === "progress" || c.tagName === "meter") && (L("node:".concat(c.tagName, ":").concat(c.key, ":progress-defaults")), Vn(g, Q)), c.tagName === "slider" && (L("node:".concat(c.tagName, ":").concat(c.key, ":slider-defaults")), Jn(g, Q)), L("node:".concat(c.tagName, ":").concat(c.key, ":children-effective")); var x = nr(c, u.detailsOpen); L("node:".concat(c.tagName, ":").concat(c.key, ":children-map count=").concat(x.length)); var k = x.map(a); L("node:".concat(c.tagName, ":").concat(c.key, ":children-insert")); for (var _ = 0; _ < k.length; _++) {
        var T = x[_], U = k[_];
        if (T && T.kind === "block") {
            var D = _ === k.length - 1 ? 0 : l(T);
            U.yogaNode.setMargin(Q.EDGE_BOTTOM, D);
        }
        g.insertChild(U.yogaNode, g.getChildCount());
    } return { yogaNode: g, buildBox: function () { return ({ kind: "block", key: c.key, tagName: c.tagName, attrs: c.attrs, x: g.getComputedLeft(), y: g.getComputedTop(), width: g.getComputedWidth(), height: g.getComputedHeight(), children: k.map(function (_) { return _.buildBox(); }) }); } }; } var d = Q.Node.create(); L("root:flex-direction"), d.setFlexDirection(Q.FLEX_DIRECTION_COLUMN), L("root:align-items"), d.setAlignItems(Q.ALIGN_STRETCH), L("root:width"), d.setWidth(e), L("root:height"), d.setHeight(n), L("root:padding-left"), d.setPadding(Q.EDGE_LEFT, 16), L("root:padding-top"), d.setPadding(Q.EDGE_TOP, 16), L("root:padding-right"), d.setPadding(Q.EDGE_RIGHT, 16 + Pn), L("root:padding-bottom"), d.setPadding(Q.EDGE_BOTTOM, 16), L("root:children-map count=".concat(t.length)); var f = t.map(a); L("root:children-insert"); for (var c = 0; c < f.length; c++) {
        var E = t[c], g = f[c];
        if (E && E.kind === "block") {
            var x = c === f.length - 1 ? 0 : l(E);
            g.yogaNode.setMargin(Q.EDGE_BOTTOM, x);
        }
        d.insertChild(g.yogaNode, d.getChildCount());
    } L("root:calculate"), d.calculateLayout(e, n, Q.DIRECTION_LTR), L("root:build-box"); var h = { kind: "block", tagName: "root", x: 0, y: 0, width: d.getComputedWidth(), height: d.getComputedHeight(), children: f.map(function (c) { return c.buildBox(); }) }; return L("root:free"), (b = d.freeRecursive) == null || b.call(d), L("build:done"), h; }
    function ns(t, e, n) {
        var e_49, _a, e_50, _b, e_51, _c, e_52, _d, e_53, _f;
        var N, B;
        L("render:start");
        var r = ge, i = n != null ? n : t.stage;
        L("render:get-background");
        var o = Et(i, "__background");
        L("render:get-content-root");
        var s = be(i, "__contentRoot");
        L("render:get-dialog-root");
        var l = be(i, "__dialogRoot");
        L("render:get-overlay-root");
        var a = be(i, "__overlayRoot");
        L("render:ensure-background"), fi(i, o, 0), L("render:ensure-content-root"), He(i, s, 1), L("render:ensure-dialog-root"), He(i, l, 2), L("render:ensure-overlay-root"), He(i, a, 3), L("render:overlay-remove-children"), a.removeChildren(), L("render:overlay-removed");
        var d = [], f = [], h = Zo(e);
        L("render:clear-ui-state"), u.fieldBounds.clear(), u.sliderBounds.clear(), u.dialogDragBounds.clear(), u.hoverRects.length = 0, u.hoverHandlers.clear(), u.iframeRects.length = 0, L("render:node-cache");
        var b = (N = hi.get(i)) != null ? N : new Map;
        hi.set(i, b);
        var c = new Set, E = function (m) {
            var e_54, _a;
            var j;
            var R = 0, G = function (K, q, z) {
                var e_55, _a;
                var C;
                if (K.kind === "block" && K.tagName === "dialog")
                    return;
                var p = q + K.x, w = z + K.y;
                R = Math.max(R, w + K.height);
                try {
                    for (var _b = __values((C = K.children) != null ? C : []), _c = _b.next(); !_c.done; _c = _b.next()) {
                        var S = _c.value;
                        G(S, p, w);
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
                for (var _b = __values((j = m.children) != null ? j : []), _c = _b.next(); !_c.done; _c = _b.next()) {
                    var K = _c.value;
                    G(K, 0, 0);
                }
            }
            catch (e_54_1) { e_54 = { error: e_54_1 }; }
            finally {
                try {
                    if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
                }
                finally { if (e_54) throw e_54.error; }
            }
            return R;
        }, g = new Set;
        try {
            for (var _g = __values(u.textDrags.values()), _h = _g.next(); !_h.done; _h = _g.next()) {
                var m = _h.value;
                g.add(m.key);
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
        var x = Oo(r);
        function k(m, R, G) { return Math.max(R, Math.min(G, m)); }
        var _ = function (m) {
            var e_56, _a;
            try {
                for (var _b = __values(u.textDrags.entries()), _c = _b.next(); !_c.done; _c = _b.next()) {
                    var _d = __read(_c.value, 2), R = _d[0], G = _d[1];
                    if (G.key === m)
                        return R;
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
        }, T = function (m) {
            var e_57, _a;
            var R = u.keyboardOwnerPointerId;
            if (u.focusedKeyByPointer.get(R) === m)
                return R;
            try {
                for (var _b = __values(u.focusedKeyByPointer.entries()), _c = _b.next(); !_c.done; _c = _b.next()) {
                    var _d = __read(_c.value, 2), G = _d[0], j = _d[1];
                    if (j === m)
                        return G;
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
        L("render:background-clear"), Mt(o), L("render:background-rect"), o.rect(0, 0, t.renderer.width, t.renderer.height), L("render:background-fill"), o.fill(r.background), L("render:content-position");
        {
            var m = u.scroll, R = m && Number(m.y || 0) || 0;
            if (R !== 0) {
                var G = s.position;
                G && (G.x = 0, G.y = -R);
            }
        }
        L("render:content-position-done");
        function U(m, R, G, j, K, q, z, p, w) {
            var e_58, _a;
            if (j === void 0) { j = 0; }
            if (K === void 0) { K = 0; }
            var H, A, X, it, et, Y, V, lt, tt, nt, ot, gt, It, at, Tt, Lt, $t, Nt, Ot, Dt;
            L("render:draw:".concat(p, ":").concat(m.kind, ":").concat(m.kind === "block" ? m.tagName : "text", ":start"));
            var C = m.kind === "block" ? m.key && m.key.length > 0 ? m.key : "".concat(p, ":").concat((H = m.tagName) != null ? H : "block") : "", S = m.kind === "block" ? "b:".concat(C) : "t:".concat(p);
            L("render:draw:".concat(p, ":cache"));
            var P = b.get(S);
            (!P || Dn(R, P)) && (L("render:draw:".concat(p, ":new-container")), P = new _t, P.label = S, b.set(S, P)), L("render:draw:".concat(p, ":ensure-child")), c.add(S), He(R, P, w), L("render:draw:".concat(p, ":children-root"));
            var F = be(P, "__children");
            if (L("render:draw:".concat(p, ":ensure-children-root")), He(P, F, 1), L("render:draw:".concat(p, ":position")), ie(P, m.x, m.y), m.kind === "block" && m.tagName === "hr" && ie(P, Math.round(m.x), Math.round(m.y)), m.kind === "block" && m.tagName === "dialog" && m.key) {
                var ht = Je(u.dialogs, m.key), ft = Math.max(0, m.width), ut = Math.max(0, m.height), ct = z.x, Ut = z.y, At = Math.max(ct, z.x + z.w - ft), rt = Math.max(Ut, z.y + z.h - ut);
                if (u.dialogDragBounds.set(m.key, { minX: ct, minY: Ut, maxX: At, maxY: rt }), jt() && !ht.__trueosInitialPositionSeeded) {
                    var mt = z.w <= 760 && z.h <= 800, yt = ct + Math.max(12, Math.floor((z.w - ft) / 2)), Ct = Ut + Math.max(mt ? 190 : 40, Math.floor((z.h - ut) / 2));
                    ht.x = Math.max(ct, Math.min(At, yt)), ht.y = Math.max(Ut, Math.min(rt, Ct)), ht.__trueosInitialPositionSeeded = !0;
                }
                ht.x = Math.max(ct, Math.min(At, ht.x)), ht.y = Math.max(Ut, Math.min(rt, ht.y)), ie(P, ht.x, ht.y);
            }
            var y = j + P.position.x, I = K + P.position.y;
            if (m.kind === "block") {
                L("render:draw:".concat(p, ":block:").concat(m.tagName, ":begin"));
                var ht = G;
                (m.tagName === "h1" || m.tagName === "h2" || m.tagName === "h3" || m.tagName === "summary" || m.tagName === "th") && (ht = { bold: !0 }), L("render:draw:".concat(p, ":graphics"));
                var ft = Et(P, "__g");
                L("render:draw:".concat(p, ":graphics-clear")), Mt(ft), L("render:draw:".concat(p, ":graphics-ensure")), fi(P, ft, 0), ft.zIndex = -10;
                var ut = Math.max(0, m.width), ct = Math.max(0, m.height), Ut = null;
                if ((m.tagName === "h1" || m.tagName === "h2" || m.tagName === "h3") && (ie(P, Math.round(m.x), Math.round(m.y)), ut = Math.round(ut), ct = Math.round(ct)), L("render:draw:".concat(p, ":widget:").concat(m.tagName)), m.tagName === "hr")
                    rr({ graphics: ft, w: ut, theme: r });
                else if (m.tagName !== "barrow") {
                    if (m.tagName !== "searchrow") {
                        if (m.tagName === "searchbutton")
                            Hr({ node: m, container: P, graphics: ft, w: ut, h: ct, theme: r, uiState: u, getPointerId: Gt, focusInputKey: (A = m.attrs) == null ? void 0 : A["data-focus-key"], requestPaint: st });
                        else if (m.tagName === "progress" || m.tagName === "meter")
                            jn({ node: m, graphics: ft, w: ut, h: ct, theme: r });
                        else if (m.tagName === "sliderlabel")
                            Qn({ node: m, container: P, theme: r, sliderStates: u.sliders });
                        else if (m.tagName === "slider")
                            Ve({ node: m, container: P, graphics: ft, w: ut, h: ct, absX: y, absY: I, theme: r, sliderStates: u.sliders, sliderBounds: u.sliderBounds, sliderDrags: u.sliderDrags, requestPaint: st, getPointerId: Gt });
                        else if (m.tagName === "timeinput" || m.tagName === "dateinput" || m.tagName === "monthinput" || m.tagName === "weekinput" || m.tagName === "datetimelocalinput")
                            qr({ node: m, container: P, graphics: ft, w: ut, h: ct, absX: y, absY: I, theme: r, uiState: u, getPointerId: Gt, getCursorColor: zt, temporalStates: u.temporals, yearSliderOwners: u.temporalYearOwners, getOrInitInputValue: function (J, xt) { return ln(J, xt); }, requestPaint: st, popupSink: f });
                        else if (m.tagName === "input") {
                            var J = m.key, xt = J != null ? T(J) : null, qt = J != null && u.focusedKeyByPointer.get(u.keyboardOwnerPointerId) === J, Rt = J == null ? null : qt ? u.keyboardOwnerPointerId : g.has(J) ? _(J) : null, te = Rt != null, Vt = xt != null ? zt(xt) : null;
                            vr({ node: m, container: P, graphics: ft, w: ut, h: ct, absX: y, absY: I, theme: r, textMeasure: x, uiState: u, getOrInitInputState: ln, clamp: k, radioGroups: h, textDrags: u.textDrags, requestPaint: st, showCaret: te, caretPointerId: Rt, focusColor: Vt != null ? Vt : void 0, getCursorColor: zt, getPointerId: Gt });
                        }
                        else if (m.tagName === "textarea") {
                            var J = m.key, xt = J != null ? T(J) : null, qt = J != null && u.focusedKeyByPointer.get(u.keyboardOwnerPointerId) === J, Rt = J == null ? null : qt ? u.keyboardOwnerPointerId : g.has(J) ? _(J) : null, te = Rt != null, Vt = xt != null ? zt(xt) : null;
                            Gr({ node: m, container: P, graphics: ft, w: ut, h: ct, absX: y, absY: I, theme: r, textMeasure: x, uiState: u, getOrInitInputState: ln, clamp: k, textDrags: u.textDrags, requestPaint: st, showCaret: te, caretPointerId: Rt, focusColor: Vt != null ? Vt : void 0, getCursorColor: zt, getPointerId: Gt });
                        }
                        else if (m.tagName === "select") {
                            if (m.key) {
                                var J = Number((it = (X = m.attrs) == null ? void 0 : X["data-selected-index"]) != null ? it : "0");
                                de(u.selects, m.key, Number.isFinite(J) ? J : 0);
                            }
                            tn({ node: m, container: P, graphics: ft, w: ut, h: ct, absX: y, absY: I, theme: r, selectStates: u.selects, uiState: u, getPointerId: Gt, getCursorColor: zt, requestPaint: st, popupSink: d });
                        }
                        else if (m.tagName === "summary")
                            m.key && u.hoverRects.push({ key: m.key, kind: "summary", cursor: "pointer", x: y, y: I, w: ut, h: ct }), qn({ node: m, container: P, w: ut, h: ct, theme: r, detailsOpen: u.detailsOpen, requestRerender: cn });
                        else if (m.tagName === "dialog")
                            $r({ node: m, container: P, w: ut, h: ct, theme: r, selectedBy: u.dialogSelectedBy, getCursorColor: zt, dialogStates: u.dialogs, dialogDrags: u.dialogDrags, bringToFront: function (J) { u.dialogZ.set(J, u.dialogZCounter++); }, requestPaint: st, getPointerId: Gt });
                        else if (m.tagName === "img")
                            wr({ node: m, container: P, graphics: ft, w: ut, h: ct, theme: r, requestRerender: cn });
                        else if (m.tagName === "svg") {
                            var J = (Y = (et = m.attrs) == null ? void 0 : et["data-svg"]) != null ? Y : "";
                            Ir({ svgMarkup: J, container: P, w: ut, h: ct, requestRerender: cn });
                        }
                        else if (m.tagName === "canvas")
                            Or({ node: m, container: P, graphics: ft, w: ut, h: ct, theme: r });
                        else if (m.tagName === "iframe")
                            Dr({ node: m, container: P, graphics: ft, w: ut, h: ct, theme: r });
                        else if (m.tagName === "color")
                            u.color.bounds = { x: y, y: I, w: Math.max(0, ut), h: Math.max(0, ct) }, jr({ node: m, container: P, graphics: ft, w: ut, h: ct, theme: r, rgb: u.color.rgb, setRgb: function (J) { u.color.rgb = J; }, alpha: u.color.a, setAlpha: function (J) { u.color.a = Math.max(0, Math.min(255, Math.round(J))); }, pick: u.color.pick, setPick: function (J) { u.color.pick = J; }, requestPaint: st, getPointerId: Gt, setDraggingPointerId: function (J) { u.color.draggingPointerId = J; } });
                        else if (m.tagName === "number") {
                            var J_1 = m.key, xt_1 = String((lt = (V = m.attrs) == null ? void 0 : V.channel) != null ? lt : "").toLowerCase(), qt_1 = xt_1 === "r" || xt_1 === "g" || xt_1 === "b" || xt_1 === "a";
                            J_1 && Xr({ node: m, container: P, graphics: ft, w: ut, h: ct, theme: r, getValue: function () { var Rt, te; return qt_1 ? xt_1 === "a" ? (Rt = u.color.a) != null ? Rt : 255 : (te = u.color.rgb[xt_1]) != null ? te : 0 : _n(u.numbers, J_1, m.attrs).value; }, setValue: function (Rt) { qt_1 ? xt_1 === "a" ? u.color.a = Math.max(0, Math.min(255, Math.round(Rt))) : u.color.rgb[xt_1] = Math.max(0, Math.min(255, Math.round(Rt))) : _n(u.numbers, J_1, m.attrs).value = Rt; }, requestPaint: st, numberHolds: u.numberHolds, getPointerId: Gt });
                        }
                        else if (m.tagName === "button")
                            m.key && u.hoverRects.push({ key: m.key, kind: "button", cursor: "pointer", x: y, y: I, w: ut, h: ct }), or({ container: P, graphics: ft, w: ut, h: ct, label: se(Rn(m)), theme: r, registerHoverHandlers: m.key ? function (J) { u.hoverHandlers.set(m.key, J); } : void 0 });
                        else if (!xn(m.tagName))
                            if (m.tagName === "table")
                                ar({ graphics: ft, w: ut, h: ct, boxBorder: r.boxBorder });
                            else if (m.tagName === "td" || m.tagName === "th")
                                lr({ nodeTag: m.tagName, graphics: ft, w: ut, h: ct, theme: r });
                            else {
                                var J = Math.max(0, Math.round(ut)), xt = Math.max(0, Math.round(ct));
                                ft.rect(0, 0, J, xt), ft.stroke({ width: 1, color: r.boxBorder, alignment: 0 });
                            }
                    }
                }
                L("render:draw:".concat(p, ":overlay-label")), Ut && P.addChild(Ut);
                var At = null, rt = null, mt = m.tagName === "iframe" && String((nt = (tt = m.attrs) == null ? void 0 : tt["data-root"]) != null ? nt : "") === "1";
                if (m.tagName === "iframe" && !mt) {
                    m.key && u.iframeRects.push({ key: m.key, x: y, y: I, w: Math.max(0, ut), h: Math.max(0, ct) }), At = be(P, "__iframeContentRoot"), ie(At, 0, 0);
                    var Rt = Et(P, "__iframeContentMask");
                    Mt(Rt);
                    var te = 0, Vt = 34, Mi = Math.max(0, ut), ki = Math.max(0, ct - 34);
                    Rt.rect(te, Vt, Mi, ki), Rt.fill(16777215), Rt.alpha = 0, At.mask = Rt;
                    var $e_1 = (ot = m.key) != null ? ot : "", dt_1 = (gt = u.iframeScroll.get($e_1)) != null ? gt : { y: 0, contentHeight: 0, viewportHeight: 0, draggingPointerId: null, dragOffsetY: 0, track: { x: 0, y: 0, w: Pe, h: 0 }, thumb: { x: 0, y: 0, w: Pe, h: 0 }, rect: { x: y, y: I, w: Math.max(0, ut), h: Math.max(0, ct) } };
                    dt_1.rect = { x: y, y: I, w: Math.max(0, ut), h: Math.max(0, ct) }, dt_1.contentHeight = E(m), dt_1.viewportHeight = Math.max(0, ct - 34 - 8);
                    var we_1 = Math.max(0, dt_1.contentHeight - dt_1.viewportHeight);
                    dt_1.y = Math.max(0, Math.min(dt_1.y, we_1)), rt = be(At, "__iframeScrollRoot"), ie(rt, 0, -dt_1.y);
                    var le = Et(P, "__iframeScrollbar");
                    Mt(le), le.eventMode = "static";
                    var dn = Pn, he = Pe, Ue = Math.max(0, ut - he - dn), mn = 34 + dn, Ce = Math.max(0, ct - 34 - dn * 2), An = we_1 > .5 && Ce > 1;
                    if (le.visible = An, An) {
                        var hn = Math.max(24, (dt_1.viewportHeight || 1) / Math.max(1, dt_1.contentHeight) * Ce), Ei = Math.max(1, Ce - hn), Si = we_1 <= 0 ? 0 : dt_1.y / we_1, vn = mn + Ei * Si;
                        dt_1.track = { x: y + Ue, y: I + mn, w: he, h: Ce }, dt_1.thumb = { x: y + Ue, y: I + vn, w: he, h: hn }, le.rect(Ue, mn, he, Ce), le.fill({ color: 0, alpha: .06 }), le.rect(Ue, vn, he, hn), le.fill({ color: 0, alpha: .25 }), le.on("pointerdown", function (Jt) { var Ln, Bn, Fn, Hn, Wn, $n; if ((Jt == null ? void 0 : Jt.button) === 2)
                            return; var fn = Gt(Jt); if (fn <= 0)
                            return; var Xe = (Bn = (Ln = Jt.global) == null ? void 0 : Ln.x) != null ? Bn : 0, fe = (Hn = (Fn = Jt.global) == null ? void 0 : Fn.y) != null ? Hn : 0; if (!(Xe >= dt_1.track.x && Xe <= dt_1.track.x + dt_1.track.w && fe >= dt_1.track.y && fe <= dt_1.track.y + dt_1.track.h))
                            return; if (Xe >= dt_1.thumb.x && Xe <= dt_1.thumb.x + dt_1.thumb.w && fe >= dt_1.thumb.y && fe <= dt_1.thumb.y + dt_1.thumb.h) {
                            dt_1.draggingPointerId = fn, dt_1.dragOffsetY = fe - dt_1.thumb.y, u.iframeScroll.set($e_1, dt_1), (Wn = Jt.stopPropagation) == null || Wn.call(Jt);
                            return;
                        } var Nn = Math.max(1, dt_1.track.h - dt_1.thumb.h), Gn = Math.max(dt_1.track.y, Math.min(dt_1.track.y + Nn, fe - dt_1.thumb.h / 2)), Pi = (Gn - dt_1.track.y) / Nn; dt_1.y = Math.max(0, Math.min(we_1, Pi * we_1)), dt_1.draggingPointerId = fn, dt_1.dragOffsetY = fe - Gn, u.iframeScroll.set($e_1, dt_1), st == null || st(), ($n = Jt.stopPropagation) == null || $n.call(Jt); });
                    }
                    else
                        dt_1.track = { x: 0, y: 0, w: he, h: 0 }, dt_1.thumb = { x: 0, y: 0, w: he, h: 0 };
                    u.iframeScroll.set($e_1, dt_1);
                }
                var yt = [], Ct = m.tagName === "dialog" || m.tagName === "iframe" && !mt ? yt : q, vt = z;
                if (m.tagName === "dialog")
                    vt = { x: 0, y: 0, w: Math.max(0, ut), h: Math.max(0, ct) };
                else if (m.tagName === "iframe" && !mt) {
                    var J = (It = m.key) != null ? It : "", xt = u.iframeScroll.get(J), qt = xt ? xt.y : 0, Rt = 34;
                    vt = { x: 0, y: Rt + qt, w: Math.max(0, ut), h: Math.max(0, ct - Rt) };
                }
                var Yt = (at = rt != null ? rt : At) != null ? at : F, ne = y + ((Tt = At == null ? void 0 : At.position.x) != null ? Tt : 0), ae = I + ((Lt = At == null ? void 0 : At.position.y) != null ? Lt : 0) + (($t = rt == null ? void 0 : rt.position.y) != null ? $t : 0);
                L("render:draw:".concat(p, ":children"));
                var Ie = 0;
                for (var J = 0; J < ((Nt = m.children) != null ? Nt : []).length; J++) {
                    var xt = ((Ot = m.children) != null ? Ot : [])[J];
                    if (xt.kind === "block" && xt.tagName === "dialog")
                        Ct.push(xt);
                    else {
                        if (m.tagName === "button" && xt.kind === "text")
                            continue;
                        U(xt, Yt, ht, ne, ae, Ct, vt, "".concat(p, ".").concat(J), Ie++);
                    }
                }
                if ((m.tagName === "dialog" || m.tagName === "iframe" && !mt) && yt.length > 0) {
                    yt.sort(function (J, xt) { var te, Vt; var qt = J.key && (te = u.dialogZ.get(J.key)) != null ? te : 0, Rt = xt.key && (Vt = u.dialogZ.get(xt.key)) != null ? Vt : 0; return qt - Rt; });
                    try {
                        for (var yt_1 = __values(yt), yt_1_1 = yt_1.next(); !yt_1_1.done; yt_1_1 = yt_1.next()) {
                            var J = yt_1_1.value;
                            var xt = J.key && J.key.length > 0 ? J.key : "".concat(p, ".dlg.").concat(Ie);
                            U(J, Yt, ht, ne, ae, yt, vt, "".concat(p, ".dlg.").concat(xt), Ie++);
                        }
                    }
                    catch (e_58_1) { e_58 = { error: e_58_1 }; }
                    finally {
                        try {
                            if (yt_1_1 && !yt_1_1.done && (_a = yt_1.return)) _a.call(yt_1);
                        }
                        finally { if (e_58) throw e_58.error; }
                    }
                }
            }
            else {
                L("render:draw:".concat(p, ":text:begin"));
                var ht = St(P, "__text", function (ft) { ft.style = { fontFamily: r.fontFamily, fontSize: r.fontSize, fill: r.text, fontWeight: G.bold ? "700" : "400", wordWrap: !0, wordWrapWidth: 0 }; });
                ht.text = (Dt = m.text) != null ? Dt : "", ht.style.fontFamily = r.fontFamily, ht.style.fontSize = r.fontSize, ht.style.fill = r.text, ht.style.fontWeight = G.bold ? "700" : "400", ht.style.wordWrap = !0, ht.style.wordWrapWidth = Math.max(0, Math.ceil(m.width) + Te), ie(ht, 0, bt), L("render:draw:".concat(p, ":text:done"));
            }
        }
        L("render:root-loop");
        var D = { bold: !1 }, $ = { x: 0, y: 0, w: t.renderer.width, h: t.renderer.height }, v = [], O = s.position, M = O && Number(O.y || 0) || 0, W = 0;
        for (var m = 0; m < e.children.length; m++) {
            L("render:root-loop:".concat(m));
            var R = e.children[m];
            R && (R.kind === "block" && R.tagName === "dialog" ? v.push(R) : (L("render:root-loop:".concat(m, ":dispatch")), U(R, s, D, 0, M, v, $, "root.".concat(m), W++)));
        }
        if (L("render:root-dialogs"), v.length > 0) {
            v.sort(function (R, G) { var q, z; var j = R.key && (q = u.dialogZ.get(R.key)) != null ? q : 0, K = G.key && (z = u.dialogZ.get(G.key)) != null ? z : 0; return j - K; });
            var m = 0;
            try {
                for (var v_1 = __values(v), v_1_1 = v_1.next(); !v_1_1.done; v_1_1 = v_1.next()) {
                    var R = v_1_1.value;
                    var G = R.key && R.key.length > 0 ? R.key : "rootdlg.".concat(m);
                    U(R, l, D, 0, 0, v, $, "dlg.".concat(G), m++);
                }
            }
            catch (e_50_1) { e_50 = { error: e_50_1 }; }
            finally {
                try {
                    if (v_1_1 && !v_1_1.done && (_b = v_1.return)) _b.call(v_1);
                }
                finally { if (e_50) throw e_50.error; }
            }
        }
        if (L("render:temporal-popups"), f.length > 0 && ti({ popups: f, stage: a, theme: r, viewportW: t.renderer.width, viewportH: t.renderer.height, temporalStates: u.temporals, getOrInitInputValue: function (m, R) { return ln(m, R); }, sliders: u.sliders, sliderBounds: u.sliderBounds, sliderDrags: u.sliderDrags, selects: u.selects, selectPopups: d, uiFocus: u, getPointerId: Gt, getCursorColor: zt, requestPaint: st }), L("render:select-popups"), d.length > 0)
            try {
                for (var d_2 = __values(d), d_2_1 = d_2.next(); !d_2_1.done; d_2_1 = d_2.next()) {
                    var m = d_2_1.value;
                    Jr({ popup: m, stage: a, theme: r, selectStates: u.selects, uiState: u, getPointerId: Gt, requestPaint: st, viewportW: t.renderer.width, viewportH: t.renderer.height });
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
        var _loop_3 = function (m, R) {
            if (!(R != null && R.open))
                return "continue";
            var G = new _t;
            G.eventMode = "static", G.cursor = "default", ie(G, R.x, R.y);
            var j = 140, K = 28, q = 6, z = ["Copy", "Paste", "Close"], p = new wt;
            p.rect(0, 0, j + q * 2, z.length * K + q * 2), p.fill(16777215);
            var w = 1;
            p.rect(w, w, j + q * 2 - w * 2, z.length * K + q * 2 - w * 2), p.stroke({ width: 2, color: zt(m), alignment: 0 }), G.addChild(p), z.forEach(function (C, S) { var P = q + S * K, F = new _t; F.eventMode = "static", F.cursor = "pointer", ie(F, q, P); var y = new wt; y.rect(0, 0, j, K), y.fill(16777215), F.addChild(y); var I = Kt({ text: C, fontFamily: r.fontFamily, fontSize: r.fontSize, fill: r.text }); ie(I, 8, Math.max(0, (K - I.height) / 2) + bt), F.addChild(I); var H = function (A) { return Gt(A) === m; }; F.on("pointerover", function (A) { H(A) && (y.clear(), y.rect(0, 0, j, K), y.fill(15921906)); }), F.on("pointerout", function (A) { H(A) && (y.clear(), y.rect(0, 0, j, K), y.fill(16777215)); }), F.on("pointerdown", function (A) { var V, lt, tt, nt, ot, gt, It, at, Tt, Lt, $t; if (!H(A))
                return; (V = A.stopPropagation) == null || V.call(A); var X = (lt = u.focusedKeyByPointer.get(m)) != null ? lt : null, it = X ? u.inputs.get(X) : null, et = X != null && u.fieldBounds.has(X) && it != null && typeof it.value == "string"; if (C === "Copy" && et) {
                var Nt = it, Ot = (tt = Nt.value) != null ? tt : "", Dt = (ot = (nt = Nt.selections) == null ? void 0 : nt.get(m)) != null ? ot : null, ht = Dt ? Math.max(0, Math.min(Ot.length, (gt = Dt.start) != null ? gt : 0)) : 0, ft = Dt ? Math.max(0, Math.min(Ot.length, (It = Dt.end) != null ? It : ht)) : ht, ut = Math.min(ht, ft), ct = Math.max(ht, ft), Ut = ut !== ct ? Ot.slice(ut, ct) : Ot;
                u.clipboards.set(m, Ut);
            }
            else if (C === "Paste" && et) {
                var Nt = (at = u.clipboards.get(m)) != null ? at : "";
                if (Nt.length > 0) {
                    var Ot = it, Dt = (Tt = Ot.value) != null ? Tt : "";
                    if (Ot.selections || (Ot.selections = new Map), !Ot.selections.has(m)) {
                        var rt = Dt.length;
                        Ot.selections.set(m, { start: rt, end: rt });
                    }
                    var ht = Ot.selections.get(m), ft = Math.max(0, Math.min(Dt.length, (Lt = ht.start) != null ? Lt : Dt.length)), ut = Math.max(0, Math.min(Dt.length, ($t = ht.end) != null ? $t : ft)), ct = Math.min(ft, ut), Ut = Math.max(ft, ut);
                    Ot.value = Dt.slice(0, ct) + Nt + Dt.slice(Ut);
                    var At = ct + Nt.length;
                    ht.start = At, ht.end = At;
                }
            } var Y = u.contextMenus.get(m); Y && (Y.open = !1, u.contextMenus.set(m, Y)), st == null || st(); }), G.addChild(F); }), a.addChild(G);
        };
        try {
            for (var _j = __values(u.contextMenus.entries()), _k = _j.next(); !_k.done; _k = _j.next()) {
                var _l = __read(_k.value, 2), m = _l[0], R = _l[1];
                _loop_3(m, R);
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
            for (var _m = __values(b.entries()), _p = _m.next(); !_p.done; _p = _m.next()) {
                var _q = __read(_p.value, 2), m = _q[0], R = _q[1];
                if (!c.has(m)) {
                    try {
                        R.removeFromParent(), (B = R.destroy) == null || B.call(R, { children: !0 });
                    }
                    catch (G) { }
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
        L("render:done");
    }
    function rs() {
        return Ye(this, null, function () {
            var t, e, n, r, _a, i_1, _b, o_3, s, l, a_2, d, f_1, h_1, b_1, c_1, g_1, x_1, k_2, _, _c, T_1, U_1, D_1, $_1, v_2, O_1, M_1, W_1, N_1, B_1, m_2, R_2, G, j, K_2, q_3, p, w, C, z_1, n_3, r;
            return __generator(this, function (_d) {
                switch (_d.label) {
                    case 0:
                        _d.trys.push([0, 9, , 10]);
                        kt("main:start");
                        n = (t = document.getElementById("app")) != null ? t : document.body, r = !0;
                        kt("main:yoga");
                        if (!r) return [3 /*break*/, 1];
                        _a = Vo();
                        return [3 /*break*/, 3];
                    case 1: return [4 /*yield*/, Promise.resolve().then(function () { return (di(), ui); })];
                    case 2:
                        _a = (_d.sent()).default;
                        _d.label = 3;
                    case 3:
                        Q = _a, kt("main:create-app");
                        i_1 = r ? jo() : new je;
                        _b = r;
                        if (_b) return [3 /*break*/, 5];
                        return [4 /*yield*/, i_1.init({ background: "#ffffff", resizeTo: window, antialias: !1, preference: "webgl" })];
                    case 4:
                        _b = (_d.sent());
                        _d.label = 5;
                    case 5:
                        _b, kt("main:attach-capture"), ci(i_1), window.__TRUEOS_PIXI_APP = i_1, kt("main:append-canvas"), n.appendChild(i_1.canvas), i_1.ticker.stop(), kt("main:capture-flags"), r && (u.harness.enabled = !1, u.virtualCursor.enabled = !1, window.__pixiCapture && (window.__pixiCapture.persist = !1)), kt("main:canvas-listeners"), i_1.canvas.addEventListener("contextmenu", function (p) { return p.preventDefault(); }), i_1.canvas.addEventListener("wheel", function (p) { var F, y; var w = (F = p.offsetX) != null ? F : 0, C = (y = p.offsetY) != null ? y : 0, S = null; for (var I = u.iframeRects.length - 1; I >= 0; I--) {
                            var H = u.iframeRects[I];
                            if (w >= H.x && w <= H.x + H.w && C >= H.y && C <= H.y + H.h) {
                                S = H.key;
                                break;
                            }
                        } if (S) {
                            var I = u.iframeScroll.get(S);
                            if (I) {
                                var H = Math.max(0, I.contentHeight - I.viewportHeight);
                                H > 0 && (I.y = Math.max(0, Math.min(H, I.y + p.deltaY)), u.iframeScroll.set(S, I), st == null || st(), p.preventDefault());
                                return;
                            }
                        } var P = Math.max(0, u.scroll.contentHeight - u.scroll.viewportHeight); P <= 0 || (u.scroll.y = Math.max(0, Math.min(P, u.scroll.y + p.deltaY)), st == null || st(), p.preventDefault()); }, { passive: !1 }), kt("main:stage:eventMode"), i_1.stage.eventMode = "static", kt("main:stage:hitArea"), i_1.stage.hitArea = i_1.screen, kt("main:stage:on:pointerdown"), i_1.stage.on("pointerdown", function (p) {
                            var e_59, _a;
                            var w, C, S, P, F, y;
                            if ((p == null ? void 0 : p.button) === 2) {
                                var I = Gt(p);
                                if (I > 0) {
                                    var H = (w = u.contextMenus.get(I)) != null ? w : { open: !1, x: 0, y: 0 };
                                    H.open = !0, H.x = (S = (C = p.global) == null ? void 0 : C.x) != null ? S : 0, H.y = (F = (P = p.global) == null ? void 0 : P.y) != null ? F : 0, u.contextMenus.set(I, H);
                                }
                                st == null || st(), (y = p.preventDefault) == null || y.call(p);
                                return;
                            }
                            if ((p == null ? void 0 : p.button) !== 2) {
                                var I = Gt(p), H = I > 0 ? u.contextMenus.get(I) : null;
                                H && H.open && (H.open = !1, u.contextMenus.set(I, H), st == null || st());
                            }
                            if ((p == null ? void 0 : p.button) !== 2) {
                                var I = !1;
                                try {
                                    for (var _b = __values(u.selects.values()), _c = _b.next(); !_c.done; _c = _b.next()) {
                                        var H = _c.value;
                                        H.open && (H.open = !1, I = !0);
                                    }
                                }
                                catch (e_59_1) { e_59 = { error: e_59_1 }; }
                                finally {
                                    try {
                                        if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
                                    }
                                    finally { if (e_59) throw e_59.error; }
                                }
                                I && (st == null || st());
                            }
                            (p == null ? void 0 : p.button) !== 2 && ei(u.temporals) && (st == null || st()), R_2();
                        }), kt("main:stage:done"), kt("main:roots");
                        o_3 = new _t, s = new _t;
                        s.eventMode = "static";
                        l = new _t;
                        l.eventMode = "none", i_1.stage.addChild(o_3), i_1.stage.addChild(s), i_1.stage.addChild(l);
                        a_2 = new wt;
                        a_2.label = "__trueosGlobalScrollbar", a_2.eventMode = "static", s.addChild(a_2);
                        d = function (p, w) { p.clear(); var C = w.half, S = w.strokeWidth, P = w.color; p.moveTo(-C, 0), p.lineTo(C, 0), p.stroke({ width: S, color: P }), p.moveTo(0, -C), p.lineTo(0, C), p.stroke({ width: S, color: P }); }, f_1 = new wt;
                        f_1.eventMode = "none", f_1.visible = !1, l.addChild(f_1);
                        h_1 = new wt;
                        h_1.eventMode = "none", h_1.visible = !1, l.addChild(h_1);
                        b_1 = new wt;
                        b_1.eventMode = "none", b_1.visible = !1, l.addChild(b_1);
                        c_1 = new wt;
                        c_1.eventMode = "none", l.addChild(c_1), kt("main:text-measure");
                        g_1 = document.createElement("canvas").getContext("2d");
                        if (!g_1)
                            throw new Error("2D canvas not available");
                        g_1.font = "".concat(ge.fontSize, "px ").concat(ge.fontFamily);
                        x_1 = function (p) { return g_1.measureText(p).width; }, k_2 = ge.fontSize * 1.25;
                        kt("main:html");
                        if (!(typeof window.__TRUEOS_INPUT_HTML__ == "string")) return [3 /*break*/, 6];
                        _c = window.__TRUEOS_INPUT_HTML__;
                        return [3 /*break*/, 8];
                    case 6: return [4 /*yield*/, fetch("/input.html").then(function (p) { return p.text(); })];
                    case 7:
                        _c = _d.sent();
                        _d.label = 8;
                    case 8:
                        _ = _c;
                        jt() && console.log("[trueos pixi widgets] input-html chars=".concat(_.length, " sample=\"").concat(Lo(_), "\"")), kt("main:render-tree"), pi.clear();
                        T_1 = qo(window.__TRUEOS_WIDGET_RENDER_TREE__);
                        if (jt() && console.log("[trueos pixi widgets] render-tree source=truesurfer nodes=".concat(T_1.length)), T_1.length === 0)
                            throw new Error("TrueSurfer widget render tree is missing");
                        U_1 = No(T_1), D_1 = null, $_1 = { nodes: 0, blocks: 0, text: 0, maxDepth: 0, tags: {} }, v_2 = 0, O_1 = function () { var p = Math.max(0, u.scroll.contentHeight - u.scroll.viewportHeight); u.scroll.y = Math.max(0, Math.min(u.scroll.y, p)); }, M_1 = function () { var p = i_1.renderer.width, w = i_1.renderer.height; u.scroll.viewportHeight = w; var C = u.scroll.contentHeight, S = Math.max(0, C - w), P = S > .5; if (a_2.clear(), a_2.visible = P, !P) {
                            u.scroll.track = { x: 0, y: 0, w: u.scroll.track.w, h: 0 }, u.scroll.thumb = { x: 0, y: 0, w: u.scroll.thumb.w, h: 0 };
                            return;
                        } var F = Pn, y = Pe, I = Math.max(0, p - y - F), H = F, A = Math.max(0, w - F * 2), it = Math.max(24, w / Math.max(w, C) * A), et = Math.max(1, A - it), Y = S <= 0 ? 0 : u.scroll.y / S, V = H + et * Y; u.scroll.track = { x: I, y: H, w: y, h: A }, u.scroll.thumb = { x: I, y: V, w: y, h: it }, a_2.rect(I, H, y, A), a_2.fill({ color: 0, alpha: .06 }), a_2.rect(I, V, y, it), a_2.fill({ color: 0, alpha: .25 }); }, W_1 = function () { if (D_1) {
                            if (kt("main:paint:clamp"), O_1(), kt("main:paint:render-to-pixi"), ns(i_1, D_1, o_3), kt("main:paint:scrollbar"), M_1(), kt("main:paint:renderer-render"), i_1.renderer.render(i_1.stage), zo(U_1, $_1, Bo(T_1), Fo(D_1)), jt()) {
                                var p = Xo(D_1);
                                window.__TRUEOS_PIXI_LAYOUT_TEXT_OVERLAYS__ = p, v_2 < 4 && (v_2 += 1, console.log("[trueos pixi widgets] layout-text-overlays count=".concat(p.length, " samples=").concat(Yo(p))));
                            }
                            kt("main:paint:done");
                        } };
                        jt() && (window.__TRUEOS_REPAINT_NOW__ = function () { window.__TRUEOS_PIXI_DIRTY__ = !1, W_1(); });
                        N_1 = function () { kt("main:layout-build"); var p = es(T_1, window.innerWidth, window.innerHeight); kt("main:layout-commit"), D_1 = p, jt() && (window.__TRUEOS_PIXI_LAST_LAYOUT__ = p, window.__TRUEOS_PIXI_LAYOUT_TEXT_OVERLAYS__ = []), $_1 = Go(p), u.scroll.contentHeight = Jo(p), u.scroll.viewportHeight = window.innerHeight, W_1(); };
                        cn = function () { N_1(); };
                        B_1 = !1, m_2 = !1, R_2 = function () { if (jt()) {
                            window.__TRUEOS_PIXI_DIRTY__ = !0;
                            return;
                        } m_2 || B_1 || (m_2 = !0, requestAnimationFrame(function () { m_2 = !1, i_1.renderer.render(i_1.stage); })); };
                        st = function () { if (!B_1) {
                            if (jt()) {
                                window.__TRUEOS_PIXI_DIRTY__ = !0;
                                return;
                            }
                            B_1 = !0, requestAnimationFrame(function () { B_1 = !1, W_1(); });
                        } }, kt("main:first-rerender"), N_1(), kt("main:cursor-setup");
                        G = 2, j = 10, K_2 = jt();
                        d(f_1, { half: j, strokeWidth: G, color: zt(Bt) }), d(h_1, { half: j, strokeWidth: G, color: zt(Ft) }), d(b_1, { half: j, strokeWidth: G, color: zt(Wt) });
                        q_3 = 2;
                        if (d(c_1, { half: j, strokeWidth: G, color: zt(q_3) }), u.userCursorPos.set(Bt, { x: i_1.renderer.width * .25, y: i_1.renderer.height * .5 }), u.userCursorPos.set(Ft, { x: i_1.renderer.width * .25 + 40, y: i_1.renderer.height * .5 + 20 }), u.userCursorPos.set(Wt, { x: i_1.renderer.width * .25 + 80, y: i_1.renderer.height * .5 + 40 }), f_1.visible = !K_2, h_1.visible = !K_2, b_1.visible = !K_2, !K_2) {
                            p = u.userCursorPos.get(Bt), w = u.userCursorPos.get(Ft), C = u.userCursorPos.get(Wt);
                            f_1.position.set(p.x, p.y), h_1.position.set(w.x, w.y), b_1.position.set(C.x, C.y);
                        }
                        c_1.visible = !K_2 && u.virtualCursor.enabled;
                        z_1 = function () { if (K_2) {
                            f_1.visible = !1, h_1.visible = !1, b_1.visible = !1, c_1.visible = !1;
                            return;
                        } var p = u.userCursorPos.get(Bt), w = u.userCursorPos.get(Ft), C = u.userCursorPos.get(Wt); p && (f_1.visible = !0, f_1.position.set(p.x, p.y)), w && (h_1.visible = !0, h_1.position.set(w.x, w.y)), C && (b_1.visible = !0, b_1.position.set(C.x, C.y)); var S = function (P, F) { var y = null, I = null; for (var H = u.hoverRects.length - 1; H >= 0; H--) {
                            var A = u.hoverRects[H];
                            if (P >= A.x && P <= A.x + A.w && F >= A.y && F <= A.y + A.h) {
                                y = A.key, I = A.cursor;
                                break;
                            }
                        } return { hitKey: y, hitCursor: I }; }; if (p) {
                            var _a = S(p.x, p.y), P = _a.hitKey, F = _a.hitCursor;
                            u.hoveredKeyByPointer.set(Bt, P), u.hoveredCursorByPointer.set(Bt, F);
                            var y = u.textDrags.has(Bt) || u.sliderDrags.has(Bt) || u.dialogDrags.has(Bt);
                            f_1.rotation = F != null || y ? Math.PI / 4 : 0;
                        } if (w) {
                            var _b = S(w.x, w.y), P = _b.hitKey, F = _b.hitCursor;
                            u.hoveredKeyByPointer.set(Ft, P), u.hoveredCursorByPointer.set(Ft, F);
                            var y = u.textDrags.has(Ft) || u.sliderDrags.has(Ft) || u.dialogDrags.has(Ft);
                            h_1.rotation = F != null || y ? Math.PI / 4 : 0;
                        } if (C) {
                            var _c = S(C.x, C.y), P = _c.hitKey, F = _c.hitCursor;
                            u.hoveredKeyByPointer.set(Wt, P), u.hoveredCursorByPointer.set(Wt, F);
                            var y = u.textDrags.has(Wt) || u.sliderDrags.has(Wt) || u.dialogDrags.has(Wt);
                            b_1.rotation = F != null || y ? Math.PI / 4 : 0;
                        } R_2(); };
                        u.harness.enabled && setInterval(function () {
                            var e_60, _a, e_61, _b;
                            var p = u.harness.activeUserPointerId, w = p === Bt ? Ft : p === Ft ? Wt : Bt;
                            if (u.harness.activeUserPointerId = w, u.lastMouse.has) {
                                var A = u.userCursorPos.get(p), X = u.userCursorPos.get(w);
                                u.userCursorPos.set(w, { x: u.lastMouse.x, y: u.lastMouse.y }), X ? u.userCursorPos.set(p, { x: X.x, y: X.y }) : A && u.userCursorPos.set(p, { x: A.x, y: A.y });
                            }
                            var C = u.textDrags.size > 0, S = u.sliderDrags.size > 0, P = u.dialogDrags.size > 0, F = u.scroll.draggingPointerId != null, y = u.color.draggingPointerId != null, I = !1;
                            try {
                                for (var _c = __values(u.iframeScroll.values()), _d = _c.next(); !_d.done; _d = _c.next()) {
                                    var A = _d.value;
                                    if (A.draggingPointerId != null) {
                                        I = !0;
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
                            var H = C || S || P || F || y || I;
                            u.textDrags.delete(Bt), u.textDrags.delete(Ft), u.textDrags.delete(Wt), u.sliderDrags.delete(Bt), u.sliderDrags.delete(Ft), u.sliderDrags.delete(Wt), u.dialogDrags.delete(Bt), u.dialogDrags.delete(Ft), u.dialogDrags.delete(Wt);
                            try {
                                for (var _f = __values([Bt, Ft, Wt]), _g = _f.next(); !_g.done; _g = _f.next()) {
                                    var A = _g.value;
                                    var X = u.numberHolds.get(A);
                                    X && (X.timeoutId != null && window.clearTimeout(X.timeoutId), X.intervalId != null && window.clearInterval(X.intervalId), u.numberHolds.delete(A));
                                }
                            }
                            catch (e_61_1) { e_61 = { error: e_61_1 }; }
                            finally {
                                try {
                                    if (_g && !_g.done && (_b = _f.return)) _b.call(_f);
                                }
                                finally { if (e_61) throw e_61.error; }
                            }
                            (u.scroll.draggingPointerId === Bt || u.scroll.draggingPointerId === Ft || u.scroll.draggingPointerId === Wt) && (u.scroll.draggingPointerId = null), (u.color.draggingPointerId === Bt || u.color.draggingPointerId === Ft || u.color.draggingPointerId === Wt) && (u.color.draggingPointerId = null), z_1(), H && (st == null || st());
                        }, u.harness.periodMs), !K_2 && u.virtualCursor.enabled && i_1.ticker.add(function () { var F, y, I, H, A; var p = Math.max(0, i_1.ticker.deltaMS) / 1e3; c_1.visible = !0, u.virtualCursor.t += p; var w = i_1.renderer.width * .75, C = i_1.renderer.height * .25, S = u.virtualCursor.t * u.virtualCursor.speed, P = u.virtualCursor.radius; u.virtualCursor.x = w + Math.cos(S) * P, u.virtualCursor.y = C + Math.sin(S) * P, c_1.position.set(u.virtualCursor.x, u.virtualCursor.y); {
                            var X = q_3, it = u.virtualCursor.x, et = u.virtualCursor.y, Y = null, V = null;
                            for (var nt = u.hoverRects.length - 1; nt >= 0; nt--) {
                                var ot = u.hoverRects[nt];
                                if (it >= ot.x && it <= ot.x + ot.w && et >= ot.y && et <= ot.y + ot.h) {
                                    Y = ot.key, V = ot.cursor;
                                    break;
                                }
                            }
                            var lt = (F = u.hoveredKeyByPointer.get(X)) != null ? F : null;
                            lt !== Y && (lt && ((I = (y = u.hoverHandlers.get(lt)) == null ? void 0 : y.out) == null || I.call(y)), Y && ((A = (H = u.hoverHandlers.get(Y)) == null ? void 0 : H.over) == null || A.call(H)), u.hoveredKeyByPointer.set(X, Y)), u.hoveredCursorByPointer.set(X, V);
                            var tt = u.textDrags.has(X) || u.sliderDrags.has(X) || u.dialogDrags.has(X);
                            c_1.rotation = V != null || tt ? Math.PI / 4 : 0;
                        } }), u.virtualCursor.x = i_1.renderer.width * .75 + u.virtualCursor.radius, u.virtualCursor.y = i_1.renderer.height * .25, c_1.position.set(u.virtualCursor.x, u.virtualCursor.y), jt() && W_1(), i_1.stage.on("pointerup", function (p) {
                            var e_62, _a;
                            var S, P, F;
                            var w = Gt(p), C = (P = (S = u.sliderDrags.get(w)) == null ? void 0 : S.key) != null ? P : null;
                            u.textDrags.delete(w), u.sliderDrags.delete(w), u.dialogDrags.delete(w), u.scroll.draggingPointerId === w && (u.scroll.draggingPointerId = null), u.color.draggingPointerId === w && (u.color.draggingPointerId = null);
                            try {
                                for (var _b = __values(u.iframeScroll.values()), _c = _b.next(); !_c.done; _c = _b.next()) {
                                    var y = _c.value;
                                    y.draggingPointerId === w && (y.draggingPointerId = null);
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
                                var y = u.numberHolds.get(w);
                                y && (y.timeoutId != null && window.clearTimeout(y.timeoutId), y.intervalId != null && window.clearInterval(y.intervalId), u.numberHolds.delete(w));
                            }
                            if (C) {
                                var y = (F = u.temporalYearOwners.get(C)) != null ? F : null;
                                if (y) {
                                    var I = u.temporals.get(y);
                                    I && I.openYear && (I.openYear = !1, u.temporals.set(y, I), st == null || st());
                                }
                            }
                            R_2();
                        }), i_1.stage.on("pointerupoutside", function (p) {
                            var e_63, _a;
                            var S, P, F;
                            var w = Gt(p), C = (P = (S = u.sliderDrags.get(w)) == null ? void 0 : S.key) != null ? P : null;
                            u.textDrags.delete(w), u.sliderDrags.delete(w), u.dialogDrags.delete(w), u.scroll.draggingPointerId === w && (u.scroll.draggingPointerId = null), u.color.draggingPointerId === w && (u.color.draggingPointerId = null);
                            try {
                                for (var _b = __values(u.iframeScroll.values()), _c = _b.next(); !_c.done; _c = _b.next()) {
                                    var y = _c.value;
                                    y.draggingPointerId === w && (y.draggingPointerId = null);
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
                                var y = u.numberHolds.get(w);
                                y && (y.timeoutId != null && window.clearTimeout(y.timeoutId), y.intervalId != null && window.clearInterval(y.intervalId), u.numberHolds.delete(w));
                            }
                            if (C) {
                                var y = (F = u.temporalYearOwners.get(C)) != null ? F : null;
                                if (y) {
                                    var I = u.temporals.get(y);
                                    I && I.openYear && (I.openYear = !1, u.temporals.set(y, I), st == null || st());
                                }
                            }
                            R_2();
                        }), a_2.on("pointerdown", function (p) { var et, Y, V, lt, tt, nt; if ((p == null ? void 0 : p.button) === 2)
                            return; var w = Gt(p); if (w <= 0)
                            return; var C = (Y = (et = p.global) == null ? void 0 : et.x) != null ? Y : 0, S = (lt = (V = p.global) == null ? void 0 : V.y) != null ? lt : 0, P = u.scroll.track, F = u.scroll.thumb; if (!(C >= P.x && C <= P.x + P.w && S >= P.y && S <= P.y + P.h))
                            return; var I = Math.max(0, u.scroll.contentHeight - u.scroll.viewportHeight); if (I <= .5)
                            return; if (C >= F.x && C <= F.x + F.w && S >= F.y && S <= F.y + F.h) {
                            u.scroll.draggingPointerId = w, u.scroll.dragOffsetY = S - F.y, (tt = p.stopPropagation) == null || tt.call(p);
                            return;
                        } var A = Math.max(1, P.h - F.h), X = Math.max(P.y, Math.min(P.y + A, S - F.h / 2)), it = (X - P.y) / A; u.scroll.y = Math.max(0, Math.min(I, it * I)), u.scroll.draggingPointerId = w, u.scroll.dragOffsetY = S - X, st == null || st(), (nt = p.stopPropagation) == null || nt.call(p); }), i_1.stage.on("pointermove", function (p) {
                            var e_64, _a;
                            var y, I, H, A, X, it, et, Y, V, lt, tt, nt, ot, gt, It, at, Tt, Lt, $t, Nt, Ot, Dt, ht, ft, ut, ct, Ut, At;
                            var w = Number((H = (I = p == null ? void 0 : p.pointerId) != null ? I : (y = p == null ? void 0 : p.data) == null ? void 0 : y.pointerId) != null ? H : 1);
                            if (String((it = (X = p == null ? void 0 : p.pointerType) != null ? X : (A = p == null ? void 0 : p.data) == null ? void 0 : A.pointerType) != null ? it : "").toLowerCase() === "mouse" || w === 1) {
                                var rt = (Y = (et = p.global) == null ? void 0 : et.x) != null ? Y : 0, mt = (lt = (V = p.global) == null ? void 0 : V.y) != null ? lt : 0;
                                u.lastMouse.x = rt, u.lastMouse.y = mt, u.lastMouse.has = !0, u.primaryMousePointerId = w;
                                var yt = u.harness.enabled ? u.harness.activeUserPointerId : w;
                                u.userCursorPos.set(yt, { x: rt, y: mt }), z_1();
                            }
                            var P = Gt(p);
                            if (P <= 0)
                                return;
                            var F = !1;
                            {
                                var rt = u.textDrags.get(P);
                                if (rt) {
                                    var mt = rt.key, yt = u.fieldBounds.get(mt), Ct = u.inputs.get(mt);
                                    if (yt && Ct && typeof Ct.value == "string") {
                                        var vt = yt.isPassword ? "\u2022".repeat(Ct.value.length) : Ct.value, Yt = ue(ce(vt, Math.max(0, yt.innerWidth), x_1), yt.maxLines), ne = ((nt = (tt = p.global) == null ? void 0 : tt.x) != null ? nt : 0) - yt.x - yt.innerLeft, ae = ((gt = (ot = p.global) == null ? void 0 : ot.y) != null ? gt : 0) - yt.y - yt.innerTop, Ie = Me({ fullText: vt, lines: Yt, localX: ne, localY: ae, lineHeight: k_2, measure: x_1 });
                                        Ct.selections || (Ct.selections = new Map), Ct.selections.set(P, { start: rt.anchor, end: Ie }), F = !0;
                                    }
                                }
                            }
                            {
                                var rt = u.sliderDrags.get(P);
                                if (rt) {
                                    var mt = rt.key, yt = u.sliderBounds.get(mt);
                                    if (yt) {
                                        var vt = ((at = (It = p.global) == null ? void 0 : It.x) != null ? at : 0) - yt.x, Yt = Math.max(1, yt.w - yt.innerPad * 2), ne = (vt - yt.innerPad) / Yt, ae = ye(u.sliders, mt, void 0);
                                        ae.value = Math.max(0, Math.min(1, ne)), F = !0;
                                    }
                                }
                            }
                            {
                                var rt = u.color.draggingPointerId;
                                if (rt != null && rt === P) {
                                    var mt = u.color.bounds;
                                    if (mt) {
                                        var yt = (Lt = (Tt = p.global) == null ? void 0 : Tt.x) != null ? Lt : 0, Ct = (Nt = ($t = p.global) == null ? void 0 : $t.y) != null ? Nt : 0, vt = yt - mt.x, Yt = Ct - mt.y, ne = Tn({ lx: vt, ly: Yt, w: mt.w, h: mt.h });
                                        ne && (u.color.rgb = ne, u.color.pick = { x: vt, y: Yt }, F = !0);
                                    }
                                }
                            }
                            {
                                var rt = u.scroll.draggingPointerId;
                                if (rt != null && rt === P) {
                                    var mt = u.scroll.track, yt = u.scroll.thumb, Ct = Math.max(0, u.scroll.contentHeight - u.scroll.viewportHeight);
                                    if (Ct > .5 && mt.h > 0 && yt.h > 0) {
                                        var vt = (Dt = (Ot = p.global) == null ? void 0 : Ot.y) != null ? Dt : 0, Yt = Math.max(1, mt.h - yt.h), ae = (Math.max(mt.y, Math.min(mt.y + Yt, vt - u.scroll.dragOffsetY)) - mt.y) / Yt;
                                        u.scroll.y = Math.max(0, Math.min(Ct, ae * Ct)), F = !0;
                                    }
                                }
                            }
                            try {
                                for (var _b = __values(u.iframeScroll.values()), _c = _b.next(); !_c.done; _c = _b.next()) {
                                    var rt = _c.value;
                                    if (rt.draggingPointerId == null || rt.draggingPointerId !== P)
                                        continue;
                                    var mt = Math.max(0, rt.contentHeight - rt.viewportHeight);
                                    if (mt <= .5 || rt.track.h <= 0 || rt.thumb.h <= 0)
                                        continue;
                                    var yt = (ft = (ht = p.global) == null ? void 0 : ht.y) != null ? ft : 0, Ct = Math.max(1, rt.track.h - rt.thumb.h), Yt = (Math.max(rt.track.y, Math.min(rt.track.y + Ct, yt - rt.dragOffsetY)) - rt.track.y) / Ct;
                                    rt.y = Math.max(0, Math.min(mt, Yt * mt)), F = !0;
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
                                var rt = u.dialogDrags.get(P);
                                if (rt) {
                                    var mt = Je(u.dialogs, rt.key), yt = (ct = (ut = p.global) == null ? void 0 : ut.x) != null ? ct : 0, Ct = (At = (Ut = p.global) == null ? void 0 : Ut.y) != null ? At : 0;
                                    mt.x = rt.originX + (yt - rt.startGX), mt.y = rt.originY + (Ct - rt.startGY);
                                    var vt = u.dialogDragBounds.get(rt.key);
                                    vt && (mt.x = Math.max(vt.minX, Math.min(vt.maxX, mt.x)), mt.y = Math.max(vt.minY, Math.min(vt.maxY, mt.y))), F = !0;
                                }
                            }
                            F && (st == null || st());
                        }), kt("main:input-listeners"), window.addEventListener("keydown", function (p) {
                            var V, lt, tt, nt, ot, gt, It;
                            var w = u.keyboardOwnerPointerId, C = (V = u.focusedKeyByPointer.get(w)) != null ? V : null;
                            if (!C)
                                return;
                            var S = u.inputs.get(C);
                            if (!S || typeof S.value != "string")
                                return;
                            if (S.selections || (S.selections = new Map), !S.selections.has(w)) {
                                var at = S.value.length;
                                S.selections.set(w, { start: at, end: at });
                            }
                            var P = S.selections.get(w), F = S.value.length, y = function (at) { return Math.max(0, Math.min(F, at)); }, I = y((lt = P.start) != null ? lt : F), H = y((tt = P.end) != null ? tt : I);
                            P.start = I, P.end = H;
                            var A = Math.min(I, H), X = Math.max(I, H), it = A !== X, et = function (at) { var Tt = Math.max(0, Math.min(S.value.length, at)); P.start = Tt, P.end = Tt; }, Y = function (at, Tt) { P.start = Math.max(0, Math.min(S.value.length, at)), P.end = Math.max(0, Math.min(S.value.length, Tt)); };
                            if (p.key.toLowerCase() === "a" && (p.ctrlKey || p.metaKey)) {
                                Y(0, S.value.length), p.preventDefault(), W_1();
                                return;
                            }
                            if (p.key === "ArrowLeft" || p.key === "ArrowRight") {
                                var at = p.key === "ArrowLeft" ? -1 : 1;
                                if (p.shiftKey) {
                                    var Tt = (nt = P.start) != null ? nt : F, Lt = ((ot = P.end) != null ? ot : Tt) + at;
                                    Y(Tt, Lt);
                                }
                                else
                                    et((it ? A : X) + at);
                                p.preventDefault(), N_1();
                                return;
                            }
                            if (p.key === "Home") {
                                p.shiftKey ? Y((gt = P.start) != null ? gt : F, 0) : et(0), p.preventDefault(), N_1();
                                return;
                            }
                            if (p.key === "End") {
                                p.shiftKey ? Y((It = P.start) != null ? It : 0, S.value.length) : et(S.value.length), p.preventDefault(), N_1();
                                return;
                            }
                            if (p.key === "Backspace") {
                                if (it)
                                    S.value = S.value.slice(0, A) + S.value.slice(X), et(A);
                                else {
                                    var at = X;
                                    at > 0 && (S.value = S.value.slice(0, at - 1) + S.value.slice(at), et(at - 1));
                                }
                                p.preventDefault(), N_1();
                                return;
                            }
                            if (p.key === "Enter") {
                                var at = "\n";
                                if (it)
                                    S.value = S.value.slice(0, A) + at + S.value.slice(X), et(A + at.length);
                                else {
                                    var Tt = X;
                                    S.value = S.value.slice(0, Tt) + at + S.value.slice(Tt), et(Tt + at.length);
                                }
                                p.preventDefault(), N_1();
                                return;
                            }
                            if (p.key === "Delete") {
                                if (it)
                                    S.value = S.value.slice(0, A) + S.value.slice(X), et(A);
                                else {
                                    var at = X;
                                    at < S.value.length && (S.value = S.value.slice(0, at) + S.value.slice(at + 1), et(at));
                                }
                                p.preventDefault(), N_1();
                                return;
                            }
                            if (p.key === "Escape") {
                                u.focusedKeyByPointer.set(w, null), N_1();
                                return;
                            }
                            if (p.key.length === 1 && !p.ctrlKey && !p.metaKey && !p.altKey) {
                                if (it)
                                    S.value = S.value.slice(0, A) + p.key + S.value.slice(X), et(A + 1);
                                else {
                                    var at = X;
                                    S.value = S.value.slice(0, at) + p.key + S.value.slice(at), et(at + 1);
                                }
                                p.preventDefault(), N_1();
                            }
                        }), window.addEventListener("resize", function () { N_1(), c_1.visible = u.virtualCursor.enabled; }), kt("main:done"), r && (window.__TRUEOS_PIXI_APP_READY__ = !0, window.__TRUEOS_PIXI_APP_ERROR__ = "", window.__TRUEOS_PIXI_APP_PHASE__ = "ready");
                        return [3 /*break*/, 10];
                    case 9:
                        n_3 = _d.sent();
                        window.__TRUEOS_PIXI_APP_READY__ = !1, window.__TRUEOS_PIXI_APP_ERROR__ = _i(n_3);
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
    rs().then(function () { window.__TRUEOS_PIXI_APP_ERROR__ || (window.__TRUEOS_PIXI_APP_READY__ = !0, window.__TRUEOS_PIXI_APP_ERROR__ = "", window.__TRUEOS_PIXI_APP_PHASE__ = "ready"); }).catch(function (t) { var n; window.__TRUEOS_PIXI_APP_READY__ = !1, window.__TRUEOS_PIXI_APP_ERROR__ = _i(t), console.error(t); var e = document.createElement("pre"); e.textContent = String((n = t == null ? void 0 : t.stack) != null ? n : t), document.body.appendChild(e); });
})();
