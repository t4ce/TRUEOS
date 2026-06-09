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
    var Jn = Object.defineProperty, Ni = Object.defineProperties;
    var vi = Object.getOwnPropertyDescriptors;
    var Vn = Object.getOwnPropertySymbols;
    var Gi = Object.prototype.hasOwnProperty, Li = Object.prototype.propertyIsEnumerable;
    var wn = function (t, e, n) { return e in t ? Jn(t, e, { enumerable: !0, configurable: !0, writable: !0, value: n }) : t[e] = n; }, re = function (t, e) {
        var e_1, _a;
        for (var n in e || (e = {}))
            Gi.call(e, n) && wn(t, n, e[n]);
        if (Vn)
            try {
                for (var _b = __values(Vn(e)), _c = _b.next(); !_c.done; _c = _b.next()) {
                    var n = _c.value;
                    Li.call(e, n) && wn(t, n, e[n]);
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
    }, ge = function (t, e) { return Ni(t, vi(e)); };
    var Fi = function (t, e) { return function () { return (t && (e = t(t = 0)), e); }; };
    var Bi = function (t, e) { for (var n in e)
        Jn(t, n, { get: e[n], enumerable: !0 }); };
    var et = function (t, e, n) { return wn(t, typeof e != "symbol" ? e + "" : e, n); };
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
    var gi = {};
    Bi(gi, { default: function () { return vo; } });
    var vo, bi = Fi(function () { vo = {}; });
    var Re = /** @class */ (function () {
        function Re(e, n) {
            if (e === void 0) { e = 0; }
            if (n === void 0) { n = 0; }
            et(this, "x");
            et(this, "y");
            this.x = Number(e) || 0, this.y = Number(n) || 0;
        }
        Re.prototype.set = function (e, n) {
            if (e === void 0) { e = 0; }
            if (n === void 0) { n = e; }
            this.x = Number(e) || 0, this.y = Number(n) || 0;
        };
        return Re;
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
    }()), _n = /** @class */ (function () {
        function _n() {
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
            this.parent = null, this.position = new Re, this.scale = new Re(1, 1), this.pivot = new Re, this.visible = !0, this.alpha = 1, this.mask = null, this.rotation = 0, this.zIndex = 0, this.eventMode = null, this.cursor = null, this.hitArea = null, this.listeners = {};
        }
        Object.defineProperty(_n.prototype, "x", {
            get: function () { return this.position.x; },
            set: function (e) { this.position.x = Number(e) || 0; },
            enumerable: false,
            configurable: true
        });
        Object.defineProperty(_n.prototype, "y", {
            get: function () { return this.position.y; },
            set: function (e) { this.position.y = Number(e) || 0; },
            enumerable: false,
            configurable: true
        });
        _n.prototype.on = function (e, n) { return this; };
        _n.prototype.removeAllListeners = function (e) { return e == null ? this.listeners = {} : delete this.listeners[String(e)], this; };
        _n.prototype.removeFromParent = function () { var e; return (e = this.parent) == null || e.removeChild(this), this; };
        _n.prototype.destroy = function (e) { this.removeFromParent(), this.removeAllListeners(); };
        _n.prototype.toLocal = function (e) { var n = e || {}; return { x: (Number(n.x) || 0) - this.getGlobalX(), y: (Number(n.y) || 0) - this.getGlobalY() }; };
        _n.prototype.getGlobalPosition = function () { return { x: this.getGlobalX(), y: this.getGlobalY() }; };
        _n.prototype.getGlobalX = function () { return (this.parent ? this.parent.getGlobalX() : 0) + this.x; };
        _n.prototype.getGlobalY = function () { return (this.parent ? this.parent.getGlobalY() : 0) + this.y; };
        return _n;
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
    }(_n)), wt = /** @class */ (function (_super) {
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
            _this.geometry = (r = n.geometry) != null ? r : new Te, _this.shader = (i = n.shader) != null ? i : new De;
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
    }()), Tn = { VERTEX: 1, COPY_DST: 2 }, De = /** @class */ (function () {
        function De(e) {
            if (e === void 0) { e = {}; }
            et(this, "options");
            this.options = e;
        }
        return De;
    }());
    var Zn = "", Qn = "", qn = "", Je = /** @class */ (function () {
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
    function jt(t) { var i, o; var e = t.wrapWidth, n = (i = t.wordWrap) != null ? i : e != null, r = (o = t.wordWrapWidth) != null ? o : e == null ? void 0 : Math.max(0, Math.ceil(e) + Me); return new Qt({ text: t.text, style: { fontFamily: t.fontFamily, fontSize: t.fontSize, fill: t.fill, fontWeight: t.bold ? "700" : "400", wordWrap: n, wordWrapWidth: r } }); }
    function Mn(t, e) { var n = t.children; if (!Array.isArray(n))
        return null; for (var r = 0; r < n.length; r += 1) {
        var i = n[r];
        if (i && i.label === e)
            return i;
    } return null; }
    function xe(t, e) { var n = Mn(t, e); if (n)
        return n; var r = new _t; return r.label = e, t.addChild(r), r; }
    function St(t, e) { var n = Mn(t, e); if (n)
        return n; var r = new wt; return r.label = e, t.addChild(r), r; }
    function It(t, e, n) { var r = Mn(t, e); if (r)
        return r; var i = new Qt({ text: "" }); return i.label = e, n == null || n(i), t.addChild(i), i; }
    function Et(t) { t.clear(), t.removeAllListeners(), t.hitArea = null; }
    function $t(t) { t.removeAllListeners(); }
    function ce(t, e, n) {
        var r = String(t != null ? t : ""), i = [], o = 0;
        for (var s = 0; s <= r.length; s++) {
            if (!(s === r.length || r[s] === "\n"))
                continue;
            var a = o, d = s;
            if (a === d)
                i.push({ start: a, end: d, text: "" });
            else {
                var f = a, m = -1;
                for (var b = f; b < d; b++) {
                    r[b] === " " && (m = b);
                    var M = r.slice(f, b + 1);
                    if (n(M) <= e || b === f)
                        continue;
                    var g = m >= f ? m + 1 : b;
                    g <= f && (g = Math.min(d, f + 1)), i.push({ start: f, end: g, text: r.slice(f, g) }), f = g, b = f - 1, m = -1;
                }
                f <= d && i.push({ start: f, end: d, text: r.slice(f, d) });
            }
            o = s + 1;
        }
        return i;
    }
    function ue(t, e) { return e <= 0 ? [] : t.length <= e ? t : t.slice(0, e); }
    function Ee(t) { var e = t.fullText, n = t.lines, r = t.localX, i = t.localY, o = t.lineHeight, s = t.measure; if (n.length === 0)
        return 0; var l = Math.max(0, r), a = Math.max(0, i), d = Math.max(1, o), f = Math.max(0, Math.min(n.length - 1, Math.floor(a / d))), m = n[f], b = m.start, c = Number.POSITIVE_INFINITY; for (var M = m.start; M <= m.end; M++) {
        var g = s(e.slice(m.start, M)), y = Math.abs(g - l);
        y < c && (c = y, b = M);
    } return b; }
    function tr(t) { var M, g, y, E; var e = t.node, n = t.graphics, r = t.w, i = t.h, o = t.theme, s = Math.max(0, Math.round(r)), l = Math.max(0, Math.round(i)); n.rect(.5, .5, Math.max(0, s - 1), Math.max(0, l - 1)), n.fill(o.control.progress.background), n.stroke({ width: 1, color: o.control.progress.border }); var a = Number((g = (M = e.attrs) == null ? void 0 : M.value) != null ? g : "0"), d = Number((E = (y = e.attrs) == null ? void 0 : y.max) != null ? E : "1"), f = d > 0 ? Math.max(0, Math.min(1, a / d)) : 0, m = 3, b = Math.max(0, s - m * 2), c = Math.max(0, l - m * 2); n.rect(m, m, Math.max(0, b * f), c), n.fill(o.control.progress.fill); }
    function er(t, e) { t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0), t.setHeight(14), t.setMinWidth(240); }
    function ye(t, e, n) { var l; var r = t.get(e); if (r)
        return r; var i = Number((l = n == null ? void 0 : n.value) != null ? l : "0"), o = Number.isFinite(i) ? i : 0, s = { value: Math.max(0, Math.min(1, o)) }; return t.set(e, s), s; }
    function nr(t, e) { t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0), t.setHeight(14), t.setMinWidth(240); }
    function rr(t) { var e = t.node, n = t.Yoga, r = t.measurer, i = n.Node.create(); return i.setPadding(n.EDGE_LEFT, 0), i.setPadding(n.EDGE_RIGHT, 0), i.setPadding(n.EDGE_TOP, 0), i.setPadding(n.EDGE_BOTTOM, 0), i.setMargin(n.EDGE_RIGHT, 6), i.setMeasureFunc(function () { var o = r.measure("100"); return { width: o.width, height: o.height }; }), { yogaNode: i, buildBox: function () { return ({ kind: "block", key: e.key, tagName: e.tagName, attrs: e.attrs, x: i.getComputedLeft(), y: i.getComputedTop(), width: i.getComputedWidth(), height: i.getComputedHeight(), children: [] }); } }; }
    function ir(t) { var d, f; var e = t.node, n = t.container, r = t.theme, i = t.sliderStates, o = (d = e.attrs) == null ? void 0 : d["data-slider-key"], s = null; if (o) {
        var m = i.get(o);
        if (m)
            s = m;
        else {
            var b = (f = e.attrs) == null ? void 0 : f["data-slider-init"];
            s = ye(i, o, b != null ? { value: String(b) } : void 0);
        }
    } var l = s ? Math.round(s.value * 100) : 0, a = It(n, "__pct", function (m) { m.style = { fontFamily: r.fontFamily, fontSize: r.fontSize, fill: r.text, fontWeight: "400", wordWrap: !1 }; }); a.text = String(l), a.position.set(0, xt); }
    function Ze(t) { var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.absX, l = t.absY, a = t.theme, d = t.sliderStates, f = t.sliderBounds, m = t.sliderDrags, b = t.requestPaint, c = e.key, M = c ? ye(d, c, e.attrs) : null, g = Math.max(0, Math.round(i)), y = Math.max(0, Math.round(o)), E = 3; c && f.set(c, { x: s, y: l, w: g, h: y, innerPad: E }), r.rect(.5, .5, Math.max(0, g - 1), Math.max(0, y - 1)), r.fill(a.control.progress.background), r.stroke({ width: 1, color: a.control.progress.border }); var _ = M ? Math.max(0, Math.min(1, M.value)) : 0, w = Math.max(0, g - E * 2), G = Math.max(0, y - E * 2); r.rect(E, E, Math.max(0, w * _), G), r.fill(a.control.progress.fill); var A = E + w * _, $ = G / 2; r.moveTo(A, E - $), r.lineTo(A, E + G + $), r.stroke({ width: 2, color: a.text }), c && ($t(n), n.eventMode = "static", n.cursor = "pointer", n.hitArea = new gt(0, 0, Math.max(0, g), Math.max(0, y)), n.on("pointerdown", function (v) {
        var e_5, _a;
        var B, q, Z, tt, Y, j;
        if ((v == null ? void 0 : v.button) === 2)
            return;
        var k = t.getPointerId ? t.getPointerId(v) : Number((Z = (q = v == null ? void 0 : v.pointerId) != null ? q : (B = v == null ? void 0 : v.data) == null ? void 0 : B.pointerId) != null ? Z : 0);
        if (k <= 0)
            return;
        try {
            for (var _b = __values(m.entries()), _c = _b.next(); !_c.done; _c = _b.next()) {
                var _d = __read(_c.value, 2), N = _d[0], p = _d[1];
                p.key === c && N !== k && m.delete(N);
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
        var T = f.get(c), H = (Y = (tt = v.global) == null ? void 0 : tt.x) != null ? Y : 0, U = T ? H - T.x : 0, L = T ? Math.max(1, T.w - T.innerPad * 2) : 1, h = (U - ((j = T == null ? void 0 : T.innerPad) != null ? j : 0)) / L, I = ye(d, c, e.attrs);
        I.value = Math.max(0, Math.min(1, h)), b == null || b();
    })); }
    function or(t) { var G; var e = t.node, n = t.container, r = t.w, i = t.h, o = t.theme, s = t.detailsOpen, l = t.requestRerender, a = (G = e.attrs) == null ? void 0 : G["data-details-key"], d = e.attrs ? Object.prototype.hasOwnProperty.call(e.attrs, "data-details-open") : !1, f = a && s.has(a) ? s.get(a) === !0 : d, m = function (A) { var k; if (!a || (A == null ? void 0 : A.button) === 2)
        return; var v = !(s.has(a) ? s.get(a) === !0 : d); s.set(a, v), l == null || l(), (k = A == null ? void 0 : A.stopPropagation) == null || k.call(A); }, b = 16, c = St(n, "__arrow"); Et(c); var M = 2, g = 3, y = g, E = g, _ = b - g, w = b - g; f ? (c.moveTo(y, E), c.lineTo((y + _) / 2, w), c.lineTo(_, E)) : (c.moveTo(y, E), c.lineTo(_, (E + w) / 2), c.lineTo(y, w)), c.stroke({ width: M, color: o.text }), c.position.set(4, Math.max(0, (i - b) / 2)), c.eventMode = "static", c.cursor = "pointer", c.hitArea = new gt(0, 0, b + 8, b + 8), c.on("pointerdown", m), a && ($t(n), n.eventMode = "static", n.cursor = "pointer", n.hitArea = new gt(0, 0, Math.max(0, r), Math.max(0, i)), n.on("pointerdown", m)); }
    function sr(t, e) { t.setFlexDirection(e.FLEX_DIRECTION_ROW), t.setAlignItems(e.ALIGN_CENTER), t.setJustifyContent(e.JUSTIFY_SPACE_BETWEEN), t.setPadding(e.EDGE_TOP, 6), t.setPadding(e.EDGE_BOTTOM, 6), t.setPadding(e.EDGE_LEFT, 26), t.setPadding(e.EDGE_RIGHT, 12), t.setMinHeight(36); }
    function ar(t, e) { t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0); }
    function lr(t, e) { var i, o, s; if (!t || t.tagName !== "details" || !t.key)
        return (i = t == null ? void 0 : t.children) != null ? i : []; var n = t.attrs ? Object.prototype.hasOwnProperty.call(t.attrs, "open") : !1; return (e.has(t.key) ? e.get(t.key) === !0 : n) ? (o = t.children) != null ? o : [] : ((s = t.children) != null ? s : []).filter(function (l) { return l && l.kind === "block" && l.tagName === "summary"; }); }
    function cr(t) { var e = t.graphics, n = t.w, r = t.theme; e.rect(0, 0, Math.round(n), 1), e.fill(r.hr); }
    function ur(t, e) { t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0), t.setMargin(e.EDGE_TOP, 2), t.setMargin(e.EDGE_BOTTOM, 2), t.setHeight(1); }
    function dr(t) { var y, E; var e = t.container, n = t.graphics, r = t.w, i = t.h, o = t.label, s = t.theme, l = t.registerHoverHandlers, a = function (_) { n.clear(); var w = 1, G = w / 2; s.control.button.radius > 0 ? n.roundRect(G, G, Math.max(0, r - w), Math.max(0, i - w), s.control.button.radius) : n.rect(G, G, Math.max(0, r - w), Math.max(0, i - w)), n.fill(_), n.stroke({ width: w, color: s.control.button.border }); }; a(s.control.button.fill); var d = It(e, "__label", function (_) { _.style = { fontFamily: s.fontFamily, fontSize: s.fontSize, fill: s.control.button.text, fontWeight: "400", wordWrap: !1, wordWrapWidth: 0 }; }), f = String(o != null ? o : "").trim(); d.text = f, d.visible = f.length > 0, d.style = ge(re({}, d.style), { fontFamily: s.fontFamily, fontSize: s.fontSize, fill: s.control.button.text, wordWrap: !1, wordWrapWidth: Math.max(0, Math.ceil(r - 16)) }); var m = Number((y = d.width) != null ? y : 0), b = Number((E = d.height) != null ? E : 0), c = s.fontSize * 1.25; d.position.set(m > 0 ? Math.max(8, Math.floor((r - m) / 2)) : 8, Math.max(0, Math.floor((i - (b > 0 ? b : c)) / 2)) + xt); var M = function () { return a(s.control.button.hoverFill); }, g = function () { return a(s.control.button.fill); }; l == null || l({ over: M, out: g }), $t(e), e.eventMode = "static", e.cursor = "pointer", e.on("pointerover", M), e.on("pointerout", g), e.on("pointerdown", function () { return a(s.control.button.activeFill); }), e.on("pointerup", function () { return a(s.control.button.hoverFill); }); }
    function hr(t, e) { t.setFlexDirection(e.FLEX_DIRECTION_ROW), t.setPadding(e.EDGE_TOP, 6), t.setPadding(e.EDGE_BOTTOM, 6), t.setMinHeight(36), t.setMinWidth(100), t.setAlignItems(e.ALIGN_CENTER), t.setJustifyContent(e.JUSTIFY_CENTER); }
    function mr(t) { var e = t.graphics, n = t.w, r = t.h, i = t.boxBorder, o = Math.max(0, Math.round(n)), s = Math.max(0, Math.round(r)); e.rect(0, 0, o, s), e.stroke({ width: 1, color: i, alignment: 0 }); }
    function fr(t) { var e = t.nodeTag, n = t.graphics, r = t.w, i = t.h, o = t.theme; e === "th" && (n.rect(0, 0, r, i), n.fill(o.control.table.headerFill)), n.rect(0, 0, r, i), n.stroke({ width: 1, color: o.control.table.cellBorder }); }
    function pr(t, e) { t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0); }
    function gr(t, e) { t.setFlexDirection(e.FLEX_DIRECTION_ROW), t.setFlexWrap(e.WRAP_NO_WRAP), t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0), t.setMargin(e.EDGE_BOTTOM, 0); }
    function br(t, e) { t.setFlexGrow(1), t.setFlexShrink(1), t.setMinWidth(80), t.setPadding(e.EDGE_LEFT, 8), t.setPadding(e.EDGE_RIGHT, 8), t.setPadding(e.EDGE_TOP, 6), t.setPadding(e.EDGE_BOTTOM, 6), t.setMargin(e.EDGE_BOTTOM, 0); }
    function En(t) { var e = String(t != null ? t : "").toLowerCase(); if (e.length !== 2 || e.charAt(0) !== "h")
        return !1; var n = e.charCodeAt(1); return n >= 49 && n <= 54; }
    function xr(t, e) { t.setPadding(e.EDGE_TOP, 6), t.setPadding(e.EDGE_BOTTOM, 6), t.setMinHeight(36), t.setJustifyContent(e.JUSTIFY_CENTER); }
    function yr(t, e) {
        var n = Math.max(1, Math.floor(t)), r = Math.max(1, Math.floor(e));
        return "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<svg viewBox=\"0 0 ".concat(n, " ").concat(r, "\" xmlns=\"http://www.w3.org/2000/svg\">\n  <rect x=\"0\" y=\"0\" width=\"").concat(n, "\" height=\"").concat(r, "\" fill=\"#f6f6f6\"/>\n  <rect x=\"0.5\" y=\"0.5\" width=\"").concat(Math.max(0, n - 1), "\" height=\"").concat(Math.max(0, r - 1), "\" fill=\"none\" stroke=\"#999\"/>\n  <path d=\"M2 2 L").concat(Math.max(2, n - 2), " ").concat(Math.max(2, r - 2), "\" stroke=\"#c8c8c8\"/>\n  <path d=\"M").concat(Math.max(2, n - 2), " 2 L2 ").concat(Math.max(2, r - 2), "\" stroke=\"#c8c8c8\"/>\n</svg>");
    }
    function wr(_a) {
        var _b = _a === void 0 ? {} : _a, _c = _b.ring, t = _c === void 0 ? 34 : _c, _d = _b.core, e = _d === void 0 ? 14 : _d, _f = _b.hueA, n = _f === void 0 ? "#00e5ff" : _f, _g = _b.hueB, r = _g === void 0 ? "#ff2bd6" : _g;
        var i = Math.max(0, t - 10), o = Math.max(0, e * .35);
        return "\n<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 100 100\">\n  <rect width=\"100\" height=\"100\" fill=\"#ffffff\"/>\n  <rect width=\"100\" height=\"100\" fill=\"".concat(n, "\" opacity=\"0.08\"/>\n\n  <circle cx=\"50\" cy=\"50\" r=\"").concat(t, "\" fill=\"none\" stroke=\"").concat(r, "\" stroke-width=\"4\" opacity=\"0.95\"/>\n  <circle cx=\"50\" cy=\"50\" r=\"").concat(i, "\" fill=\"none\" stroke=\"").concat(n, "\" stroke-width=\"1\" opacity=\"0.35\"/>\n\n  <circle cx=\"50\" cy=\"50\" r=\"").concat(e, "\" fill=\"").concat(n, "\" opacity=\"0.9\"/>\n  <circle cx=\"43\" cy=\"43\" r=\"").concat(o, "\" fill=\"#ffffff\" opacity=\"0.55\"/>\n\n  <path d=\"M50 16 L52 22 L58 24 L52 26 L50 32 L48 26 L42 24 L48 22 Z\" fill=\"#ffffff\" opacity=\"0.85\"/>\n  <path d=\"M82 52 L85 56 L90 57 L85 58 L82 62 L79 58 L74 57 L79 56 Z\" fill=\"#ffffff\" opacity=\"0.70\"/>\n  <path d=\"M20 70 L22 74 L27 75 L22 76 L20 80 L18 76 L13 75 L18 74 Z\" fill=\"#ffffff\" opacity=\"0.65\"/>\n</svg>\n");
    }
    var _r = new Map;
    function Ae() { var t = globalThis; return !0; }
    function Er(t) { var e = globalThis, n = String(t != null ? t : "").trim(); if (!n)
        return null; var r = e.__TRUEOS_READY_IMAGE_TEXTURE_CACHE__; (!r || typeof r.get != "function" || typeof r.set != "function") && (r = new Map, e.__TRUEOS_READY_IMAGE_TEXTURE_CACHE__ = r); var i = r.get(n); if (i)
        return i; var o = e.__trueosResolveReadyImageTexture; if (typeof o != "function")
        return null; var s = o(n); if (s && typeof s.then == "function") {
        var l = s.then(function (a) { return (r.set(n, a), a); }).catch(function (a) { throw r.delete(n), a; });
        return r.set(n, l), l;
    } return r.set(n, s), s; }
    function Wi(t) { var s; var e = globalThis, n = String(t != null ? t : "").trim(); if (!n)
        return null; var r = e.__trueosPeekReadyImageTexture; if (typeof r != "function")
        return null; var i = r(n); return !i || typeof i.then == "function" ? null : Math.max(0, Number((s = i == null ? void 0 : i.texId) != null ? s : 0) | 0) > 0 ? i : null; }
    function Hi(t, e) { var n = globalThis, r = String(t != null ? t : "").trim(); if (!r || !e)
        return; var i = n.__TRUEOS_READY_IMAGE_TEXTURE_CACHE__; (!i || typeof i.get != "function" || typeof i.set != "function") && (i = new Map, n.__TRUEOS_READY_IMAGE_TEXTURE_CACHE__ = i), i.set(r, e); }
    function kr(t, e) { var r, i, o, s, l; var n = Math.max(0, Number((r = e == null ? void 0 : e.texId) != null ? r : 0) | 0); if (n <= 0)
        throw new Error("image texture missing"); t.state = "ready", t.texId = n, t.width = Math.max(0, Number((o = (i = e == null ? void 0 : e.pixelWidth) != null ? i : e == null ? void 0 : e.width) != null ? o : 0) | 0), t.height = Math.max(0, Number((l = (s = e == null ? void 0 : e.pixelHeight) != null ? s : e == null ? void 0 : e.height) != null ? l : 0) | 0); }
    function $i(t, e) { var n = Wi(t) || Er(t); return !n || typeof n.then == "function" ? !1 : (kr(e, n), Hi(t, n), !0); }
    function Tr(t, e) { var n = String(t != null ? t : "").trim(); if (!n)
        return null; var r = _r.get(n); if (r) {
        if (Ae() && r.state === "loading")
            try {
                $i(n, r);
            }
            catch (l) {
                r.state = "error";
            }
        return r;
    } if (Ae())
        return null; var i = { state: "loading", texId: 0, width: 0, height: 0 }; _r.set(n, i); var o = function (l) { kr(i, l), Ae() || e == null || e(); }, s = function () { i.state = "error", Ae() || e == null || e(); }; try {
        var l = Er(n);
        if (!l)
            return i;
        if (l && typeof l.then == "function") {
            if (Ae())
                return i;
            l.then(o).catch(s);
        }
        else
            o(l);
    }
    catch (l) {
        s();
    } return i; }
    function Ui(t) { var e = String(t != null ? t : ""); if (!e.startsWith("data:image/svg+xml"))
        return null; var n = e.indexOf(","); if (n === -1)
        return null; var r = e.slice(0, n).toLowerCase(), i = e.slice(n + 1); try {
        return r.includes(";base64") ? atob(i) : decodeURIComponent(i);
    }
    catch (o) {
        return null;
    } }
    function Xi(t) { return Mr(Mr(String(t), "tspan"), "text"); }
    function Yi(t) { return "data:image/svg+xml;charset=utf-8,".concat(encodeURIComponent(t)); }
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
    function Sr(t) { var G, A, $, v; var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.theme, l = t.requestRerender, a = (A = (G = e.attrs) == null ? void 0 : G.alt) != null ? A : "", d = (v = ($ = e.attrs) == null ? void 0 : $.src) != null ? v : "", f = d.trim().length > 0, m = a.trim().length > 0 ? a : d.trim().length > 0 ? d : "img", b = r.image, c = f ? Tr(d, l) : null; if ((c == null ? void 0 : c.state) === "ready" && c.texId > 0 && typeof b == "function") {
        b.call(r, c.texId, 0, 0, Math.max(0, i), Math.max(0, o));
        var k = n.getChildByLabel ? n.getChildByLabel("__label") : n.children.find(function (T) { return (T == null ? void 0 : T.label) === "__label"; });
        k && (k.visible = !1);
        return;
    } var M = f ? Ui(d) : null, g = Xi(M != null ? M : f ? yr(i, o) : wr({ ring: 34, core: 14 })), y = St(n, "__svg"), E = Tr(Yi(g), l); if ((E == null ? void 0 : E.state) === "ready" && E.texId > 0 && typeof y.image == "function") {
        var k = "texture:".concat(E.texId, ":").concat(Math.round(i), "x").concat(Math.round(o));
        if (y.__key !== k && (Et(y), y.image(E.texId, 0, 0, Math.max(0, i), Math.max(0, o)), y.__key = k), y.scale.set(1), y.position.set(0, 0), !f) {
            var T = n.getChildByLabel ? n.getChildByLabel("__label") : n.children.find(function (H) { return (H == null ? void 0 : H.label) === "__label"; });
            T && (T.visible = !1);
            return;
        }
        if (m.trim().length > 0) {
            var T = It(n, "__label", function (H) { H.style = { fontFamily: s.fontFamily, fontSize: s.fontSize, fill: s.mutedText, fontWeight: "400", wordWrap: !0, wordWrapWidth: 0 }; });
            T.text = m, T.style.wordWrapWidth = Math.max(0, Math.ceil(i - 16) + Me), T.position.set(8, 8 + xt), T.visible = !0;
        }
        return;
    }
    else
        Et(y); var _ = y.svg; if (0 && y.__key !== k)
        try { }
        catch (H) { } r.rect(.5, .5, Math.max(0, i - 1), Math.max(0, o - 1)), r.fill(16185078), r.stroke({ width: 1, color: s.control.border }); var w = It(n, "__label", function (k) { k.style = { fontFamily: s.fontFamily, fontSize: s.fontSize, fill: s.mutedText, fontWeight: "400", wordWrap: !0, wordWrapWidth: 0 }; }); w.text = m, w.style.wordWrapWidth = Math.max(0, Math.ceil(i - 16) + Me), w.position.set(8, 8 + xt); }
    function Ir(t, e, n) { var d, f, m, b; t.setPadding(n.EDGE_LEFT, 0), t.setPadding(n.EDGE_RIGHT, 0), t.setPadding(n.EDGE_TOP, 0), t.setPadding(n.EDGE_BOTTOM, 0); var r = Number((f = (d = e.attrs) == null ? void 0 : d.width) != null ? f : "0"), i = Number((b = (m = e.attrs) == null ? void 0 : m.height) != null ? b : "0"), o = Number.isFinite(r) && r > 0, s = Number.isFinite(i) && i > 0, l = o ? r : 240, a = s ? i : 140; (o || s) && (t.setAlignSelf(n.ALIGN_FLEX_START), t.setFlexGrow(0), t.setFlexShrink(0)), t.setWidth(l), t.setHeight(a), t.setMinWidth(120), t.setMinHeight(80); }
    var Pr = new Map;
    function Ne() { var t = globalThis; return !0; }
    function Or(t) { var e = globalThis, n = String(t != null ? t : "").trim(); if (!n)
        return null; var r = e.__TRUEOS_READY_IMAGE_TEXTURE_CACHE__; (!r || typeof r.get != "function" || typeof r.set != "function") && (r = new Map, e.__TRUEOS_READY_IMAGE_TEXTURE_CACHE__ = r); var i = r.get(n); if (i)
        return i; var o = e.__trueosResolveReadyImageTexture; if (typeof o != "function")
        return null; var s = o(n); if (s && typeof s.then == "function") {
        var l = s.then(function (a) { return (r.set(n, a), a); }).catch(function (a) { throw r.delete(n), a; });
        return r.set(n, l), l;
    } return r.set(n, s), s; }
    function Ki(t) { var s; var e = globalThis, n = String(t != null ? t : "").trim(); if (!n)
        return null; var r = e.__trueosPeekReadyImageTexture; if (typeof r != "function")
        return null; var i = r(n); return !i || typeof i.then == "function" ? null : Math.max(0, Number((s = i == null ? void 0 : i.texId) != null ? s : 0) | 0) > 0 ? i : null; }
    function zi(t, e) { var n = globalThis, r = String(t != null ? t : "").trim(); if (!r || !e)
        return; var i = n.__TRUEOS_READY_IMAGE_TEXTURE_CACHE__; (!i || typeof i.get != "function" || typeof i.set != "function") && (i = new Map, n.__TRUEOS_READY_IMAGE_TEXTURE_CACHE__ = i), i.set(r, e); }
    function Rr(t, e) { var r, i, o, s, l; var n = Math.max(0, Number((r = e == null ? void 0 : e.texId) != null ? r : 0) | 0); if (n <= 0)
        throw new Error("svg texture missing"); t.state = "ready", t.texId = n, t.width = Math.max(0, Number((o = (i = e == null ? void 0 : e.pixelWidth) != null ? i : e == null ? void 0 : e.width) != null ? o : 0) | 0), t.height = Math.max(0, Number((l = (s = e == null ? void 0 : e.pixelHeight) != null ? s : e == null ? void 0 : e.height) != null ? l : 0) | 0); }
    function ji(t, e) { var n = Ki(t) || Or(t); return !n || typeof n.then == "function" ? !1 : (Rr(e, n), zi(t, n), !0); }
    function Vi(t) { return Cr(Cr(String(t), "tspan"), "text"); }
    function Cr(t, e) { var n = "", r = 0, i = t.toLowerCase(), o = "<" + e, s = "</" + e; for (; r < t.length;) {
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
    function Dr(t) { var e = String(t), r = e.toLowerCase().indexOf("viewbox"); if (r < 0)
        return null; var i = e.indexOf("=", r + 7); if (i < 0)
        return null; var o = i + 1; for (; o < e.length;) {
        var c = e.charCodeAt(o);
        if (c !== 32 && c !== 9 && c !== 10 && c !== 13 && c !== 12)
            break;
        o += 1;
    } var s = e.charAt(o); if (s !== '"' && s !== "'")
        return null; var l = e.indexOf(s, o + 1); if (l < 0)
        return null; var a = Ji(e.slice(o + 1, l)); if (a.length < 4)
        return null; var d = Number(a[0]), f = Number(a[1]), m = Number(a[2]), b = Number(a[3]); return ![d, f, m, b].every(function (c) { return Number.isFinite(c); }) || m <= 0 || b <= 0 ? null : { minX: d, minY: f, w: m, h: b }; }
    function Ji(t) { var e = [], n = ""; for (var r = 0; r < t.length; r += 1) {
        var i = t.charCodeAt(r);
        i === 32 || i === 9 || i === 10 || i === 13 || i === 12 ? n.length > 0 && (e.push(n), n = "") : n += t.charAt(r);
    } return n.length > 0 && e.push(n), e; }
    function Zi(t, e) { var n = String(t != null ? t : ""); if (!n.trim())
        return null; var r = Pr.get(n), i = "data:image/svg+xml;charset=utf-8,".concat(encodeURIComponent(n)); if (r) {
        if (Ne() && r.state === "loading")
            try {
                ji(i, r);
            }
            catch (a) {
                r.state = "error";
            }
        return r;
    } if (Ne())
        return null; var o = { state: "loading", texId: 0, width: 0, height: 0 }; Pr.set(n, o); var s = function (a) { Rr(o, a), Ne() || e == null || e(); }, l = function () { o.state = "error", Ne() || e == null || e(); }; try {
        var a = Or(i);
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
    function Qi(t, e, n) { var r = Math.max(0, e), i = Math.max(0, n), o = Dr(t); if (!o || r <= 0 || i <= 0)
        return { x: 0, y: 0, w: r, h: i }; var s = r / o.w, l = i / o.h, a = Math.min(s, l), d = Math.max(0, o.w * a), f = Math.max(0, o.h * a); return { x: Math.max(0, (r - d) / 2), y: Math.max(0, (i - f) / 2), w: d, h: f }; }
    function Ar(t, e, n) { var d, f, m, b; t.setPadding(n.EDGE_LEFT, 0), t.setPadding(n.EDGE_RIGHT, 0), t.setPadding(n.EDGE_TOP, 0), t.setPadding(n.EDGE_BOTTOM, 0); var r = Number((f = (d = e.attrs) == null ? void 0 : d.width) != null ? f : "0"), i = Number((b = (m = e.attrs) == null ? void 0 : m.height) != null ? b : "0"), o = Number.isFinite(r) && r > 0, s = Number.isFinite(i) && i > 0, l = o ? r : 300, a = s ? i : 150; (o || s) && (t.setAlignSelf(n.ALIGN_FLEX_START), t.setFlexGrow(0), t.setFlexShrink(0)), t.setWidth(l), t.setHeight(a), t.setMinWidth(Math.min(120, l)), t.setMinHeight(Math.min(80, a)); }
    function Nr(t) { var e = t.svgMarkup, n = t.container, r = t.w, i = t.h, o = t.requestRerender, s = Vi(e), l = St(n, "__svg"), a = l.__svgString, d = l.__w, f = l.__h, m = a !== s, b = Zi(s, o); if (l.scale.set(1), l.position.set(0, 0), (b == null ? void 0 : b.state) === "ready" && b.texId > 0 && typeof l.image == "function") {
        if (m || d !== r || f !== i || l.__texId !== b.texId) {
            var M = Qi(s, r, i);
            Et(l), l.image(b.texId, M.x, M.y, M.w, M.h), l.__svgString = s, l.__w = r, l.__h = i, l.__texId = b.texId;
        }
        return;
    } Et(l); return; if (typeof c == "function") {
        if (m || d !== r || f !== i) {
            Et(l);
            var g = void 0;
            try {
                g = c.call(l, s);
            }
            catch (y) {
                g = null;
            }
            g && typeof g.then == "function" && g.then(function () { return o == null ? void 0 : o(); }).catch(function () { }), l.__svgString = s, l.__w = r, l.__h = i;
        }
        var M = Dr(s);
        if (M) {
            var g = r / M.w, y = i / M.h, E = Math.min(g, y), _ = M.w * E, w = M.h * E;
            l.scale.set(E), l.position.set(-M.minX * E + (r - _) / 2, -M.minY * E + (i - w) / 2);
        }
        return;
    } }
    function vr(t, e, n) { var d, f, m, b; t.setPadding(n.EDGE_LEFT, 0), t.setPadding(n.EDGE_RIGHT, 0), t.setPadding(n.EDGE_TOP, 0), t.setPadding(n.EDGE_BOTTOM, 0); var r = Number((f = (d = e.attrs) == null ? void 0 : d.width) != null ? f : "0"), i = Number((b = (m = e.attrs) == null ? void 0 : m.height) != null ? b : "0"), o = Number.isFinite(r) && r > 0, s = Number.isFinite(i) && i > 0, l = o ? r : 300, a = s ? i : 150; (o || s) && (t.setAlignSelf(n.ALIGN_FLEX_START), t.setFlexGrow(0), t.setFlexShrink(0)), t.setWidth(l), t.setHeight(a), t.setMinWidth(Math.min(120, l)), t.setMinHeight(Math.min(80, a)); }
    function Gr(t) { var e = t.graphics, n = t.container, r = t.w, i = t.h, o = t.theme, s = 1, l = s / 2; e.rect(l, l, Math.max(0, r - s), Math.max(0, i - s)), e.fill(16777215), e.stroke({ width: s, color: o.control.border, alignment: 0 }), e.moveTo(6, i - 6), e.lineTo(r - 6, 6), e.stroke({ width: 1, color: 0, alpha: .1 }); var a = jt({ text: "canvas", fontFamily: o.fontFamily, fontSize: Math.max(10, Math.floor(o.fontSize * .85)), fill: o.mutedText, wordWrap: !1 }); a.position.set(8, 8 + xt), n.addChild(a); }
    function Lr(t, e, n) { var f, m, b, c, M, g; var r = String((m = (f = e.attrs) == null ? void 0 : f["data-root"]) != null ? m : "") === "1"; if (t.setFlexDirection(n.FLEX_DIRECTION_COLUMN), t.setAlignItems(n.ALIGN_STRETCH), r) {
        t.setPadding(n.EDGE_LEFT, 0), t.setPadding(n.EDGE_RIGHT, 0), t.setPadding(n.EDGE_TOP, 0), t.setPadding(n.EDGE_BOTTOM, 0), t.setAlignSelf(n.ALIGN_STRETCH), t.setFlexGrow(1), t.setFlexShrink(1), t.setMinWidth(0), t.setMinHeight(0);
        return;
    } t.setPadding(n.EDGE_LEFT, 8), t.setPadding(n.EDGE_RIGHT, 8), t.setPadding(n.EDGE_BOTTOM, 8), t.setPadding(n.EDGE_TOP, 34); var i = Number((c = (b = e.attrs) == null ? void 0 : b.width) != null ? c : "0"), o = Number((g = (M = e.attrs) == null ? void 0 : M.height) != null ? g : "0"), s = Number.isFinite(i) && i > 0, l = Number.isFinite(o) && o > 0, a = s ? i : 420, d = l ? o : 240; (s || l) && (t.setAlignSelf(n.ALIGN_FLEX_START), t.setFlexGrow(0), t.setFlexShrink(0)), t.setWidth(a), t.setHeight(d), t.setMinWidth(Math.min(200, a)), t.setMinHeight(Math.min(160, d)); }
    function Fr(t) { var c, M, g, y; var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.theme; if (String((M = (c = e.attrs) == null ? void 0 : c["data-root"]) != null ? M : "") === "1")
        return; var a = 1, d = a / 2; r.rect(d, d, Math.max(0, i - a), Math.max(0, o - a)), r.fill(16777215), r.stroke({ width: a, color: s.control.border, alignment: 0 }), r.rect(d, d, Math.max(0, i - a), 26), r.fill({ color: 0, alpha: .04 }); var m = String((y = (g = e.attrs) == null ? void 0 : g.srcdoc) != null ? y : "").trim().length > 0 ? "srcdoc" : "empty", b = It(n, "__title", function (E) { E.style = { fontFamily: s.fontFamily, fontSize: Math.max(10, Math.floor(s.fontSize * .85)), fill: s.mutedText, fontWeight: "400", wordWrap: !1 }; }); b.text = "iframe (".concat(m, ")"), b.position.set(8, 6 + xt), n.eventMode = "static", n.cursor = "default", n.hitArea = new gt(0, 0, Math.max(0, i), Math.max(0, o)); }
    function Br(t, e, n) { var i, o; var r = ((o = (i = e.attrs) == null ? void 0 : i.type) != null ? o : "text").toLowerCase(); r === "checkbox" || r === "radio" ? (t.setPadding(n.EDGE_LEFT, 0), t.setPadding(n.EDGE_RIGHT, 0), t.setPadding(n.EDGE_TOP, 0), t.setPadding(n.EDGE_BOTTOM, 0), t.setWidth(16), t.setHeight(16), t.setMinWidth(16), t.setMargin(n.EDGE_RIGHT, 6)) : (t.setPadding(n.EDGE_TOP, 6), t.setPadding(n.EDGE_BOTTOM, 6), t.setHeight(36), t.setMinHeight(36), t.setMinWidth(220)); }
    function Wr(t) {
        var e_6, _a, e_7, _b;
        var U, L, h, I, B, q, Z, tt, Y, j;
        var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.absX, l = t.absY, a = t.theme, d = t.textMeasure, f = t.uiState, m = t.getOrInitInputState, b = t.clamp, c = t.radioGroups, M = t.textDrags, g = t.requestPaint, y = ((L = (U = e.attrs) == null ? void 0 : U.type) != null ? L : "text").toLowerCase(), E = e.key, _ = E ? m(E, e.attrs) : void 0, w = (h = t.showCaret) != null ? h : !1, G = (I = t.caretPointerId) != null ? I : null, A = t.focusColor, $ = t.getCursorColor;
        n.sortableChildren = !0;
        try {
            for (var _c = __values(n.children), _d = _c.next(); !_d.done; _d = _c.next()) {
                var N = _d.value;
                var p = N.label;
                p && (p.startsWith("__sel:") || p === "__caret") && (N.visible = !1);
            }
        }
        catch (e_6_1) { e_6 = { error: e_6_1 }; }
        finally {
            try {
                if (_d && !_d.done && (_a = _c.return)) _a.call(_c);
            }
            finally { if (e_6) throw e_6.error; }
        }
        var v = 8, k = 6 + xt, T = 5, H = a.fontSize * 1.25;
        if (y === "checkbox")
            r.rect(.5, .5, Math.max(0, i - 1), Math.max(0, o - 1)), r.fill(a.control.background), r.stroke({ width: 1, color: a.control.border }), _ != null && _.indeterminate ? (r.moveTo(4, 4), r.lineTo(Math.max(4, i - 4), Math.max(4, o - 4)), r.stroke({ width: 2, color: a.control.accent }), r.moveTo(Math.max(4, i - 4), 4), r.lineTo(4, Math.max(4, o - 4)), r.stroke({ width: 2, color: a.control.accent })) : _ != null && _.checked && (r.rect(3, 3, Math.max(0, i - 3 * 2), Math.max(0, o - 3 * 2)), r.fill(a.control.accent));
        else if (y === "radio") {
            {
                var P = Math.max(0, Math.min(i, o) / 2 - .5);
                r.circle(i / 2, o / 2, P), r.fill(a.control.background), r.stroke({ width: 1, color: a.control.border });
            }
            if (_ != null && _.checked) {
                var N = Math.max(0, Math.min(i, o) / 2 - 4.5);
                r.circle(i / 2, o / 2, N), r.fill(a.control.accent);
            }
        }
        else {
            var N = A != null ? 2 : 1, p = N / 2;
            a.control.radius > 0 ? r.roundRect(p, p, Math.max(0, i - N), Math.max(0, o - N), a.control.radius) : r.rect(p, p, Math.max(0, i - N), Math.max(0, o - N)), r.fill(a.control.background), r.stroke({ width: N, color: A != null ? A : a.control.border });
            var P = y === "password" ? "\u2022".repeat(((B = _ == null ? void 0 : _.value) != null ? B : "").length) : (q = _ == null ? void 0 : _.value) != null ? q : "", O = Math.max(0, i - v * 2);
            E && f.fieldBounds.set(E, { x: s, y: l, w: i, h: o, innerLeft: v, innerTop: k, innerWidth: O, maxLines: T, isPassword: y === "password" });
            var C = ce(P, O, d), x = ue(C, T), S = x.length > 0 ? x[x.length - 1].end : 0;
            if (E && _ && typeof _.value == "string") {
                var W = _.selections;
                if (W && W.size > 0)
                    try {
                        for (var _f = __values(W.entries()), _g = _f.next(); !_g.done; _g = _f.next()) {
                            var _h = __read(_g.value, 2), X = _h[0], z = _h[1];
                            var K = b((Z = z.start) != null ? Z : 0, 0, P.length), V = b((tt = z.end) != null ? tt : K, 0, P.length), nt = b(Math.min(K, V), 0, S), Q = b(Math.max(K, V), 0, S);
                            if (nt === Q)
                                continue;
                            var it = St(n, "__sel:".concat(X));
                            Et(it), it.zIndex = 0, it.visible = !0;
                            for (var st = 0; st < x.length; st++) {
                                var dt = x[st], yt = Math.max(nt, dt.start), Rt = Math.min(Q, dt.end);
                                if (yt >= Rt)
                                    continue;
                                var Gt = v + d(P.slice(dt.start, yt)), at = d(P.slice(yt, Rt));
                                it.rect(Gt, k + st * H, at, H);
                            }
                            it.fill({ color: $(X), alpha: .22 });
                        }
                    }
                    catch (e_7_1) { e_7 = { error: e_7_1 }; }
                    finally {
                        try {
                            if (_g && !_g.done && (_b = _f.return)) _b.call(_f);
                        }
                        finally { if (e_7) throw e_7.error; }
                    }
                if (w && G != null) {
                    var X = (Y = _.selections) == null ? void 0 : Y.get(G), z = X ? X.end : 0, K = b(z, 0, S), V = Math.max(0, x.length - 1);
                    for (var st = 0; st < x.length; st++) {
                        var dt = x[st];
                        if (K >= dt.start && K <= dt.end) {
                            V = st;
                            break;
                        }
                    }
                    var nt = (j = x[V]) != null ? j : { start: 0, end: 0, text: "" }, Q = v + d(P.slice(nt.start, K)), it = St(n, "__caret");
                    Et(it), it.zIndex = 2, it.visible = !0, it.moveTo(Q, k + V * H), it.lineTo(Q, k + V * H + H), it.stroke({ width: 1, color: A != null ? A : a.control.focusBorder });
                }
            }
            var D = x.map(function (W) { return W.text; }).join("\n"), R = It(n, "__valueText", function (W) { W.style = { fontFamily: a.fontFamily, fontSize: a.fontSize, fill: a.text, fontWeight: "400", wordWrap: !1 }, W.zIndex = 1; });
            R.text = D, R.position.set(v, k);
        }
        E && ($t(n), n.eventMode = "static", n.cursor = "text", n.on("pointerdown", function (N) {
            var e_8, _a, e_9, _b, e_10, _c;
            var P, O, C, x, S, D, R, W, X, z, K, V, nt;
            if ((N == null ? void 0 : N.button) === 2)
                return;
            var p = t.getPointerId ? t.getPointerId(N) : Number((C = (O = N == null ? void 0 : N.pointerId) != null ? O : (P = N == null ? void 0 : N.data) == null ? void 0 : P.pointerId) != null ? C : 0);
            if (!(p <= 0)) {
                if (f.focusedKeyByPointer.set(p, E), f.keyboardOwnerPointerId = p, y === "checkbox") {
                    var Q = m(E, e.attrs), it = Q.indeterminate === !0, st = Q.checked === !0;
                    !st && !it ? (Q.checked = !0, Q.indeterminate = !1) : st && !it ? (Q.checked = !1, Q.indeterminate = !0) : (Q.checked = !1, Q.indeterminate = !1);
                }
                else if (y === "radio") {
                    var it = "radio:".concat((S = (x = e.attrs) == null ? void 0 : x.name) != null ? S : "__default__"), st = (D = c.get(it)) != null ? D : [];
                    try {
                        for (var st_1 = __values(st), st_1_1 = st_1.next(); !st_1_1.done; st_1_1 = st_1.next()) {
                            var dt = st_1_1.value;
                            var yt = m(dt, void 0);
                            yt.checked = dt === E;
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
                    var Q = m(E, e.attrs);
                    if (typeof Q.value == "string") {
                        try {
                            for (var _d = __values(f.inputs.entries()), _f = _d.next(); !_f.done; _f = _d.next()) {
                                var _g = __read(_f.value, 2), Mt = _g[0], Dt = _g[1];
                                Mt !== E && ((R = Dt.selections) == null || R.delete(p));
                            }
                        }
                        catch (e_9_1) { e_9 = { error: e_9_1 }; }
                        finally {
                            try {
                                if (_f && !_f.done && (_b = _d.return)) _b.call(_d);
                            }
                            finally { if (e_9) throw e_9.error; }
                        }
                        var it = y === "password" ? "\u2022".repeat(Q.value.length) : Q.value, st = f.fieldBounds.get(E), dt = (W = st == null ? void 0 : st.innerWidth) != null ? W : Math.max(0, i - v * 2), yt = ue(ce(it, dt, d), T), Rt = ((z = (X = N.global) == null ? void 0 : X.x) != null ? z : 0) - s - v, Gt = ((V = (K = N.global) == null ? void 0 : K.y) != null ? V : 0) - l - k, at = Ee({ fullText: it, lines: yt, localX: Rt, localY: Gt, lineHeight: H, measure: d });
                        Q.selections || (Q.selections = new Map), Q.selections.set(p, { start: at, end: at });
                        try {
                            for (var _h = __values(M.entries()), _j = _h.next(); !_j.done; _j = _h.next()) {
                                var _k = __read(_j.value, 2), Mt = _k[0], Dt = _k[1];
                                Dt.key === E && Mt !== p && M.delete(Mt);
                            }
                        }
                        catch (e_10_1) { e_10 = { error: e_10_1 }; }
                        finally {
                            try {
                                if (_j && !_j.done && (_c = _h.return)) _c.call(_h);
                            }
                            finally { if (e_10) throw e_10.error; }
                        }
                        M.set(p, { key: E, anchor: at });
                    }
                }
                (y === "checkbox" || y === "radio") && ((nt = N.stopPropagation) == null || nt.call(N)), g == null || g();
            }
        }), (y === "checkbox" || y === "radio") && (n.cursor = "pointer"), n.hitArea = new gt(0, 0, Math.max(0, i), Math.max(0, o)));
    }
    function Hr(t, e) { t.setPadding(e.EDGE_TOP, 6), t.setPadding(e.EDGE_BOTTOM, 6), t.setHeight(108), t.setMinHeight(108), t.setMinWidth(220); }
    function $r(t) {
        var e_11, _a, e_12, _b;
        var tt, Y, j, N, p, P, O, C;
        var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.absX, l = t.absY, a = t.theme, d = t.textMeasure, f = t.uiState, m = t.getOrInitInputState, b = t.clamp, c = t.textDrags, M = t.requestPaint, g = e.key, y = g ? m(g, ge(re({}, (tt = e.attrs) != null ? tt : {}), { type: "text" })) : void 0, E = (Y = t.showCaret) != null ? Y : !1, _ = (j = t.caretPointerId) != null ? j : null, w = t.focusColor, G = t.getCursorColor;
        n.sortableChildren = !0;
        try {
            for (var _c = __values(n.children), _d = _c.next(); !_d.done; _d = _c.next()) {
                var x = _d.value;
                var S = x.label;
                S && (S.startsWith("__sel:") || S === "__caret") && (x.visible = !1);
            }
        }
        catch (e_11_1) { e_11 = { error: e_11_1 }; }
        finally {
            try {
                if (_d && !_d.done && (_a = _c.return)) _a.call(_c);
            }
            finally { if (e_11) throw e_11.error; }
        }
        var A = 8, $ = 6 + xt, v = 5, k = a.fontSize * 1.25, T = w != null ? 2 : 1, H = T / 2;
        a.control.radius > 0 ? r.roundRect(H, H, Math.max(0, i - T), Math.max(0, o - T), a.control.radius) : r.rect(H, H, Math.max(0, i - T), Math.max(0, o - T)), r.fill(a.control.background), r.stroke({ width: T, color: w != null ? w : a.control.border });
        var U = (N = y == null ? void 0 : y.value) != null ? N : "", L = Math.max(0, i - A * 2);
        g && f.fieldBounds.set(g, { x: s, y: l, w: i, h: o, innerLeft: A, innerTop: $, innerWidth: L, maxLines: v, isPassword: !1 });
        var h = ce(U, L, d), I = ue(h, v), B = I.length > 0 ? I[I.length - 1].end : 0;
        if (g && y && typeof y.value == "string") {
            var x = y.selections;
            if (x && x.size > 0)
                try {
                    for (var _f = __values(x.entries()), _g = _f.next(); !_g.done; _g = _f.next()) {
                        var _h = __read(_g.value, 2), S = _h[0], D = _h[1];
                        var R = b((p = D.start) != null ? p : 0, 0, U.length), W = b((P = D.end) != null ? P : R, 0, U.length), X = b(Math.min(R, W), 0, B), z = b(Math.max(R, W), 0, B);
                        if (X === z)
                            continue;
                        var K = St(n, "__sel:".concat(S));
                        Et(K), K.zIndex = 0, K.visible = !0;
                        for (var V = 0; V < I.length; V++) {
                            var nt = I[V], Q = Math.max(X, nt.start), it = Math.min(z, nt.end);
                            if (Q >= it)
                                continue;
                            var st = A + d(U.slice(nt.start, Q)), dt = d(U.slice(Q, it));
                            K.rect(st, $ + V * k, dt, k);
                        }
                        K.fill({ color: G(S), alpha: .22 });
                    }
                }
                catch (e_12_1) { e_12 = { error: e_12_1 }; }
                finally {
                    try {
                        if (_g && !_g.done && (_b = _f.return)) _b.call(_f);
                    }
                    finally { if (e_12) throw e_12.error; }
                }
            if (E && _ != null) {
                var S = (O = y.selections) == null ? void 0 : O.get(_), D = S ? S.end : 0, R = b(D, 0, B), W = Math.max(0, I.length - 1);
                for (var V = 0; V < I.length; V++) {
                    var nt = I[V];
                    if (R >= nt.start && R <= nt.end) {
                        W = V;
                        break;
                    }
                }
                var X = (C = I[W]) != null ? C : { start: 0, end: 0, text: "" }, z = A + d(U.slice(X.start, R)), K = St(n, "__caret");
                Et(K), K.zIndex = 2, K.visible = !0, K.moveTo(z, $ + W * k), K.lineTo(z, $ + W * k + k), K.stroke({ width: 1, color: w != null ? w : a.control.focusBorder });
            }
        }
        var q = I.map(function (x) { return x.text; }).join("\n"), Z = It(n, "__valueText", function (x) { x.style = { fontFamily: a.fontFamily, fontSize: a.fontSize, fill: a.text, fontWeight: "400", wordWrap: !1 }, x.zIndex = 1; });
        Z.text = q, Z.position.set(A, $), g && ($t(n), n.eventMode = "static", n.cursor = "text", n.hitArea = new gt(0, 0, Math.max(0, i), Math.max(0, o)), n.on("pointerdown", function (x) {
            var e_13, _a, e_14, _b;
            var R, W, X, z, K, V, nt, Q, it, st;
            if ((x == null ? void 0 : x.button) === 2)
                return;
            var S = t.getPointerId ? t.getPointerId(x) : Number((X = (W = x == null ? void 0 : x.pointerId) != null ? W : (R = x == null ? void 0 : x.data) == null ? void 0 : R.pointerId) != null ? X : 0);
            if (S <= 0)
                return;
            f.focusedKeyByPointer.set(S, g), f.keyboardOwnerPointerId = S;
            var D = m(g, ge(re({}, (z = e.attrs) != null ? z : {}), { type: "text" }));
            if (typeof D.value == "string") {
                try {
                    for (var _c = __values(f.inputs.entries()), _d = _c.next(); !_d.done; _d = _c.next()) {
                        var _f = __read(_d.value, 2), Ct = _f[0], Nt = _f[1];
                        Ct !== g && ((K = Nt.selections) == null || K.delete(S));
                    }
                }
                catch (e_13_1) { e_13 = { error: e_13_1 }; }
                finally {
                    try {
                        if (_d && !_d.done && (_a = _c.return)) _a.call(_c);
                    }
                    finally { if (e_13) throw e_13.error; }
                }
                var dt = f.fieldBounds.get(g), yt = (V = dt == null ? void 0 : dt.innerWidth) != null ? V : Math.max(0, i - A * 2), Rt = D.value, Gt = ue(ce(Rt, yt, d), v), at = ((Q = (nt = x.global) == null ? void 0 : nt.x) != null ? Q : 0) - s - A, Mt = ((st = (it = x.global) == null ? void 0 : it.y) != null ? st : 0) - l - $, Dt = Ee({ fullText: Rt, lines: Gt, localX: at, localY: Mt, lineHeight: k, measure: d });
                D.selections || (D.selections = new Map), D.selections.set(S, { start: Dt, end: Dt });
                try {
                    for (var _g = __values(c.entries()), _h = _g.next(); !_h.done; _h = _g.next()) {
                        var _j = __read(_h.value, 2), Ct = _j[0], Nt = _j[1];
                        Nt.key === g && Ct !== S && c.delete(Ct);
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
            M == null || M();
        }));
    }
    function Ur(t, e) { t.setFlexDirection(e.FLEX_DIRECTION_ROW), t.setAlignItems(e.ALIGN_CENTER), t.setJustifyContent(e.JUSTIFY_FLEX_START), t.setPadding(e.EDGE_LEFT, 8), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0); }
    function qi(t, e, n, r, i) { t.circle(e, n, r), t.stroke({ width: 2, color: i }); var o = e + r * .65, s = n + r * .65, l = e + r * 1.55, a = n + r * 1.55; t.moveTo(o, s), t.lineTo(l, a), t.stroke({ width: 2, color: i }); }
    function Xr(t, e) { t.setFlexDirection(e.FLEX_DIRECTION_ROW), t.setFlexWrap(e.WRAP_NO_WRAP), t.setAlignItems(e.ALIGN_CENTER), t.setJustifyContent(e.JUSTIFY_FLEX_START), t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0); }
    function Yr(t, e) { t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0), t.setWidth(36), t.setHeight(36), t.setMinWidth(36), t.setMinHeight(36), t.setFlexGrow(0), t.setFlexShrink(0), t.setMargin(e.EDGE_RIGHT, 6); }
    function Kr(t) { var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.theme, l = t.uiState, a = t.getPointerId, d = t.focusInputKey, f = t.requestPaint, m = function (c) { r.clear(); var M = 1, g = M / 2; s.control.button.radius > 0 ? r.roundRect(g, g, Math.max(0, i - M), Math.max(0, o - M), s.control.button.radius) : r.rect(g, g, Math.max(0, i - M), Math.max(0, o - M)), r.fill(c), r.stroke({ width: M, color: s.control.button.border }); var y = i / 2 - 2, E = o / 2 - 2, _ = Math.max(5, Math.min(7, Math.min(i, o) * .22)); qi(r, y, E, _, s.text); }; m(s.control.button.fill), $t(n), n.eventMode = "static", n.cursor = "pointer", n.hitArea = new gt(0, 0, Math.max(0, i), Math.max(0, o)), n.on("pointerover", function () { return m(s.control.button.hoverFill); }), n.on("pointerout", function () { return m(s.control.button.fill); }), n.on("pointerdown", function (c) { var M; if ((c == null ? void 0 : c.button) !== 2) {
        if (m(s.control.button.activeFill), d) {
            var g = a(c);
            g > 0 && (l.focusedKeyByPointer.set(g, d), l.keyboardOwnerPointerId = g);
        }
        f == null || f(), (M = c.stopPropagation) == null || M.call(c);
    } }), n.on("pointerup", function () { return m(s.control.button.hoverFill); }); var b = e.attrs; }
    function Qe(t, e) { var n = t.get(e); if (n)
        return n; var r = { x: 0, y: 0 }; return t.set(e, r), r; }
    function zr(t, e) { t.setPositionType(e.POSITION_TYPE_ABSOLUTE), t.setPosition(e.EDGE_LEFT, 0), t.setPosition(e.EDGE_TOP, 0), t.setAlignSelf(e.ALIGN_FLEX_START), t.setFlexGrow(0), t.setFlexShrink(0), t.setPadding(e.EDGE_LEFT, 12), t.setPadding(e.EDGE_RIGHT, 12), t.setPadding(e.EDGE_TOP, 12), t.setPadding(e.EDGE_BOTTOM, 12), t.setWidth(540), t.setMinWidth(360), t.setMinHeight(148); }
    function jr(t) { var G, A; var e = t.node, n = t.container, r = t.w, i = t.h, o = t.theme, s = t.selectedBy, l = t.getCursorColor, a = t.dialogStates, d = t.dialogDrags, f = t.bringToFront, m = t.requestPaint, b = e.key; if (!b)
        return; var c = s.get(b), M = c == null ? o.boxBorder : l(c), g = Math.max(0, Math.round(r)), y = Math.max(0, Math.round(i)), E = St(n, "__dialogBorder"); Et(E), E.rect(0, 0, g, y), E.fill({ color: 16777215, alpha: .8 }); var _ = c == null ? 1 : 2, w = _ / 2; E.rect(w, w, Math.max(0, g - _), Math.max(0, y - _)), E.stroke({ width: _, color: M, alignment: 0 }), E.eventMode = "static", E.cursor = "move", E.hitArea = new gt(0, 0, g, y), E.on("pointerdown", function ($) {
        var e_15, _a;
        var H, U, L, h, I, B, q, Z;
        var v = function (tt) { try {
            typeof console != "undefined" && typeof console.log == "function" && console.log("[dialog pointerdown] ".concat(tt));
        }
        catch (Y) { } };
        if (v("start"), ($ == null ? void 0 : $.button) === 2)
            return;
        v("pointer-id");
        var k = t.getPointerId ? t.getPointerId($) : Number((L = (U = $ == null ? void 0 : $.pointerId) != null ? U : (H = $ == null ? void 0 : $.data) == null ? void 0 : H.pointerId) != null ? L : 0);
        if (k <= 0 || k <= 0)
            return;
        v("clear-other-drags");
        try {
            for (var _b = __values(d.entries()), _c = _b.next(); !_c.done; _c = _b.next()) {
                var _d = __read(_c.value, 2), tt = _d[0], Y = _d[1];
                Y.key === b && tt !== k && d.delete(tt);
            }
        }
        catch (e_15_1) { e_15 = { error: e_15_1 }; }
        finally {
            try {
                if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
            }
            finally { if (e_15) throw e_15.error; }
        }
        v("select"), s.set(b, k), v("bring-to-front"), f == null || f(b), v("state");
        var T = Qe(a, b);
        v("set-drag"), d.set(k, { key: b, startGX: (I = (h = $.global) == null ? void 0 : h.x) != null ? I : 0, startGY: (q = (B = $.global) == null ? void 0 : B.y) != null ? q : 0, originX: T.x, originY: T.y }), v("request-paint"), m == null || m(), v("stop-propagation"), (Z = $.stopPropagation) == null || Z.call($), v("done");
    }); {
        var $ = n.getChildByLabel, v = (A = (G = $ == null ? void 0 : $.call(n, "__children")) != null ? G : n.children.find(function (k) { return k && k.label === "__children"; })) != null ? A : null;
        if (v && E.parent === n) {
            var k = n.getChildIndex(v), T = Math.max(0, n.children.length - 1), H = Math.max(0, Math.min(k - 1, T));
            n.getChildIndex(E) > H && n.setChildIndex(E, H);
        }
    } }
    function Sn(t, e, n) { var l; var r = t.get(e); if (r)
        return r; var i = Number((l = n == null ? void 0 : n.value) != null ? l : "0"), s = { value: Number.isFinite(i) ? i : 0 }; return t.set(e, s), s; }
    function Vr(t, e) { t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0), t.setHeight(36), t.setMinHeight(36), t.setMinWidth(140), t.setFlexGrow(0), t.setFlexShrink(0); }
    function to(t, e, n) { return Math.max(e, Math.min(n, t)); }
    function kn(t, e, n) { var i; var r = Number((i = t == null ? void 0 : t[e]) != null ? i : ""); return Number.isFinite(r) ? r : n; }
    function eo(t, e, n, r, i, o) { var l = e + 3, a = e + r - 3, d = n + 3, f = n + i - 3; t.moveTo(l, f), t.lineTo((l + a) / 2, d), t.lineTo(a, f), t.stroke({ width: 2, color: o }); }
    function no(t, e, n, r, i, o) { var l = e + 3, a = e + r - 3, d = n + 3, f = n + i - 3; t.moveTo(l, d), t.lineTo((l + a) / 2, f), t.lineTo(a, d), t.stroke({ width: 2, color: o }); }
    function Jr(t) { var L; var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.theme, l = t.getValue, a = t.setValue, d = t.requestPaint, f = e.key, m = e.attrs, b = kn(m, "min", 0), c = kn(m, "max", 255), M = Math.max(1e-9, kn(m, "step", 1)), g = l(), y = 1, E = y / 2; r.rect(E, E, Math.max(0, i - y), Math.max(0, o - y)), r.fill(s.control.background), r.stroke({ width: y, color: s.control.border }); var _ = 22, w = Math.max(0, i - _); r.moveTo(w + .5, 0), r.lineTo(w + .5, o), r.stroke({ width: 1, color: s.control.border, alignment: 0 }); var G = St(n, "__arrows"); Et(G), eo(G, w, 0, _, o / 2, s.text), no(G, w, o / 2, _, o / 2, s.text); var A = ((L = m == null ? void 0 : m.channel) != null ? L : "").toLowerCase(), $ = A === "r" ? "R" : A === "g" ? "G" : A === "b" ? "B" : A === "a" ? "A" : "", v = It(n, "__valueText", function (h) { h.style = { fontFamily: s.fontFamily, fontSize: s.fontSize, fill: s.text, fontWeight: "400", wordWrap: !1 }; }); if (v.text = $ ? "".concat($, ": ").concat(Math.round(g)) : String(Math.round(g)), v.position.set(8, 9 + xt), !f)
        return; var k = new gt(w, 0, _, o / 2), T = new gt(w, o / 2, _, o / 2), H = function (h) { var I = l(), B = to(I + h * M, b, c); a(B), d == null || d(); }, U = St(n, "__hit"); Et(U), U.eventMode = "static", U.cursor = "default", U.hitArea = new gt(0, 0, Math.max(0, i), Math.max(0, o)), U.on("pointerdown", function (h) {
        var e_16, _a;
        var j, N, p, P, O, C;
        if ((h == null ? void 0 : h.button) === 2)
            return;
        var I = t.getPointerId ? t.getPointerId(h) : Number((p = (N = h == null ? void 0 : h.pointerId) != null ? N : (j = h == null ? void 0 : h.data) == null ? void 0 : j.pointerId) != null ? p : 0);
        if (I <= 0)
            return;
        var B = n.toLocal(h.global), q = (P = B == null ? void 0 : B.x) != null ? P : 0, Z = (O = B == null ? void 0 : B.y) != null ? O : 0, tt = k.contains(q, Z) ? 1 : T.contains(q, Z) ? -1 : null;
        if (!tt)
            return;
        H(tt);
        var Y = t.numberHolds;
        if (Y && f) {
            try {
                for (var _b = __values(Y.entries()), _c = _b.next(); !_c.done; _c = _b.next()) {
                    var _d = __read(_c.value, 2), D = _d[0], R = _d[1];
                    D !== I && (R.timeoutId != null && window.clearTimeout(R.timeoutId), R.intervalId != null && window.clearInterval(R.intervalId), Y.delete(D));
                }
            }
            catch (e_16_1) { e_16 = { error: e_16_1 }; }
            finally {
                try {
                    if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
                }
                finally { if (e_16) throw e_16.error; }
            }
            var x = Y.get(I);
            x && (x.timeoutId != null && window.clearTimeout(x.timeoutId), x.intervalId != null && window.clearInterval(x.intervalId));
            var S_1 = { key: f, timeoutId: null, intervalId: null };
            S_1.timeoutId = window.setTimeout(function () { S_1.timeoutId = null, S_1.intervalId = window.setInterval(function () { H(tt); }, 250); }, 500), Y.set(I, S_1);
        }
        (C = h.stopPropagation) == null || C.call(h);
    }); }
    var qe = null;
    function Zr() { return qe || (qe = new Ve({ data: ie, label: "attribute-color-picker-colors", shrinkToFit: !1, usage: Tn.VERTEX | Tn.COPY_DST }), qe); }
    function Qr(t, e, n) { var d, f, m, b; t.setPadding(n.EDGE_LEFT, 0), t.setPadding(n.EDGE_RIGHT, 0), t.setPadding(n.EDGE_TOP, 0), t.setPadding(n.EDGE_BOTTOM, 0); var r = Number((f = (d = e.attrs) == null ? void 0 : d.width) != null ? f : "0"), i = Number((b = (m = e.attrs) == null ? void 0 : m.height) != null ? b : "0"), o = Number.isFinite(r) && r > 0, s = Number.isFinite(i) && i > 0, l = o ? r : 240, a = s ? i : 200; (o || s) && (t.setAlignSelf(n.ALIGN_FLEX_START), t.setFlexGrow(0), t.setFlexShrink(0)), t.setWidth(l), t.setHeight(a), t.setMinWidth(Math.min(240, l)), t.setMinHeight(Math.min(200, a)); }
    function ae(t) { return Number.isFinite(t) ? Math.max(0, Math.min(255, Math.round(t))) : 0; }
    function tn(t) { return ae(t).toString(16).padStart(2, "0"); }
    function ro(t, e, n, r, i, o, s, l) { var a = s - n, d = l - r, f = i - n, m = o - r, b = t - n, c = e - r, M = a * a + d * d, g = a * f + d * m, y = a * b + d * c, E = f * f + m * m, _ = f * b + m * c, w = 1 / (M * E - g * g), G = (E * y - g * _) * w, A = (M * _ - g * y) * w; return G >= 0 && A >= 0 && G + A <= 1; }
    function io(t, e, n, r, i, o, s, l) { var a = i - n, d = o - r, f = s - n, m = l - r, b = t - n, c = e - r, M = a * m - f * d; if (Math.abs(M) < 1e-9)
        return { w0: 1, w1: 0, w2: 0 }; var g = (b * m - f * c) / M, y = (a * c - b * d) / M; return { w0: 1 - g - y, w1: g, w2: y }; }
    var oo = { name: "solid-out", fragment: { main: "\n      outColor = vec4(1.0);\n    " } }, en = null;
    function so() { if (en)
        return en; var t = { name: "color-picker-vertex-color", bits: [Qn, qn, Zn, oo] }; return en = new De({ glProgram: t, resources: {} }), en; }
    function qr(t, e, n) { var r = new Float32Array(12), i = [-90, -30, 30, 90, 150, 210]; for (var o = 0; o < 6; o++) {
        var s = i[o] * Math.PI / 180;
        r[o * 2 + 0] = t + Math.cos(s) * n, r[o * 2 + 1] = e + Math.sin(s) * n;
    } return r; }
    var ie = new Uint8Array([255, 0, 0, 255, 128, 128, 0, 255, 0, 255, 0, 255, 0, 128, 128, 255, 0, 0, 255, 255, 128, 0, 128, 255]), ke = new Uint32Array([0, 1, 2, 0, 2, 3, 0, 3, 4, 0, 4, 5]);
    function In(t) { var e = t.lx, n = t.ly, r = t.w, i = t.h, o = 10, s = Math.max(0, r - o * 2), l = Math.max(0, i - o * 2), a = o + s / 2, d = o + l / 2, f = Math.max(0, Math.min(s, l) / 2 - 2), m = qr(a, d, f); for (var b = 0; b < ke.length; b += 3) {
        var c = ke[b + 0], M = ke[b + 1], g = ke[b + 2], y = m[c * 2 + 0], E = m[c * 2 + 1], _ = m[M * 2 + 0], w = m[M * 2 + 1], G = m[g * 2 + 0], A = m[g * 2 + 1];
        if (!ro(e, n, y, E, _, w, G, A))
            continue;
        var $ = io(e, n, y, E, _, w, G, A), v = c * 4, k = M * 4, T = g * 4, H = $.w0 * ie[v + 0] + $.w1 * ie[k + 0] + $.w2 * ie[T + 0], U = $.w0 * ie[v + 1] + $.w1 * ie[k + 1] + $.w2 * ie[T + 1], L = $.w0 * ie[v + 2] + $.w1 * ie[k + 2] + $.w2 * ie[T + 2];
        return { r: ae(H), g: ae(U), b: ae(L) };
    } return null; }
    function ti(t) { var Y, j; var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.theme, l = t.rgb, a = t.setRgb, d = t.alpha, f = t.setAlpha, m = t.pick, b = t.setPick, c = t.requestPaint, M = t.getPointerId, g = t.setDraggingPointerId, y = 1, E = y / 2; r.rect(E, E, Math.max(0, i - y), Math.max(0, o - y)), r.fill(16777215), r.stroke({ width: y, color: s.control.border, alignment: 0 }); var _ = 10, w = Math.max(0, i - _ * 2), G = Math.max(0, o - _ * 2), A = _ + w / 2, $ = _ + G / 2, v = Math.max(0, Math.min(w, G) / 2 - 2), k = qr(A, $, v), T = "".concat(Math.round(i), "x").concat(Math.round(o)), H = n.getChildByLabel, U = H ? H.call(n, "__mesh") : n.children.find(function (N) { return (N == null ? void 0 : N.label) === "__mesh"; }); if (U) {
        if (U.__sizeKey !== T) {
            var N = new Float32Array(k.length), p = new Te({ positions: k, uvs: N, indices: ke });
            p.addAttribute("aColor", { buffer: Zr(), format: "unorm8x4", stride: 4, offset: 0 });
            try {
                (j = (Y = U.geometry) == null ? void 0 : Y.destroy) == null || j.call(Y);
            }
            catch (P) { }
            U.geometry = p, U.__sizeKey = T;
        }
    }
    else {
        var N = new Float32Array(k.length), p = new Te({ positions: k, uvs: N, indices: ke });
        p.addAttribute("aColor", { buffer: Zr(), format: "unorm8x4", stride: 4, offset: 0 }), U = new je({ geometry: p, shader: so() }), U.label = "__mesh", n.addChild(U), U.__sizeKey = T;
    } U.removeAllListeners(), U.eventMode = "static", U.cursor = "crosshair", U.hitArea = new gt(_, _, w, G), U.on("pointerdown", function (N) { var S, D, R; if ((N == null ? void 0 : N.button) === 2)
        return; var p = M(N); if (p <= 0)
        return; var P = n.toLocal(N.global), O = (S = P == null ? void 0 : P.x) != null ? S : 0, C = (D = P == null ? void 0 : P.y) != null ? D : 0, x = In({ lx: O, ly: C, w: i, h: o }); x && (b({ x: O, y: C }), a(x), g(p), c == null || c(), (R = N.stopPropagation) == null || R.call(N)); }); {
        var N = St(n, "__border");
        Et(N), N.moveTo(k[0], k[1]);
        for (var p = 1; p < 6; p++)
            N.lineTo(k[p * 2 + 0], k[p * 2 + 1]);
        N.closePath(), N.stroke({ width: 2, color: 0 });
    } var L = St(n, "__overlay"); Et(L); var h = 44, I = 18, B = Math.max(_, i - _ - h), q = _; L.rect(B, q, h, I), L.fill({ color: ae(l.r) << 16 | ae(l.g) << 8 | ae(l.b), alpha: Math.max(0, Math.min(1, ae(d) / 255)) }), L.rect(B + .5, q + .5, h - 1, I - 1), L.stroke({ width: 1, color: s.control.border, alignment: 0 }), m && (L.circle(m.x, m.y, 4), L.stroke({ width: 2, color: 16777215 }), L.circle(m.x, m.y, 4), L.stroke({ width: 1, color: 0 })); var Z = "#".concat(tn(l.r)).concat(tn(l.g)).concat(tn(l.b)).concat(tn(d)).toUpperCase(), tt = It(n, "__label", function (N) { N.style = { fontFamily: s.fontFamily, fontSize: Math.max(10, Math.floor(s.fontSize * .75)), fill: s.mutedText, fontWeight: "400", wordWrap: !1 }; }); tt.text = Z, tt.position.set(_, Math.max(_, o - _ - tt.height)), f && f(ae(d)); }
    function de(t, e, n) { var r = t.get(e); if (r)
        return r; var i = { selectedIndex: Math.max(0, n | 0), open: !1 }; return t.set(e, i), i; }
    function ei(t, e) { t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0), t.setHeight(36), t.setMinHeight(36), t.setMinWidth(220); }
    function ao(t, e, n, r, i, o) { var l = e + 4, a = e + r - 4, d = n + 4, f = n + i - 4; t.moveTo(l, (d + f) / 2 - 2), t.lineTo((l + a) / 2, (d + f) / 2 + 2), t.lineTo(a, (d + f) / 2 - 2), t.stroke({ width: 2, color: o }); }
    function lo(t) {
        var r;
        var n = String((r = t == null ? void 0 : t["data-options"]) != null ? r : "").split("\n").map(function (i) { return i.trim(); }).filter(function (i) { return i.length > 0; });
        return n.length > 0 ? n : ["(empty)"];
    }
    function co(t) { var n; var e = Number((n = t == null ? void 0 : t["data-selected-index"]) != null ? n : "0"); return Number.isFinite(e) ? Math.max(0, e | 0) : 0; }
    function nn(t) { var U; var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.absX, l = t.absY, a = t.theme, d = t.selectStates, f = t.uiState, m = t.getPointerId, b = t.getCursorColor, c = t.requestPaint, M = t.popupSink, g = e.key; if (!g)
        return; var y = lo(e.attrs), E = co(e.attrs), _ = de(d, g, E); _.selectedIndex = Math.max(0, Math.min(y.length - 1, _.selectedIndex | 0)); var w = (function () {
        var e_17, _a;
        var L = f.keyboardOwnerPointerId;
        if (f.focusedKeyByPointer.get(L) === g)
            return L;
        try {
            for (var _b = __values(f.focusedKeyByPointer.entries()), _c = _b.next(); !_c.done; _c = _b.next()) {
                var _d = __read(_c.value, 2), h = _d[0], I = _d[1];
                if (I === g)
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
    })(), G = w != null ? b(w) : null, A = G != null ? 2 : 1, $ = A / 2; a.control.radius > 0 ? r.roundRect($, $, Math.max(0, i - A), Math.max(0, o - A), a.control.radius) : r.rect($, $, Math.max(0, i - A), Math.max(0, o - A)), r.fill(a.control.background), r.stroke({ width: A, color: G != null ? G : a.control.border }); var v = 22, k = Math.max(0, i - v); r.moveTo(k + .5, 0), r.lineTo(k + .5, o), r.stroke({ width: 1, color: a.control.border, alignment: 0 }), ao(r, k, 0, v, o, a.text); var T = (U = y[_.selectedIndex]) != null ? U : "", H = It(n, "__label", function (L) { L.style = { fontFamily: a.fontFamily, fontSize: a.fontSize, fill: a.text, fontWeight: "400", wordWrap: !1 }; }); H.text = T, H.position.set(8, 9 + xt), $t(n), n.eventMode = "static", n.cursor = "pointer", n.hitArea = new gt(0, 0, Math.max(0, i), Math.max(0, o)), n.on("pointerdown", function (L) { var I; if ((L == null ? void 0 : L.button) === 2)
        return; var h = m(L); h <= 0 || (f.focusedKeyByPointer.set(h, g), f.keyboardOwnerPointerId = h, _.open = !_.open, c == null || c(), (I = L.stopPropagation) == null || I.call(L)); }), _.open && M.push({ key: g, absX: s, absY: l, w: i, h: o, options: y, selectedIndex: _.selectedIndex }); }
    function ni(t) { var _; var e = t.popup, n = t.stage, r = t.theme, i = t.selectStates, o = t.uiState, s = t.getPointerId, l = t.requestPaint, a = t.viewportW, d = t.viewportH, f = 30, b = Math.min(7, e.options.length), c = b * f, M = e.absX, g = e.absY + e.h; M = Math.max(0, Math.min(M, Math.max(0, a - e.w))), g + c > d - 4 && (g = e.absY - c), g = Math.max(0, Math.min(g, Math.max(0, d - c))); var y = new _t; y.position.set(M, g), n.addChild(y); var E = new wt; E.rect(0, 0, e.w, c), E.fill(16777215), E.rect(.5, .5, Math.max(0, e.w - 1), Math.max(0, c - 1)), E.stroke({ width: 1, color: r.control.border, alignment: 0 }), y.addChild(E), y.eventMode = "static", y.cursor = "pointer", y.hitArea = new gt(0, 0, e.w, c), y.on("pointerdown", function (w) { var H, U, L; if ((w == null ? void 0 : w.button) === 2)
        return; var G = s(w), A = y.toLocal(w.global), $ = (H = A == null ? void 0 : A.x) != null ? H : -1, v = (U = A == null ? void 0 : A.y) != null ? U : -1; if ($ < 0 || $ > e.w || v < 0 || v > c)
        return; var k = Math.max(0, Math.min(e.options.length - 1, Math.floor(v / f))), T = i.get(e.key); T && (T.selectedIndex = k, T.open = !1), G > 0 && (o.focusedKeyByPointer.set(G, e.key), o.keyboardOwnerPointerId = G), l == null || l(), (L = w.stopPropagation) == null || L.call(w); }); for (var w = 0; w < b; w++) {
        var G = w * f;
        if (w === e.selectedIndex) {
            var $ = new wt;
            $.rect(1, G + 1, Math.max(0, e.w - 2), f - 2), $.fill({ color: 0, alpha: .06 }), y.addChild($);
        }
        var A = jt({ text: (_ = e.options[w]) != null ? _ : "", fontFamily: r.fontFamily, fontSize: r.fontSize, fill: r.text, wordWrap: !1 });
        A.position.set(8, G + 7 + xt), y.addChild(A);
    } }
    function Pt(t, e, n) { var r = Number.isFinite(t) ? t | 0 : 0; return Math.max(e, Math.min(n, r)); }
    function Yt(t) { var e = Pt(t, 0, 99); return e < 10 ? "0".concat(e) : String(e); }
    function qt(t, e, n) { var r = Number(t); if (!Number.isFinite(r))
        return null; var i = Math.trunc(r); return i < e || i > n ? null : i; }
    function sn(t) { if (t.length !== 4)
        return null; for (var r = 0; r < t.length; r += 1) {
        var i = t.charCodeAt(r);
        if (i < 48 || i > 57)
            return null;
    } var e = Number(t); if (!Number.isFinite(e))
        return null; var n = e - 2e3; return n < 0 || n > 99 ? null : n; }
    function uo(t) { var e = String(t != null ? t : "").trim().split(":"); if (e.length !== 2 && e.length !== 3)
        return null; var n = qt(e[0], 0, 23), r = qt(e[1], 0, 59), i = e.length === 3 ? qt(e[2], 0, 59) : 0; return n == null || r == null || i == null ? null : { hour: n, minute: r, second: i }; }
    function ho(t) { var e = String(t != null ? t : "").trim().split("-"); if (e.length !== 2)
        return null; var n = sn(e[0]), r = qt(e[1], 1, 12); return n == null || r == null ? null : { year2: n, month: r }; }
    function mo(t) { var e = String(t != null ? t : "").trim().split("-"); if (e.length !== 3)
        return null; var n = sn(e[0]), r = qt(e[1], 1, 12), i = qt(e[2], 1, 31); if (n == null || r == null || i == null)
        return null; var o = Pt(Math.floor((i - 1) / 7) + 1, 1, 4); return { year2: n, month: r, weekIndex: o }; }
    function fo(t) { var e = String(t != null ? t : "").trim(), n = e.indexOf("-W"); if (n < 0)
        return null; var r = sn(e.slice(0, n)), i = qt(e.slice(n + 2), 1, 48); if (r == null || i == null)
        return null; var o = Pt(Math.floor((i - 1) / 4) + 1, 1, 12), s = Pt((i - 1) % 4 + 1, 1, 4); return { year2: r, month: o, weekIndex: s }; }
    function po(t) { var e = String(t != null ? t : "").trim(), n = e.indexOf("T"); if (n < 0 && (n = e.indexOf(" ")), n < 0)
        return null; var r = e.slice(0, n).split("-"), i = e.slice(n + 1).split(":"); if (r.length !== 3 || i.length !== 2 && i.length !== 3)
        return null; var o = sn(r[0]), s = qt(r[1], 1, 12), l = qt(r[2], 1, 31), a = qt(i[0], 0, 23), d = qt(i[1], 0, 59), f = i.length === 3 ? qt(i[2], 0, 59) : 0; if (o == null || s == null || l == null || a == null || d == null || f == null)
        return null; var m = Pt(Math.floor((l - 1) / 7) + 1, 1, 4); return { year2: o, month: s, weekIndex: m, hour: a, minute: d, second: f }; }
    function rn(t) { return "20".concat(Yt(t.year2), "-").concat(Yt(t.month)); }
    function go(t) { return (Pt(t.month, 1, 12) - 1) * 4 + Pt(t.weekIndex, 1, 4); }
    function on(t) { return "20".concat(Yt(t.year2), "-W").concat(Yt(go(t))); }
    function Se(t) { var e = (Pt(t.weekIndex, 1, 4) - 1) * 7 + 1; return "20".concat(Yt(t.year2), "-").concat(Yt(t.month), "-").concat(Yt(e)); }
    function Le(t) { return "".concat(Yt(t.hour), ":").concat(Yt(t.minute), ":").concat(Yt(t.second)); }
    function ve(t) { return "".concat(Se(t), "T").concat(Le(t)); }
    function bo(t) { var f; var e = t.map, n = t.yearSliderOwners, r = t.inputKey, i = t.kind, o = t.attrs, s = e.get(r); if (s)
        return s.kind = i, s; var l = new Date, a = { kind: i, year2: Pt(l.getFullYear() - 2e3, 0, 99), month: Pt(l.getMonth() + 1, 1, 12), weekIndex: 1, hour: Pt(l.getHours(), 0, 23), minute: Pt(l.getMinutes(), 0, 59), second: Pt(l.getSeconds(), 0, 59), openPanel: null, openYear: !1, openMonthGrid: !1, yearSliderKey: "".concat(r, ":year-slider") }, d = String((f = o == null ? void 0 : o.value) != null ? f : ""); if (d.trim().length > 0) {
        if (i === "time") {
            var m = uo(d);
            m && (a.hour = m.hour, a.minute = m.minute, a.second = m.second);
        }
        else if (i === "month") {
            var m = ho(d);
            m && (a.year2 = m.year2, a.month = m.month);
        }
        else if (i === "week") {
            var m = fo(d);
            m && (a.year2 = m.year2, a.month = m.month, a.weekIndex = m.weekIndex);
        }
        else if (i === "date") {
            var m = mo(d);
            m && (a.year2 = m.year2, a.month = m.month, a.weekIndex = m.weekIndex);
        }
        else if (i === "datetime-local") {
            var m = po(d);
            m && (a.year2 = m.year2, a.month = m.month, a.weekIndex = m.weekIndex, a.hour = m.hour, a.minute = m.minute, a.second = m.second);
        }
    } return e.set(r, a), n.set(a.yearSliderKey, r), a; }
    function ii(t, e, n) { t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0), t.setHeight(36), t.setMinHeight(36), t.setMinWidth(n === "datetime-local" ? 340 : 220); }
    function xo(t, e, n, r, i) { var o = i != null ? 2 : 1, s = o / 2; e.control.radius > 0 ? t.roundRect(s, s, Math.max(0, n - o), Math.max(0, r - o), e.control.radius) : t.rect(s, s, Math.max(0, n - o), Math.max(0, r - o)), t.fill(e.control.background), t.stroke({ width: o, color: i != null ? i : e.control.border }); }
    function ri(t, e, n, r, i) { var o = e + r / 2, s = n + r / 2; t.moveTo(e, s - 2), t.lineTo(o, s + 2), t.lineTo(e + r, s - 2), t.stroke({ width: 2, color: i }); }
    function oi(t) { var k, T; var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.absX, l = t.absY, a = t.theme, d = t.uiState, f = t.getPointerId, m = t.getCursorColor, b = t.temporalStates, c = t.yearSliderOwners, M = t.getOrInitInputValue, g = t.requestPaint, y = t.popupSink, E = e.key; if (!E || !e.tagName)
        return; var _ = e.tagName === "timeinput" ? "time" : e.tagName === "monthinput" ? "month" : e.tagName === "weekinput" ? "week" : e.tagName === "dateinput" ? "date" : "datetime-local", w = bo({ map: b, yearSliderOwners: c, inputKey: E, kind: _, attrs: e.attrs }), G = M(E, ge(re({}, (k = e.attrs) != null ? k : {}), { type: "text" })); _ === "time" ? G.value = Le(w) : _ === "month" ? G.value = rn(w) : _ === "week" ? G.value = on(w) : _ === "date" ? G.value = Se(w) : G.value = ve(w); var A = (function () {
        var e_18, _a;
        var H = d.keyboardOwnerPointerId;
        if (d.focusedKeyByPointer.get(H) === E)
            return H;
        try {
            for (var _b = __values(d.focusedKeyByPointer.entries()), _c = _b.next(); !_c.done; _c = _b.next()) {
                var _d = __read(_c.value, 2), U = _d[0], L = _d[1];
                if (L === E)
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
    })(), $ = A != null ? m(A) : null; xo(r, a, i, o, $); var v = 8; if (_ !== "datetime-local") {
        var H = (T = G.value) != null ? T : "", U = It(n, "__shown", function (I) { I.style = { fontFamily: a.fontFamily, fontSize: a.fontSize, fill: a.text, fontWeight: "400", wordWrap: !1 }; });
        U.text = H, U.visible = !0, U.position.set(v, 9 + xt);
        var L = n.getChildByLabel ? n.getChildByLabel("__date") : n.children.find(function (I) { return (I == null ? void 0 : I.label) === "__date"; }), h = n.getChildByLabel ? n.getChildByLabel("__time") : n.children.find(function (I) { return (I == null ? void 0 : I.label) === "__time"; });
        L && (L.visible = !1), h && (h.visible = !1), ri(r, Math.max(0, i - 18), 11, 10, a.text);
    }
    else {
        var H = Math.max(0, Math.round(i * .52));
        r.moveTo(H + .5, 0), r.lineTo(H + .5, o), r.stroke({ width: 1, color: a.control.border, alignment: 0 });
        var U = Se(w), L = Le(w), h = It(n, "__date", function (q) { q.style = { fontFamily: a.fontFamily, fontSize: a.fontSize, fill: a.text, fontWeight: "400", wordWrap: !1 }; });
        h.text = U, h.visible = !0, h.position.set(v, 9 + xt);
        var I = It(n, "__time", function (q) { q.style = { fontFamily: a.fontFamily, fontSize: a.fontSize, fill: a.text, fontWeight: "400", wordWrap: !1 }; });
        I.text = L, I.visible = !0, I.position.set(H + v, 9 + xt);
        var B = n.getChildByLabel ? n.getChildByLabel("__shown") : n.children.find(function (q) { return (q == null ? void 0 : q.label) === "__shown"; });
        B && (B.visible = !1), ri(r, Math.max(H + 0, H + (i - H) - 18), 11, 10, a.text);
    } $t(n), n.eventMode = "static", n.cursor = "pointer", n.hitArea = new gt(0, 0, Math.max(0, i), Math.max(0, o)), n.on("pointerdown", function (H) { var L, h, I; if ((H == null ? void 0 : H.button) === 2)
        return; var U = f(H); if (!(U <= 0)) {
        if (d.focusedKeyByPointer.set(U, E), d.keyboardOwnerPointerId = U, _ !== "datetime-local")
            w.openPanel = w.openPanel ? null : _ === "time" ? "time" : _ === "month" ? "month" : "week", w.openYear = !1, w.openMonthGrid = !1;
        else {
            var Z = ((h = (L = H.global) == null ? void 0 : L.x) != null ? h : 0) - s <= i * .52;
            w.openPanel = Z ? w.openPanel === "week" ? null : "week" : w.openPanel === "time" ? null : "time", w.openYear = !1, w.openMonthGrid = !1;
        }
        b.set(E, w), g == null || g(), (I = H.stopPropagation) == null || I.call(H);
    } }), w.openPanel === "month" ? y.push({ kind: "month-panel", inputKey: E, absX: s, absY: l, anchorW: i, anchorH: o }) : w.openPanel === "week" ? y.push({ kind: "week-panel", inputKey: E, absX: s, absY: l, anchorW: i, anchorH: o }) : w.openPanel === "time" && y.push({ kind: "time-panel", inputKey: E, absX: s, absY: l, anchorW: i, anchorH: o }); }
    function Ge(t, e, n, r) { t.rect(0, 0, n, r), t.fill(e.control.background), t.rect(.5, .5, Math.max(0, n - 1), Math.max(0, r - 1)), t.stroke({ width: 1, color: e.control.border, alignment: 0 }); }
    function yo(t) { var e = t.stage, n = t.theme, r = t.popup, i = t.st, o = t.viewportW, s = t.viewportH, l = t.getPointerId, a = t.requestPaint, d = t.onPick, f = 4, m = 3, b = 44, c = 34, M = 8, g = M * 2 + f * b, y = M * 2 + m * c, E = r.absX, _ = r.absY + r.anchorH; E = Math.max(0, Math.min(E, Math.max(0, o - g))), _ + y > s - 4 && (_ = r.absY - y), _ = Math.max(0, Math.min(_, Math.max(0, s - y))); var w = new _t; w.position.set(E, _), e.addChild(w); var G = new wt; Ge(G, n, g, y), w.addChild(G); for (var A = 0; A < 12; A++) {
        var $ = A + 1, v = M + A % f * b, k = M + Math.floor(A / f) * c;
        if ($ === i.month) {
            var H = new wt;
            H.rect(v + 1, k + 1, b - 2, c - 2), H.fill({ color: 0, alpha: .06 }), w.addChild(H);
        }
        var T = jt({ text: String($), fontFamily: n.fontFamily, fontSize: n.fontSize, fill: n.text, wordWrap: !1 });
        T.position.set(v + 14, k + 8 + xt), w.addChild(T), G.rect(v, k, b, c), G.stroke({ width: 1, color: n.control.border, alignment: 0 });
    } w.eventMode = "static", w.cursor = "pointer", w.hitArea = new gt(0, 0, g, y), w.on("pointerdown", function (A) { var q, Z, tt; if ((A == null ? void 0 : A.button) === 2 || l(A) <= 0)
        return; var v = w.toLocal(A.global), k = (q = v == null ? void 0 : v.x) != null ? q : -1, T = (Z = v == null ? void 0 : v.y) != null ? Z : -1, H = k - M, U = T - M; if (H < 0 || U < 0)
        return; var L = Math.floor(H / b), h = Math.floor(U / c); if (L < 0 || L >= f || h < 0 || h >= m)
        return; var B = h * f + L + 1; B < 1 || B > 12 || (d(B), a == null || a(), (tt = A.stopPropagation) == null || tt.call(A)); }); }
    function wo(t) {
        var e_19, _a;
        var e = t.stage, n = t.theme, r = t.popup, i = t.st, o = t.viewportW, s = t.viewportH, l = t.sliders, a = t.sliderBounds, d = t.sliderDrags, f = t.getPointerId, m = t.requestPaint, b = t.onChange, c = 10, M = 250, g = 78, y = r.absX, E = r.absY;
        y = r.absX + r.anchorW + 6, E = r.absY, y = Math.max(0, Math.min(y, Math.max(0, o - M))), E = Math.max(0, Math.min(E, Math.max(0, s - g)));
        var _ = new _t;
        _.position.set(y, E), e.addChild(_);
        var w = new wt;
        Ge(w, n, M, g), _.addChild(w);
        var G = jt({ text: "20".concat(Yt(i.year2)), fontFamily: n.fontFamily, fontSize: n.fontSize, fill: n.text, wordWrap: !1 });
        G.position.set(c, 8 + xt), _.addChild(G);
        var A = i.yearSliderKey, $ = Math.max(0, Math.min(1, Pt(i.year2, 0, 99) / 99)), v = ye(l, A, { value: String($) }), k = !1;
        try {
            for (var _b = __values(d.values()), _c = _b.next(); !_c.done; _c = _b.next()) {
                var L = _c.value;
                if (L.key === A) {
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
        k || (v.value = $);
        var T = new _t;
        T.position.set(c, 40), _.addChild(T);
        var H = new wt;
        T.addChild(H), Ze({ node: { key: A, attrs: { value: String(v.value) } }, container: T, graphics: H, w: M - c * 2, h: 14, absX: y + c, absY: E + 40, theme: { text: n.text, control: { progress: n.control.progress } }, sliderStates: l, sliderBounds: a, sliderDrags: d, requestPaint: m, getPointerId: f });
        var U = Pt(Math.round(v.value * 99), 0, 99);
        U !== i.year2 && b(U), _.eventMode = "static", _.hitArea = new gt(0, 0, M, g), _.on("pointerdown", function (L) { var h; (h = L.stopPropagation) == null || h.call(L); });
    }
    function _o(t) { var e = t.panel, n = t.theme, r = t.x, i = t.y, o = t.w, s = t.st, l = t.onPick, a = 30, d = 6, f = []; for (var m = 0; m < 4; m++) {
        var b = m + 1, c = i + m * (a + d), M = new wt;
        M.rect(r, c, o, a), M.fill({ color: 0, alpha: b === s.weekIndex ? .06 : .03 }), M.rect(r + .5, c + .5, Math.max(0, o - 1), Math.max(0, a - 1)), M.stroke({ width: 1, color: n.control.border, alignment: 0 }), e.addChild(M);
        var g = (Pt(s.month, 1, 12) - 1) * 4 + b, y = jt({ text: "".concat(b, " [").concat(Yt(g), "]"), fontFamily: n.fontFamily, fontSize: n.fontSize, fill: n.text, wordWrap: !1 });
        y.position.set(r + 10, c + 7 + xt), e.addChild(y), f.push({ x: r, y: c, w: o, h: a, weekIndex: b });
    } return { hitRects: f }; }
    function si(t) {
        var e_20, _a, e_21, _b;
        var _, w, G, A, $, v;
        var e = t.popups, n = t.stage, r = t.theme, i = t.viewportW, o = t.viewportH, s = t.temporalStates, l = t.getOrInitInputValue, a = t.sliders, d = t.sliderBounds, f = t.sliderDrags, m = t.selects, b = t.selectPopups, c = t.getCursorColor, M = t.uiFocus, g = t.getPointerId, y = t.requestPaint, E = [];
        var _loop_1 = function (k) {
            var T = s.get(k.inputKey);
            if (T) {
                if (k.kind === "month-panel") {
                    var Y = k.absX, j = k.absY + k.anchorH;
                    Y = Math.max(0, Math.min(Y, Math.max(0, i - 196))), j + 156 > o - 4 && (j = k.absY - 156), j = Math.max(0, Math.min(j, Math.max(0, o - 156)));
                    var N_1 = new _t;
                    N_1.position.set(Y, j), n.addChild(N_1);
                    var p = new wt;
                    Ge(p, r, 196, 156), N_1.addChild(p);
                    var P_1 = { x: 10, y: 10, w: 132, h: 24 };
                    {
                        var x = new wt;
                        x.rect(P_1.x, P_1.y, P_1.w, P_1.h), x.fill({ color: 0, alpha: .03 }), x.rect(P_1.x + .5, P_1.y + .5, Math.max(0, P_1.w - 1), Math.max(0, P_1.h - 1)), x.stroke({ width: 1, color: r.control.border, alignment: 0 }), N_1.addChild(x);
                        var S = jt({ text: "Year 20".concat(Yt(T.year2)), fontFamily: r.fontFamily, fontSize: r.fontSize, fill: r.text, wordWrap: !1 });
                        S.position.set(P_1.x + 8, P_1.y + 4 + xt), N_1.addChild(S);
                    }
                    var O_1 = 10, C_1 = 44;
                    for (var x = 0; x < 12; x++) {
                        var S = x + 1, D = O_1 + x % 4 * 44, R = C_1 + Math.floor(x / 4) * 34;
                        if (S === T.month) {
                            var X = new wt;
                            X.rect(D + 1, R + 1, 42, 32), X.fill({ color: 0, alpha: .06 }), N_1.addChild(X);
                        }
                        var W = jt({ text: String(S), fontFamily: r.fontFamily, fontSize: r.fontSize, fill: r.text, wordWrap: !1 });
                        W.position.set(D + 14, R + 8 + xt), N_1.addChild(W), p.rect(D, R, 44, 34), p.stroke({ width: 1, color: r.control.border, alignment: 0 });
                    }
                    N_1.eventMode = "static", N_1.cursor = "pointer", N_1.hitArea = new gt(0, 0, 196, 156), N_1.on("pointerdown", function (x) { var dt, yt, Rt, Gt; if ((x == null ? void 0 : x.button) === 2)
                        return; var S = g(x); if (S <= 0)
                        return; M.focusedKeyByPointer.set(S, k.inputKey), M.keyboardOwnerPointerId = S; var D = N_1.toLocal(x.global), R = (dt = D == null ? void 0 : D.x) != null ? dt : -1, W = (yt = D == null ? void 0 : D.y) != null ? yt : -1; if (R >= P_1.x && R <= P_1.x + P_1.w && W >= P_1.y && W <= P_1.y + P_1.h) {
                        T.openYear = !0, s.set(k.inputKey, T), y == null || y(), (Rt = x.stopPropagation) == null || Rt.call(x);
                        return;
                    } var z = R - O_1, K = W - C_1; if (z < 0 || K < 0)
                        return; var V = Math.floor(z / 44), nt = Math.floor(K / 34); if (V < 0 || V >= 4 || nt < 0 || nt >= 3)
                        return; var it = nt * 4 + V + 1; if (it < 1 || it > 12)
                        return; T.month = it, T.openPanel = null, T.openYear = !1, T.openMonthGrid = !1, s.set(k.inputKey, T); var st = l(k.inputKey, { type: "text" }); st.value = rn(T), y == null || y(), (Gt = x.stopPropagation) == null || Gt.call(x); }), N_1.on("pointerdown", function (x) { var S; (S = x.stopPropagation) == null || S.call(x); }), T.openYear && E.push({ kind: "year-panel", inputKey: k.inputKey, absX: Y, absY: j, anchorW: 196, anchorH: 0 });
                }
                if (k.kind === "week-panel") {
                    var h = k.absX, I = k.absY + k.anchorH;
                    h = Math.max(0, Math.min(h, Math.max(0, i - 280))), I + 192 > o - 4 && (I = k.absY - 192), I = Math.max(0, Math.min(I, Math.max(0, o - 192)));
                    var B_1 = new _t;
                    B_1.position.set(h, I), n.addChild(B_1);
                    var q = new wt;
                    Ge(q, r, 280, 192), B_1.addChild(q);
                    var Z_1 = { x: 10, y: 10, w: 104, h: 24 }, tt_1 = { x: 10 + Z_1.w + 10, y: 10, w: 120, h: 24 }, Y = function (p, P) { var O = new wt; O.rect(p.x, p.y, p.w, p.h), O.fill({ color: 0, alpha: .03 }), O.rect(p.x + .5, p.y + .5, Math.max(0, p.w - 1), Math.max(0, p.h - 1)), O.stroke({ width: 1, color: r.control.border, alignment: 0 }), B_1.addChild(O); var C = jt({ text: P, fontFamily: r.fontFamily, fontSize: r.fontSize, fill: r.text, wordWrap: !1 }); C.position.set(p.x + 8, p.y + 4 + xt), B_1.addChild(C); };
                    Y(Z_1, "Month ".concat(T.month)), Y(tt_1, "Year 20".concat(Yt(T.year2)));
                    var j = 44, N_2 = _o({ panel: B_1, theme: r, x: 10, y: j, w: 280 - 10 * 2, st: T, onPick: function () { } }).hitRects;
                    B_1.eventMode = "static", B_1.cursor = "pointer", B_1.hitArea = new gt(0, 0, 280, 192), B_1.on("pointerdown", function (p) {
                        var e_23, _a;
                        var D, R, W, X, z;
                        if ((p == null ? void 0 : p.button) === 2)
                            return;
                        var P = g(p);
                        if (P <= 0)
                            return;
                        M.focusedKeyByPointer.set(P, k.inputKey), M.keyboardOwnerPointerId = P;
                        var O = B_1.toLocal(p.global), C = (D = O == null ? void 0 : O.x) != null ? D : -1, x = (R = O == null ? void 0 : O.y) != null ? R : -1, S = function (K) { return C >= K.x && C <= K.x + K.w && x >= K.y && x <= K.y + K.h; };
                        if (S(Z_1)) {
                            T.openMonthGrid = !T.openMonthGrid, s.set(k.inputKey, T), y == null || y(), (W = p.stopPropagation) == null || W.call(p);
                            return;
                        }
                        if (S(tt_1)) {
                            T.openYear = !0, s.set(k.inputKey, T), y == null || y(), (X = p.stopPropagation) == null || X.call(p);
                            return;
                        }
                        try {
                            for (var N_3 = (e_23 = void 0, __values(N_2)), N_3_1 = N_3.next(); !N_3_1.done; N_3_1 = N_3.next()) {
                                var K = N_3_1.value;
                                if (S(K)) {
                                    T.weekIndex = K.weekIndex;
                                    var V = l(k.inputKey, { type: "text" });
                                    T.kind === "week" ? V.value = on(T) : T.kind === "date" ? V.value = Se(T) : V.value = ve(T), T.openPanel = null, T.openYear = !1, T.openMonthGrid = !1, s.set(k.inputKey, T), y == null || y(), (z = p.stopPropagation) == null || z.call(p);
                                    return;
                                }
                            }
                        }
                        catch (e_23_1) { e_23 = { error: e_23_1 }; }
                        finally {
                            try {
                                if (N_3_1 && !N_3_1.done && (_a = N_3.return)) _a.call(N_3);
                            }
                            finally { if (e_23) throw e_23.error; }
                        }
                    }), T.openMonthGrid && E.push({ kind: "month-grid", inputKey: k.inputKey, absX: h, absY: I + Z_1.y + Z_1.h + 4, anchorW: 0, anchorH: 0 }), T.openYear && E.push({ kind: "year-panel", inputKey: k.inputKey, absX: h + tt_1.x, absY: I + tt_1.y, anchorW: tt_1.w, anchorH: 0 });
                }
                if (k.kind === "time-panel") {
                    var h_1 = k.absX, I_1 = k.absY + k.anchorH;
                    h_1 = Math.max(0, Math.min(h_1, Math.max(0, i - 330))), I_1 + 80 > o - 4 && (I_1 = k.absY - 80), I_1 = Math.max(0, Math.min(I_1, Math.max(0, o - 80)));
                    var B_2 = new _t;
                    B_2.position.set(h_1, I_1), n.addChild(B_2);
                    var q = new wt;
                    Ge(q, r, 330, 80), B_2.addChild(q);
                    var Z = jt({ text: "Time", fontFamily: r.fontFamily, fontSize: r.fontSize, fill: r.text, wordWrap: !1 });
                    Z.position.set(10, 8 + xt), B_2.addChild(Z);
                    var tt_2 = function (nt) { return Array.from({ length: nt }, function (Q, it) { return Yt(it); }).join("\n"); }, Y = k.inputKey, j = "".concat(Y, ":time-h"), N = "".concat(Y, ":time-m"), p = "".concat(Y, ":time-s"), P = de(m, j, Pt(T.hour, 0, 23)), O = de(m, N, Pt(T.minute, 0, 59)), C = de(m, p, Pt(T.second, 0, 59));
                    P.selectedIndex = Pt(T.hour, 0, 23), O.selectedIndex = Pt(T.minute, 0, 59), C.selectedIndex = Pt(T.second, 0, 59);
                    var x_1 = 96, S_2 = 36, D_1 = 32, R = 8, W = function (nt, Q, it) { var st = new _t; st.position.set(Q, D_1), B_2.addChild(st); var dt = new wt; st.addChild(dt), nn({ node: { key: nt, attrs: { "data-options": tt_2(it), "data-selected-index": String(de(m, nt, 0).selectedIndex) } }, container: st, graphics: dt, w: x_1, h: S_2, absX: h_1 + Q, absY: I_1 + D_1, theme: r, selectStates: m, uiState: M, getPointerId: g, getCursorColor: c, requestPaint: y, popupSink: b }); };
                    W(j, 10, 24), W(N, 10 + x_1 + R, 60), W(p, 10 + (x_1 + R) * 2, 60);
                    var X = Pt((w = (_ = m.get(j)) == null ? void 0 : _.selectedIndex) != null ? w : T.hour, 0, 23), z = Pt((A = (G = m.get(N)) == null ? void 0 : G.selectedIndex) != null ? A : T.minute, 0, 59), K = Pt((v = ($ = m.get(p)) == null ? void 0 : $.selectedIndex) != null ? v : T.second, 0, 59);
                    T.hour = X, T.minute = z, T.second = K, s.set(k.inputKey, T);
                    var V = l(k.inputKey, { type: "text" });
                    T.kind === "time" ? V.value = Le(T) : V.value = ve(T), B_2.eventMode = "static", B_2.hitArea = new gt(0, 0, 330, 80), B_2.on("pointerdown", function (nt) { var Q; (Q = nt.stopPropagation) == null || Q.call(nt); });
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
            var T = s.get(k.inputKey);
            T && (k.kind === "month-grid" && yo({ stage: n, theme: r, popup: k, st: T, viewportW: i, viewportH: o, getPointerId: g, requestPaint: y, onPick: function (H) { T.month = H, T.openMonthGrid = !1, s.set(k.inputKey, T); var U = l(k.inputKey, { type: "text" }); T.kind === "month" ? U.value = rn(T) : T.kind === "week" ? U.value = on(T) : T.kind === "date" ? U.value = Se(T) : U.value = ve(T); } }), k.kind === "year-panel" && wo({ stage: n, theme: r, popup: k, st: T, viewportW: i, viewportH: o, sliders: a, sliderBounds: d, sliderDrags: f, getPointerId: g, requestPaint: y, onChange: function (H) { T.year2 = H, s.set(k.inputKey, T); var U = l(k.inputKey, { type: "text" }); T.kind === "month" ? U.value = rn(T) : T.kind === "week" ? U.value = on(T) : T.kind === "date" ? U.value = Se(T) : T.kind === "time" ? U.value = Le(T) : U.value = ve(T); } }));
        };
        try {
            for (var E_1 = __values(E), E_1_1 = E_1.next(); !E_1_1.done; E_1_1 = E_1.next()) {
                var k = E_1_1.value;
                _loop_2(k);
            }
        }
        catch (e_21_1) { e_21 = { error: e_21_1 }; }
        finally {
            try {
                if (E_1_1 && !E_1_1.done && (_b = E_1.return)) _b.call(E_1);
            }
            finally { if (e_21) throw e_21.error; }
        }
    }
    function ai(t) {
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
    var li = 5e4, Fe = new WeakMap, ui = new Map, To = 1, di = 0, Mo = 0, ci = !1, we = [], Pn = null;
    function We(t) { return t instanceof wt ? "Graphics" : t instanceof Qt ? "Text" : t instanceof _t ? "Container" : "Object"; }
    function Eo(t) { var e = t && typeof t == "object" ? t.label : void 0, n = t && typeof t == "object" ? We(t) : "Object"; return e ? "".concat(n, ":").concat(String(e).slice(0, 80)) : n; }
    function he(t) { var e = Fe.get(t); return e || (e = To++, Fe.set(t, e)), ui.set(e, t), e; }
    function an(t) { var e, n, r, i, o, s; if (t == null || typeof t == "number" || typeof t == "string" || typeof t == "boolean")
        return t; if (Array.isArray(t))
        return t.slice(0, 16).map(an); if (typeof t == "object") {
        var l = t;
        return "color" in l || "alpha" in l || "width" in l && !("x" in l) && !("y" in l) && !("height" in l) ? { color: l.color, alpha: l.alpha, width: l.width } : "x" in l || "y" in l || "width" in l || "height" in l ? { x: Number((e = l.x) != null ? e : 0), y: Number((n = l.y) != null ? n : 0), w: Number((i = (r = l.width) != null ? r : l.w) != null ? i : 0), h: Number((s = (o = l.height) != null ? o : l.h) != null ? s : 0) } : We(l);
    } return String(t); }
    function On(t) { if (t != null)
        return typeof t == "symbol" ? t.toString() : String(t); }
    function hi(t) { if (t != null)
        return typeof t == "function" ? { type: "function", name: t.name || void 0, arity: t.length } : typeof t == "object" ? { id: he(t), type: We(t) } : { type: typeof t }; }
    function ko(t) { if (t != null)
        return typeof t == "object" ? { id: he(t), type: We(t) } : typeof t == "function" ? { type: "function" } : { type: typeof t }; }
    function So(t) { var e = { event: On(t[0]), listener: hi(t[1]) }; return t.length > 2 && (e.context = ko(t[2])), [e]; }
    function Io(t) { return String(t != null ? t : "").slice(0, 240); }
    function Po(t) {
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
    function Co(t) { var s, l, a, d, f, m; if (!t || typeof t != "object")
        return; var e = t, n = Number((s = e.x) != null ? s : 0), r = Number((l = e.y) != null ? l : 0), i = Number((d = (a = e.width) != null ? a : e.w) != null ? d : 0), o = Number((m = (f = e.height) != null ? f : e.h) != null ? m : 0); if (!(!Number.isFinite(n) || !Number.isFinite(r) || !Number.isFinite(i) || !Number.isFinite(o)) && !(i <= 0 || o <= 0))
        return { x: n, y: r, w: i, h: o }; }
    function Oo(t, e) { if (e) {
        if (t === "addChild" || t === "removeChild")
            return e.map(function (n) { return n && typeof n == "object" ? he(n) : 0; });
        if (t === "mask") {
            var n = e[0];
            return [n && typeof n == "object" ? he(n) : 0];
        }
        if (t === "addChildAt" || t === "setChildIndex") {
            var n = e[0];
            return [n && typeof n == "object" ? he(n) : 0, Number(e[1]) || 0];
        }
        return t === "on" ? So(e) : t === "snapshot" ? e : t === "text.text.set" ? e.length ? [Io(e[0])] : [] : t === "text.style.set" ? e.length ? [Po(e[0])] : [] : e.map(an);
    } }
    function ln(t, e, n) { var r, i; try {
        window.__TRUEOS_PIXI_CAPTURE_STEP__ = "record:".concat(e, ":begin");
        var o = window.__pixiCapture;
        if (!(o != null && o.enabled))
            return;
        o.counts[e] = ((r = o.counts[e]) != null ? r : 0) + 1;
        var s = { frame: di, seq: ++Mo, op: e, id: t && typeof t == "object" ? he(t) : void 0, target: Eo(t), event: e === "on" && (n != null && n.length) ? On(n[0]) : void 0, listener: e === "on" && (n != null && n.length) ? hi(n[1]) : void 0, args: Oo(e, n) };
        window.__TRUEOS_PIXI_CAPTURE_STEP__ = "record:".concat(e, ":push"), o.commands.push(s), o.persist && Ro(s), o.commands.length > li && o.commands.splice(0, o.commands.length - li), window.__TRUEOS_PIXI_CAPTURE_STEP__ = "record:".concat(e, ":done");
    }
    catch (o) {
        try {
            window.__TRUEOS_PIXI_CAPTURE_ERROR__ = "record:".concat(e, ":").concat(String((i = o == null ? void 0 : o.message) != null ? i : o));
        }
        catch (s) { }
    } }
    function Ro(t) { if (we.push(t), t.op === "snapshot") {
        Be();
        return;
    } if (we.length >= 512) {
        Be();
        return;
    } Pn == null && (Pn = window.setTimeout(function () { Pn = null, Be(); }, 50)); }
    function Be() {
        if (we.length === 0)
            return;
        var t = we;
        we = [];
        var e = t.map(function (n) { return JSON.stringify(n); }).join("\n") + "\n";
        navigator.sendBeacon && navigator.sendBeacon("/__pixi_capture", new Blob([e], { type: "application/x-ndjson" })) || fetch("/__pixi_capture", { method: "POST", headers: { "Content-Type": "application/x-ndjson" }, body: e, keepalive: !0 }).catch(function () { we = t.concat(we); });
    }
    function Do(t, e, n) {
        var e_26, _a, e_27, _b, e_28, _c;
        var r, i;
        if (e === "on") {
            var o = On(n[0]), s = n[1];
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
    function Ao() { window.__TRUEOS_DISPATCH_PIXI_POINTER__ = function (t, e, n, r, i, o, s) {
        var e_29, _a;
        if (s === void 0) { s = 0; }
        var M, g, y, E, _, w, G, A, $, v, k, T, H, U;
        var l = function (L) { try {
            window.__TRUEOS_PIXI_POINTER_DISPATCH_STEP__ = L, typeof console != "undefined" && typeof console.log == "function" && console.log("[trueos pointer dispatch] ".concat(L));
        }
        catch (h) { } };
        l("start node=".concat(Number(t) || 0, " event=").concat(String(e || "")));
        var a = window.__TRUEOS_PIXI_APP;
        if (String(e || "") === "wheel") {
            var L = a == null ? void 0 : a.canvas;
            if (!L || typeof L.dispatchEvent != "function")
                return l("wheel-canvas-missing"), { handled: 0, listenerCount: 0, painted: 0, targetFound: 0 };
            var h = (y = (g = (M = window.__pixiCapture) == null ? void 0 : M.commands) == null ? void 0 : g.length) != null ? y : 0, I = { type: "wheel", deltaX: 0, deltaY: Number(s) || 0, deltaMode: 0, offsetX: Number(n) || 0, offsetY: Number(r) || 0, clientX: Number(n) || 0, clientY: Number(r) || 0, pointerId: Number(i) || 1, buttons: Number(o) || 0, defaultPrevented: !1, propagationStopped: !1, preventDefault: function () { this.defaultPrevented = !0; }, stopPropagation: function () { this.propagationStopped = !0; } };
            l("wheel-dispatch deltaY=".concat(I.deltaY)), L.dispatchEvent(I);
            var B = 0;
            if (window.__TRUEOS_CAPTURE_ONLY__) {
                var j = window.__TRUEOS_REPAINT_NOW__;
                window.__TRUEOS_PIXI_DIRTY__ && typeof j == "function" && (l("wheel-repaint-call"), j(), l("wheel-repaint-return"), B = 1);
            }
            else
                (E = a == null ? void 0 : a.renderer) != null && E.render && (a != null && a.stage) && (a.renderer.render(a.stage), B = 1);
            var q = (G = (w = (_ = window.__pixiCapture) == null ? void 0 : _.commands) == null ? void 0 : w.length) != null ? G : h, Z = (A = L.listeners) == null ? void 0 : A.wheel, tt = Array.isArray(Z) ? Z.length : typeof Z == "function" ? 1 : 0, Y = I.defaultPrevented || tt > 0 ? 1 : 0;
            return l("wheel-done handled=".concat(Y, " listeners=").concat(tt, " painted=").concat(B)), { handled: Y, listenerCount: tt, painted: q > h || B ? 1 : 0, targetFound: 1 };
        }
        var d = ui.get(Number(t) || 0), f = 0, m = 0, b = 0;
        if (!d)
            return l("target-missing"), { handled: f, listenerCount: m, painted: b, targetFound: 0 };
        var c = { type: String(e || ""), button: Number(o) & 2 ? 2 : 0, buttons: Number(o) || 0, pointerId: Number(i) || 1, pointerType: "mouse", global: { x: Number(n) || 0, y: Number(r) || 0 }, data: { pointerId: Number(i) || 1, pointerType: "mouse", global: { x: Number(n) || 0, y: Number(r) || 0 } }, target: d, currentTarget: d, defaultPrevented: !1, propagationStopped: !1, preventDefault: function () { this.defaultPrevented = !0; }, stopPropagation: function () { this.propagationStopped = !0; } };
        l("target-found label=".concat(String(($ = d.label) != null ? $ : "")));
        for (var L = d; L; L = L.parent) {
            c.currentTarget = L;
            var h = (v = L.listeners) == null ? void 0 : v[c.type];
            if (!(!Array.isArray(h) || h.length === 0)) {
                m += h.length, l("listeners node=".concat((k = Fe.get(L)) != null ? k : 0, " count=").concat(h.length));
                try {
                    for (var _b = (e_29 = void 0, __values(h.slice())), _c = _b.next(); !_c.done; _c = _b.next()) {
                        var I = _c.value;
                        if (typeof I == "function" && (f = 1, l("listener-call node=".concat((T = Fe.get(L)) != null ? T : 0)), I.call(L, c), l("listener-return node=".concat((H = Fe.get(L)) != null ? H : 0)), c.propagationStopped))
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
            var L = window.__TRUEOS_REPAINT_NOW__;
            window.__TRUEOS_PIXI_DIRTY__ && typeof L == "function" && (l("capture-repaint-call"), L(), l("capture-repaint-return"), b = 1);
        }
        else
            (U = a == null ? void 0 : a.renderer) != null && U.render && (a != null && a.stage) && (l("paint-call"), a.renderer.render(a.stage), l("paint-return"), b = 1);
        return l("done handled=".concat(f, " listeners=").concat(m, " painted=").concat(b)), { handled: f, listenerCount: m, painted: b, targetFound: 1 };
    }; }
    function Cn(t, e, n) {
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
                var a = Do(this, n, s);
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
    function No(t, e) { var n = t; for (; n;) {
        var r = Object.getOwnPropertyDescriptor(n, e);
        if (r)
            return r;
        n = Object.getPrototypeOf(n);
    } }
    function Ie(t, e, n) { var o, s; if (!(t != null && t.constructor) || t.constructor["__pixiCapturePatched_".concat(n)])
        return; var r = No(t, e); if ((r == null ? void 0 : r.configurable) === !1 || r && !r.set && !r.writable)
        return; var i = typeof Symbol == "function" ? Symbol("pixiCapture:".concat(n)) : "__pixiCaptureValue_".concat(n); Object.defineProperty(t, e, { configurable: (o = r == null ? void 0 : r.configurable) != null ? o : !0, enumerable: (s = r == null ? void 0 : r.enumerable) != null ? s : !0, get: r != null && r.get ? function () { var a; return (a = r.get) == null ? void 0 : a.call(this); } : function () { var a = this; return Object.prototype.hasOwnProperty.call(a, i) ? a[i] : r && "value" in r ? r.value : void 0; }, set: function (a) { if (ln(this, n, [a]), !window.__TRUEOS_CAPTURE_ONLY__) {
            r != null && r.set ? r.set.call(this, a) : Object.defineProperty(this, i, { configurable: !0, enumerable: !1, writable: !0, value: a });
            return;
        } var d = this; n === "text.text.set" ? d._text = String(a != null ? a : "") : n === "text.style.set" ? d._style = a != null ? a : {} : n === "text.resolution.set" ? d._resolution = Math.max(1, Number(a) || 1) : Object.defineProperty(d, i, { configurable: !0, enumerable: !1, writable: !0, value: a }); } }), t.constructor["__pixiCapturePatched_".concat(n)] = !0; }
    function mi(t, e) {
        if (e === void 0) { e = 0; }
        var s, l, a, d, f, m, b, c, M;
        if (!t || e > 64)
            return null;
        var n, r;
        try {
            var g = typeof t.getGlobalPosition == "function" ? t.getGlobalPosition() : null;
            g && Number.isFinite(Number(g.x)) && Number.isFinite(Number(g.y)) && (n = Number(g.x), r = Number(g.y));
        }
        catch (g) { }
        var i = { id: he(t), type: We(t), label: (s = t.label) != null ? s : void 0, x: (d = (a = (l = t.position) == null ? void 0 : l.x) != null ? a : t.x) != null ? d : 0, y: (b = (m = (f = t.position) == null ? void 0 : f.y) != null ? m : t.y) != null ? b : 0, globalX: n, globalY: r, scaleX: Number.isFinite(Number((c = t.scale) == null ? void 0 : c.x)) ? Number(t.scale.x) : 1, scaleY: Number.isFinite(Number((M = t.scale) == null ? void 0 : M.y)) ? Number(t.scale.y) : 1, visible: t.visible, alpha: Number.isFinite(Number(t.alpha)) ? Number(t.alpha) : 1, maskId: t.mask ? he(t.mask) : 0, zIndex: Number(t.zIndex) || 0, sortableChildren: t.sortableChildren === !0 }, o = Co(t.hitArea);
        return o && (i.hitArea = o), typeof t.text == "string" && (i.text = t.text.slice(0, 120)), Array.isArray(t.children) && t.children.length && (i.children = t.children.map(function (g) { return mi(g, e + 1); })), i;
    }
    function fi() {
        var e_30, _a, e_31, _b;
        if (window.__pixiCapture)
            return window.__pixiCapture;
        var t = { enabled: !0, persist: !window.__TRUEOS_CAPTURE_ONLY__, commands: [], counts: Object.create(null), clear: function () { this.commands.length = 0, this.counts = Object.create(null); }, dump: function (e) {
                if (e === void 0) { e = 200; }
                return this.commands.slice(-e);
            }, flush: function () { Be(); }, summary: function () { return re({}, this.counts); } };
        if (window.__pixiCapture = t, Ao(), window.addEventListener("beforeunload", function () { return Be(); }), !ci) {
            ci = !0, typeof wt.prototype.image != "function" && (wt.prototype.image = function () { return this; });
            try {
                for (var _c = __values(["clear", "rect", "roundRect", "circle", "ellipse", "moveTo", "lineTo", "closePath", "poly", "fill", "stroke", "image", "svg"]), _d = _c.next(); !_d.done; _d = _c.next()) {
                    var e = _d.value;
                    Cn(wt.prototype, e);
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
                    Cn(_t.prototype, e);
                }
            }
            catch (e_31_1) { e_31 = { error: e_31_1 }; }
            finally {
                try {
                    if (_g && !_g.done && (_b = _f.return)) _b.call(_f);
                }
                finally { if (e_31) throw e_31.error; }
            }
            Ie(Qt.prototype, "text", "text.text.set"), Ie(Qt.prototype, "style", "text.style.set"), Ie(Qt.prototype, "resolution", "text.resolution.set"), Cn(Qt.prototype, "setSize", "text.setSize"), Ie(_t.prototype, "visible", "visible"), Ie(_t.prototype, "alpha", "alpha"), Ie(_t.prototype, "mask", "mask");
        }
        return t;
    }
    function pi(t) { var e = t.renderer, n = e == null ? void 0 : e.render; if (typeof n != "function" || n.__pixiCapturePatched)
        return; var r = function (o) { var s = o && typeof o == "object" && "container" in o ? o.container : o || t.stage; return di++, ln(s, "render", []), ln(s, "snapshot", [mi(s)]), window.__TRUEOS_CAPTURE_ONLY__ ? s : n.call(this, o); }; r.__pixiCapturePatched = !0, e.render = r; }
    fi();
    var rt = null, An = 6, Pe = 10, Wt = 1, Ht = 3, Ut = 4, Ce = 512, _i = new Map;
    var u = { focusedKeyByPointer: new Map, keyboardOwnerPointerId: 1, inputs: new Map, sliders: new Map, sliderDrags: new Map, sliderBounds: new Map, dialogs: new Map, dialogDrags: new Map, dialogSelectedBy: new Map, dialogZ: new Map, dialogZCounter: 1, numbers: new Map, numberHolds: new Map, selects: new Map, temporals: new Map, temporalYearOwners: new Map, color: { rgb: { r: 255, g: 0, b: 0 }, a: 255, pick: null, draggingPointerId: null, bounds: null }, cursorColors: new Map, primaryMousePointerId: 1, harness: { enabled: !0, activeUserPointerId: Wt, periodMs: 3e3 }, userCursorPos: new Map, lastMouse: { x: 0, y: 0, has: !1 }, scroll: { y: 0, contentHeight: 0, viewportHeight: 0, draggingPointerId: null, dragOffsetY: 0, track: { x: 0, y: 0, w: Pe, h: 0 }, thumb: { x: 0, y: 0, w: Pe, h: 0 } }, iframeScroll: new Map, iframeRects: [], hoverRects: [], hoverHandlers: new Map, hoveredKeyByPointer: new Map, hoveredCursorByPointer: new Map, virtualCursor: { enabled: !1, x: 0, y: 0, t: 0, radius: 120, speed: .9 }, textDrags: new Map, fieldBounds: new Map, dialogDragBounds: new Map, detailsOpen: new Map, contextMenus: new Map, clipboards: new Map }, cn = null, Nn = 0;
    function Go(t) { if (!cn) {
        var n = document.createElement("canvas").getContext("2d");
        if (!n)
            throw new Error("2D canvas not available");
        cn = n;
    } return cn.font = "".concat(t.fontSize, "px ").concat(t.fontFamily), function (e) { return (Nn += 1, cn.measureText(e).width); }; }
    function Rn(t, e) {
        if (e === void 0) { e = 16; }
        return Object.entries(t).sort(function (n, r) { return r[1] - n[1] || (n[0] < r[0] ? -1 : n[0] > r[0] ? 1 : 0); }).slice(0, e).map(function (_a) {
            var _b = __read(_a, 2), n = _b[0], r = _b[1];
            return "".concat(n, ":").concat(r);
        }).join(",");
    }
    function Ue(t) { var e = typeof t == "string" ? t : ""; return e.indexOf("<truesurfer-") >= 0 && (e = e.replace(/<truesurfer-[A-Za-z0-9._-]+>/g, "")), e; }
    function Lo(t, e) { if (e >= t.length)
        return !0; var n = t.charCodeAt(e); return n === 95 || n === 40 || n === 91 || n === 123 || n === 34 || n === 39 || n >= 48 && n <= 57 || n >= 65 && n <= 90; }
    function Ti(t) { var e = t, n = !0; for (; n;) {
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
        r >= 2 && Lo(e, r) && (e = e.slice(r), n = !0);
    } return e; }
    function Fo(t) { var e = Ue(t), n = e.indexOf("__trueos") >= 0 || e.indexOf("tsNu") >= 0 || e.indexOf("tsNum") >= 0; return e.indexOf("__TRUEOS_HOST_READY__") >= 0 && (e = e.replace(/__TRUEOS_HOST_READY__/g, "")), e.indexOf("__trueos") >= 0 && (e = Bo(e), e = e.replace(/__trueosNumberValue/g, "").replace(/__trueosHostNum/g, "").replace(/__trueosNum/g, "").replace(/__trueosNu/g, "").replace(/__trueos/g, "")), (e.indexOf("tsNu") >= 0 || e.indexOf("tsNum") >= 0) && (e = e.replace(/tsNum/g, "").replace(/tsNutsNutsNutsNu/g, "").replace(/tsNutsNutsNu/g, "").replace(/tsNutsNu/g, "").replace(/tsNu/g, "")), n && (e = Ti(e.trimStart())), e; }
    function Bo(t) { var e = "__trueosN", n = t, r = 0; for (; r < n.length;) {
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
    function Mi(t) { return Fo(t); }
    function vn(t) { return Ti(Mi(t).trimStart()); }
    function Wo(t) { var e = me(vn(t)); return !(e.length === 0 || e === "true" || e === "false" || e === "N" || e === "Nu" || e === "Num" || e.startsWith("<truesurfer-") || e.startsWith("__trueo")); }
    function Ei(t, e) { var r; var n = Ue(e) || "block"; t[n] = ((r = t[n]) != null ? r : 0) + 1; }
    function Ho(t) {
        var e_32, _a;
        var e = { nodes: 0, blocks: 0, text: 0, maxDepth: 0, tags: {} }, n = function (r, i) {
            var e_33, _a;
            if (e.nodes += 1, e.maxDepth = Math.max(e.maxDepth, i), r.kind === "text") {
                e.text += 1;
                return;
            }
            e.blocks += 1, Ei(e.tags, r.tagName);
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
    function $o(t) { var e = { nodes: 0, blocks: 0, text: 0, maxDepth: 0, tags: {} }, n = function (r, i) {
        var e_34, _a;
        var o;
        e.nodes += 1, e.maxDepth = Math.max(e.maxDepth, i), r.kind === "text" ? e.text += 1 : (e.blocks += 1, Ei(e.tags, (o = r.tagName) != null ? o : "block"));
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
    function Gn(t, e) {
        if (e === void 0) { e = 64; }
        var n = me(Mi(t)), r = "";
        for (var i = 0; i < n.length && r.length < e; i += 1) {
            var o = n.charAt(i);
            r += o === "|" || o === '"' || o === "\\" ? "_" : o;
        }
        return r;
    }
    function hn(t, e) {
        if (e === void 0) { e = 120; }
        var n = "";
        for (var r = 0; r < t.length && n.length < e; r += 1) {
            var i = t.charAt(r);
            n += i === "\r" || i === "\n" || i === "	" || i === "|" || i === '"' || i === "\\" ? "_" : i;
        }
        return n;
    }
    function Uo(t, e) {
        if (e === void 0) { e = 12; }
        var n = [], r = function (i, o, s) { if (n.length >= e)
            return; if (i.kind === "text") {
            n.push("#".concat(n.length, "@").concat(o, ":").concat(s, " chars=").concat(i.text.length, " sample=\"").concat(Gn(i.text), "\""));
            return;
        } var l = Ue(i.tagName || "block") || "block", a = i.key || ""; for (var d = 0; d < i.children.length; d += 1)
            r(i.children[d], l, a); };
        for (var i = 0; i < t.length; i += 1)
            r(t[i], "root", "");
        return n.join("|");
    }
    function Xo(t, e) {
        if (e === void 0) { e = 12; }
        var n = [], r = function (i, o, s) { var d; if (n.length >= e)
            return; if (i.kind === "text") {
            var f = (d = i.text) != null ? d : "";
            n.push("#".concat(n.length, "@").concat(o, ":").concat(s, " chars=").concat(f.length, " box=").concat(Math.round(i.x), ",").concat(Math.round(i.y), ",").concat(Math.round(i.width), ",").concat(Math.round(i.height), " sample=\"").concat(Gn(f), "\""));
            return;
        } var l = Ue(i.tagName || "block") || "block", a = i.key || ""; for (var f = 0; f < i.children.length; f += 1)
            r(i.children[f], l, a); };
        return r(t, "root", ""), n.join("|");
    }
    function mn(t) { return (typeof t == "string" ? t : "").replace(/&quot;/g, '"').replace(/&#34;/g, '"').replace(/&#39;/g, "'").replace(/&apos;/g, "'").replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&amp;/g, "&"); }
    function Dn(t) { return me(mn((typeof t == "string" ? t : "").replace(/<[^>]*>/g, " "))); }
    function Yo(t) { var e = 0, n = String(t != null ? t : ""); for (; e < n.length && n.charCodeAt(e) <= 32;)
        e += 1; for (n.charAt(e) === "/" && (e += 1); e < n.length && n.charCodeAt(e) <= 32;)
        e += 1; var r = e; for (; e < n.length;) {
        var i = n.charCodeAt(e);
        if (!(i >= 48 && i <= 57 || i >= 65 && i <= 90 || i >= 97 && i <= 122 || i === 45 || i === 58))
            break;
        e += 1;
    } return n.slice(r, e).toLowerCase(); }
    function Ko(t) { return t === "h1" || t === "h2" || t === "h3" || t === "h4" || t === "h5" || t === "h6" || t === "summary" || t === "p" || t === "button" || t === "label" || t === "legend" || t === "option"; }
    function ki(t) { var e = typeof t == "string" ? t : "", n = [], r = function (f) { var m = Dn(f); m.length !== 0 && (m.startsWith("<truesurfer-") || m.startsWith("__trueo") || n.push(m)); }, i = [], o = e.toLowerCase(), s = o.indexOf("<body"); if (s >= 0) {
        var f = e.indexOf(">", s);
        s = f >= 0 ? f + 1 : s;
    }
    else
        s = 0; var l = o.indexOf("</body>", s), a = l >= 0 ? l : e.length, d = ""; for (; s < a && n.length < Ce;) {
        var f = e.charAt(s);
        if (f !== "<") {
            d += f, s += 1;
            continue;
        }
        var m = mn(d);
        if (m.length > 0) {
            for (var w = i.length - 1; w >= 0; w -= 1)
                if (i[w].wanted) {
                    i[w].text += " ".concat(m);
                    break;
                }
        }
        d = "";
        var b = e.indexOf(">", s + 1);
        if (b < 0)
            break;
        var c = e.slice(s, b + 1), M = e.slice(s + 1, b), g = Yo(M);
        if (M.trimStart().charAt(0) === "/") {
            for (var w = i.length - 1; w >= 0; w -= 1) {
                var G = i.pop();
                if (G != null && G.wanted && r(G.text), (G == null ? void 0 : G.tag) === g)
                    break;
            }
            s = b + 1;
            continue;
        }
        if (g === "script" || g === "style" || g === "template") {
            var w = "</".concat(g, ">"), G = o.indexOf(w, b + 1);
            s = G >= 0 ? G + w.length : b + 1;
            continue;
        }
        if (g === "input") {
            var w = xi(c, "type").toLowerCase();
            (w === "button" || w === "submit" || w === "reset") && r(xi(c, "value"));
        }
        var E = c.length - 1;
        for (; E >= 0 && c.charCodeAt(E) <= 32;)
            E -= 1;
        E >= 1 && c.charAt(E) === ">" && c.charAt(E - 1) === "/" || g === "input" || g === "br" || g === "hr" || g === "img" || i.push({ tag: g, wanted: Ko(g), text: "" }), s = b + 1;
    } if (d.length > 0) {
        var f = mn(d);
        for (var m = i.length - 1; m >= 0; m -= 1)
            if (i[m].wanted) {
                i[m].text += " ".concat(f);
                break;
            }
    } for (; i.length && n.length < Ce;) {
        var f = i.pop();
        f != null && f.wanted && r(f.text);
    } if (n.length === 0) {
        var f = o.indexOf("<body");
        if (f >= 0) {
            var g = e.indexOf(">", f);
            f = g >= 0 ? g + 1 : f;
        }
        else
            f = 0;
        var m = o.indexOf("</body>", f), b = m >= 0 ? m : e.length, c = !1, M = "";
        for (var g = f; g < b && n.length < Ce; g += 1) {
            var y = e.charAt(g);
            if (y === "<") {
                r(M), M = "", c = !0;
                continue;
            }
            if (y === ">") {
                c = !1;
                continue;
            }
            c || (M += y);
        }
        r(M);
    } return n; }
    function fn(t) { var e = window == null ? void 0 : window[t]; return e !== void 0 ? e : globalThis == null ? void 0 : globalThis[t]; }
    function zo(t, e) {
        if (e === void 0) { e = 8; }
        var n = [];
        for (var r = 0; r < t.length && n.length < e; r += 1)
            n.push("#".concat(r, "=\"").concat(hn(t[r], 48), "\""));
        return n.join("|");
    }
    function xi(t, e) { var i, o, s; var r = new RegExp("".concat(e, "[ \\t\\r\\n\\f]*=[ \\t\\r\\n\\f]*(\"([^\"]*)\"|'([^']*)'|([^ \\t\\r\\n\\f>]+))"), "i").exec(t); return mn((s = (o = (i = r == null ? void 0 : r[2]) != null ? i : r == null ? void 0 : r[3]) != null ? o : r == null ? void 0 : r[4]) != null ? s : ""); }
    function $e(t) { var e = []; for (var n = 0; n < t.length; n += 1) {
        var r = t[n];
        typeof r == "string" && pn(e, r);
    } return e; }
    function jo(t) { var e = "", n = !1; for (var r = 0; r < t.length; r += 1) {
        var i = t.charCodeAt(r);
        if (i === 32 || i === 9 || i === 10 || i === 13 || i === 12) {
            n = !0;
            continue;
        }
        n && e.length > 0 && (e += " "), e += t.charAt(r), n = !1;
    } return e; }
    function pn(t, e) { var n = jo(e); if (n.length !== 0 && !(n.indexOf("<truesurfer-") === 0 || n.indexOf("__trueo") === 0)) {
        for (var r = 0; r < t.length; r += 1)
            if (t[r] === n)
                return;
        t.push(n);
    } }
    function Vo(t) {
        if (typeof t != "string" || t.length === 0)
            return [];
        var e = [], n = "";
        for (var r = 0; r < t.length; r += 1) {
            var i = t.charAt(r);
            if (i === "\r" || i === "\n") {
                pn(e, n), n = "", i === "\r" && t.charAt(r + 1) === "\n" && (r += 1);
                continue;
            }
            n += i;
        }
        return pn(e, n), e;
    }
    function Jo(t) { var e = []; if (!Array.isArray(t))
        return e; for (var n = 0; n < t.length; n += 1) {
        var r = t[n];
        typeof r == "string" && pn(e, r);
    } return e; }
    function Zo(t) { var e = []; if (!Array.isArray(t))
        return e; for (var n = 0; n < t.length; n += 1) {
        var r = t[n];
        typeof r != "string" || r.length === 0 || r.indexOf("<truesurfer-") === 0 || r.indexOf("__trueo") === 0 || (e[e.length] = r);
    } return e; }
    function Qo(t) {
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
    function qo(t) { var e = fn("__TRUEOS_WIDGET_TEXT_ROWS_TEXT__"), n = fn("__TRUEOS_WIDGET_TEXT_ROWS__"), r = Zo(n); if (r.length > 0)
        return { source: "array-trusted", rows: r }; var i = Qo(e); if (i.length > 0)
        return { source: "text-trusted", rows: i }; var o = Vo(e); if (o.length > 0)
        return { source: "text", rows: o }; var s = Jo(n); if (s.length > 0)
        return { source: "array", rows: s }; var l = ki(t); if (Kt()) {
        var a = Array.isArray(n) && typeof n[0] == "string" ? hn(n[0], 72) : "", d = typeof e == "string" ? hn(e, 72) : "";
        console.log("[trueos pixi widgets] text-fallback-globals text_type=".concat(typeof e, " text_len=").concat(typeof e == "string" ? e.length : 0, " text_rows=").concat(o.length, " text_sample=\"").concat(d, "\" array=").concat(Array.isArray(n) ? n.length : -1, " array_rows=").concat(s.length, " array0=\"").concat(a, "\" html_len=").concat(t.length, " html_rows=").concat(l.length));
    } return { source: "html", rows: l }; }
    function ts() { var e; var t = fn("__TRUEOS_WIDGET_RENDER_TREE_JSON__"); if (typeof t == "string" && t.length > 0)
        try {
            return { source: "json", tree: JSON.parse(t) };
        }
        catch (n) {
            Kt() && console.log("[trueos pixi widgets] render-tree-json parse failed err=".concat(String((e = n == null ? void 0 : n.message) != null ? e : n)));
        } return { source: "window", tree: fn("__TRUEOS_WIDGET_RENDER_TREE__") }; }
    function es(t) { var o, s, l, a; var e = [], n = String(t != null ? t : "").replace(/<script[^]*?<\/script>/gi, " ").replace(/<style[^]*?<\/style>/gi, " "), r = /<(h[1-6]|p|label|button)\b[^>]*>([^]*?)<\/\1>|<input\b[^>]*>/gi, i; for (; (i = r.exec(n)) && e.length < Ce;) {
        var d = (o = i[0]) != null ? o : "", f = String((s = i[1]) != null ? s : "").toLowerCase();
        if (d.toLowerCase().startsWith("<input"))
            continue;
        var m = Dn(f === "p" || f === "label" ? (l = i[2]) != null ? l : "" : (a = i[2]) != null ? a : "");
        m.length > 0 && e.push(m);
    } return e; }
    function ns(t) { var e = es(t), n = $e(e); return $e(n); }
    function rs(t, e, n, r) {
        var e_35, _a;
        var a, d, f, m, b, c;
        var i = $e((d = _i.get(String((a = t.key) != null ? a : ""))) != null ? d : []), o = $e(String((m = (f = t.attrs) == null ? void 0 : f["data-trueos-srcdoc-text"]) != null ? m : "").split("\n").map(function (M) { return me(M); })), s = i.length > 0 ? i : o.length > 0 ? o : ns(String((c = (b = t.attrs) == null ? void 0 : b.srcdoc) != null ? c : "")), l = n + 48;
        try {
            for (var s_2 = __values(s), s_2_1 = s_2.next(); !s_2_1.done; s_2_1 = s_2.next()) {
                var M = s_2_1.value;
                if (r.length >= Ce)
                    return;
                r.push({ x: e + 16, y: l, text: M }), l += 32;
            }
        }
        catch (e_35_1) { e_35 = { error: e_35_1 }; }
        finally {
            try {
                if (s_2_1 && !s_2_1.done && (_a = s_2.return)) _a.call(s_2);
            }
            finally { if (e_35) throw e_35.error; }
        }
    }
    function Ln(t) { var e; return t.kind === "text" ? (e = t.text) != null ? e : "" : t.children.map(Ln).join(" "); }
    function is(t) { var e = [], n = function (r, i, o, s) {
        var e_36, _a;
        var g, y, E;
        if (e.length >= Ce)
            return;
        var l = i + r.x, a = o + r.y, d = r.kind === "block" && r.tagName === "iframe" && String((y = (g = r.attrs) == null ? void 0 : g["data-root"]) != null ? y : "") !== "1", f = s + (d ? 1 : 0), m = r.kind === "block" && r.tagName === "button", b = r.kind === "text" ? (E = r.text) != null ? E : "" : m ? Ln(r) : "", c = me(vn(b)), M = e.length;
        if (Wo(c)) {
            var _ = m ? l + 8 : l, w = m ? a + Math.max(0, Math.floor((r.height - be.fontSize * 1.25) / 2)) : a;
            e.push({ x: _, y: w, text: c });
        }
        if (!m) {
            try {
                for (var _b = __values(r.children), _c = _b.next(); !_c.done; _c = _b.next()) {
                    var _ = _c.value;
                    n(_, l, a, f);
                }
            }
            catch (e_36_1) { e_36 = { error: e_36_1 }; }
            finally {
                try {
                    if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
                }
                finally { if (e_36) throw e_36.error; }
            }
            d && e.length === M && rs(r, l, a, e);
        }
    }; return n(t, 0, 0, 0), e; }
    function os(t, e) {
        if (e === void 0) { e = 8; }
        var n = [];
        for (var r = 0; r < t.length && n.length < e; r += 1) {
            var i = t[r];
            n.push("#".concat(n.length, " x=").concat(Math.round(i.x), " y=").concat(Math.round(i.y), " text=\"").concat(Gn(i.text), "\""));
        }
        return n.join("|");
    }
    function ss() {
        var e_37, _a;
        var i, o, s, l;
        var t = (o = (i = window.__pixiCapture) == null ? void 0 : i.commands) != null ? o : [], e = {}, n = {}, r = new Set(["addChild", "addChildAt", "setChildIndex", "removeChild", "removeChildren", "removeAllListeners", "on", "clear", "rect", "roundRect", "circle", "ellipse", "moveTo", "lineTo", "closePath", "poly", "fill", "stroke", "image", "visible", "alpha", "scale", "mask", "text.text.set", "text.style.set", "text.resolution.set", "text.setSize", "render", "snapshot"]);
        try {
            for (var t_2 = __values(t), t_2_1 = t_2.next(); !t_2_1.done; t_2_1 = t_2.next()) {
                var a = t_2_1.value;
                var d = Ue(a == null ? void 0 : a.op);
                d && (e[d] = ((s = e[d]) != null ? s : 0) + 1, r.has(d) || (n[d] = ((l = n[d]) != null ? l : 0) + 1));
            }
        }
        catch (e_37_1) { e_37 = { error: e_37_1 }; }
        finally {
            try {
                if (t_2_1 && !t_2_1.done && (_a = t_2.return)) _a.call(t_2);
            }
            finally { if (e_37) throw e_37.error; }
        }
        return { total: t.length, ops: Rn(e, 24), unsupported: Rn(n, 24) };
    }
    function as(t, e, n, r) { if (!Kt())
        return; var i = ss(); window.__TRUEOS_PIXI_BRIDGE_STATS__ = { renderNodes: t.nodes, renderBlocks: t.blocks, renderText: t.text, renderTags: Rn(t.tags, 24), renderTextSamples: n, layoutBoxes: e.nodes, layoutBlocks: e.blocks, layoutText: e.text, layoutMaxDepth: e.maxDepth, layoutTextSamples: r, measureTextCalls: Nn, scrollbarVisible: u.scroll.track.h > 0 ? 1 : 0, scrollbarTrack: "".concat(Math.round(u.scroll.track.x), ",").concat(Math.round(u.scroll.track.y), ",").concat(Math.round(u.scroll.track.w), ",").concat(Math.round(u.scroll.track.h)), scrollbarThumb: "".concat(Math.round(u.scroll.thumb.x), ",").concat(Math.round(u.scroll.thumb.y), ",").concat(Math.round(u.scroll.thumb.w), ",").concat(Math.round(u.scroll.thumb.h)), pixiCommands: i.total, pixiOps: i.ops, pixiUnsupported: i.unsupported }; }
    var yi = new WeakMap;
    function Fn(t, e) { var n = t; for (; n;) {
        if (n === e)
            return !0;
        n = n.parent;
    } return !1; }
    function Si(t) { return Array.isArray(t.children) || (t.children = []), t.children; }
    function oe(t, e, n) { var r = Number(e) || 0, i = Number(n) || 0; (!t.position || typeof t.position != "object") && (t.position = { x: 0, y: 0 }), t.position.x = r, t.position.y = i; }
    function He(t, e, n) { if (e === t || Fn(t, e))
        return; var r = Si(t); if (e.parent !== t) {
        var l = Math.max(0, Math.min(n, r.length));
        t.addChildAt(e, l);
        return;
    } var i = Math.max(0, r.length - 1), o = Math.max(0, Math.min(n, i)); t.getChildIndex(e) !== o && t.setChildIndex(e, o); }
    function wi(t, e, n) { if (e === t || Fn(t, e))
        return; var r = Si(t); if (e.parent !== t) {
        var l = Math.max(0, Math.min(n, r.length));
        t.addChildAt(e, l);
        return;
    } var i = Math.max(0, r.length - 1), o = Math.max(0, Math.min(n, i)); t.getChildIndex(e) !== o && t.setChildIndex(e, o); }
    var dn = null, ot = null;
    function Vt(t) { var e = u.cursorColors.get(t); if (e != null)
        return e; var n = [1118481, 2450411, 1483594, 14427686, 8141549, 959977, 16096779], r = Math.abs(Number(t) || 0) % n.length, i = n[r]; return u.cursorColors.set(t, i), i; }
    function Ft(t) { var i, o, s, l, a, d; var e = Number((s = (o = t == null ? void 0 : t.pointerId) != null ? o : (i = t == null ? void 0 : t.data) == null ? void 0 : i.pointerId) != null ? s : 0), r = String((d = (a = t == null ? void 0 : t.pointerType) != null ? a : (l = t == null ? void 0 : t.data) == null ? void 0 : l.pointerType) != null ? d : "").toLowerCase() === "mouse" || e === 1 || e === u.primaryMousePointerId; return u.harness.enabled && r ? u.harness.activeUserPointerId : e; }
    function Kt() { return !!globalThis.__TRUEOS_CAPTURE_ONLY__; }
    function kt(t) { Kt() && (window.__TRUEOS_PIXI_APP_PHASE__ = t); }
    function F(t) { Kt() && (window.__TRUEOS_PIXI_LAYOUT_STEP__ = t); }
    function Ii(t) { var l, a, d, f, m; var e = (l = window.__TRUEOS_PIXI_APP_PHASE__) != null ? l : "unknown", n = (a = window.__TRUEOS_PIXI_LAYOUT_STEP__) != null ? a : "", r = t, i = String((d = r == null ? void 0 : r.name) != null ? d : "Error"), o = String((f = r == null ? void 0 : r.message) != null ? f : t), s = String((m = r == null ? void 0 : r.stack) != null ? m : ""); return "phase=".concat(e, " layout=").concat(n, " name=").concat(i, " message=").concat(o, " stack=").concat(s); }
    function ls() { var t = Math.max(1, Number(window.innerWidth || 1920) | 0), e = Math.max(1, Number(window.innerHeight || 1080) | 0), n = new gt(0, 0, t, e), r = document.createElement("canvas"), i = { width: t, height: e, screen: n, render: function (o) { return o; }, resize: function (o, s) { var l = Math.max(1, Number(o || t) | 0), a = Math.max(1, Number(s || e) | 0); this.width = l, this.height = a, n.width = l, n.height = a; } }; return { stage: new _t, screen: n, canvas: r, renderer: i, ticker: { stop: function () { }, add: function () { }, remove: function () { } } }; }
    function cs() { var l = /** @class */ (function () {
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
        l.prototype.setMargin = function (d, f) { var m = Number(f) || 0; d === 0 ? this.marginLeft = m : d === 1 ? this.marginTop = m : d === 2 ? this.marginRight = m : d === 3 && (this.marginBottom = m); };
        l.prototype.setPadding = function (d, f) { var m = Number(f) || 0; d === 0 ? this.paddingLeft = m : d === 1 ? this.paddingTop = m : d === 2 ? this.paddingRight = m : d === 3 && (this.paddingBottom = m); };
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
        l.prototype.layout = function (d, f, m, b) {
            var e_38, _a, e_39, _b;
            var c = this.paddingLeft + this.paddingRight, M = this.paddingTop + this.paddingBottom, g = Math.max(this.minWidth, this.width || m), y = Math.max(this.minHeight, this.height || 0);
            if (this.computed.left = d, this.computed.top = f, this.computed.width = g, this.measureFunc) {
                var E = this.measureFunc(Math.max(0, g - c), 0);
                y = Math.max(y, Math.ceil(Number(E.height) || 0) + M), this.computed.height = y;
                return;
            }
            if (this.flexDirection === 1) {
                var E = this.paddingLeft, _ = 0;
                try {
                    for (var _c = __values(this.children), _d = _c.next(); !_d.done; _d = _c.next()) {
                        var w = _d.value;
                        var G = w.width || w.minWidth || Math.max(24, (g - c) / Math.max(1, this.children.length));
                        w.layout(E + w.marginLeft, this.paddingTop + w.marginTop, G, b), E += w.computed.width + w.marginLeft + w.marginRight, _ = Math.max(_, w.computed.height + w.marginTop + w.marginBottom);
                    }
                }
                catch (e_38_1) { e_38 = { error: e_38_1 }; }
                finally {
                    try {
                        if (_d && !_d.done && (_a = _c.return)) _a.call(_c);
                    }
                    finally { if (e_38) throw e_38.error; }
                }
                y = Math.max(y, _ + M);
            }
            else {
                var E = this.paddingTop;
                try {
                    for (var _f = __values(this.children), _g = _f.next(); !_g.done; _g = _f.next()) {
                        var _ = _g.value;
                        var w = Math.max(0, g - c - _.marginLeft - _.marginRight);
                        _.layout(this.paddingLeft + _.marginLeft, E + _.marginTop, w, b), E += _.computed.height + _.marginTop + _.marginBottom;
                    }
                }
                catch (e_39_1) { e_39 = { error: e_39_1 }; }
                finally {
                    try {
                        if (_g && !_g.done && (_b = _f.return)) _b.call(_f);
                    }
                    finally { if (e_39) throw e_39.error; }
                }
                y = Math.max(y, E + this.paddingBottom);
            }
            this.computed.height = Math.max(this.minHeight, y);
        };
        return l;
    }()); return { Node: l, EDGE_LEFT: 0, EDGE_TOP: 1, EDGE_RIGHT: 2, EDGE_BOTTOM: 3, FLEX_DIRECTION_COLUMN: 0, FLEX_DIRECTION_ROW: 1, FLEX_DIRECTION_ROW_REVERSE: 1, ALIGN_STRETCH: 0, ALIGN_CENTER: 1, ALIGN_FLEX_START: 2, JUSTIFY_CENTER: 0, JUSTIFY_FLEX_START: 1, JUSTIFY_SPACE_BETWEEN: 2, WRAP_WRAP: 1, WRAP_NO_WRAP: 0, POSITION_TYPE_ABSOLUTE: 1, DIRECTION_LTR: 0, MEASURE_MODE_UNDEFINED: 0 }; }
    function us(t) {
        var e_40, _a;
        var r;
        var e = 0, n = function (i, o, s) {
            var e_41, _a;
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
                catch (e_41_1) { e_41 = { error: e_41_1 }; }
                finally {
                    try {
                        if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
                    }
                    finally { if (e_41) throw e_41.error; }
                }
            }
        };
        try {
            for (var _b = __values((r = t.children) != null ? r : []), _c = _b.next(); !_c.done; _c = _b.next()) {
                var i = _c.value;
                n(i, 0, 0);
            }
        }
        catch (e_40_1) { e_40 = { error: e_40_1 }; }
        finally {
            try {
                if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
            }
            finally { if (e_40) throw e_40.error; }
        }
        return e;
    }
    function un(t, e) { var o, s, l, a; var n = u.inputs.get(t); if (n)
        return n; var r = {}, i = ((o = e == null ? void 0 : e.type) != null ? o : "text").toLowerCase(); if (i === "checkbox" || i === "radio") {
        if (r.checked = e ? Object.prototype.hasOwnProperty.call(e, "checked") : !1, i === "checkbox") {
            var d = ((s = e == null ? void 0 : e["aria-checked"]) != null ? s : "").toLowerCase(), f = ((l = e == null ? void 0 : e["data-indeterminate"]) != null ? l : "").toLowerCase();
            r.indeterminate = (e ? Object.prototype.hasOwnProperty.call(e, "indeterminate") : !1) || d === "mixed" || f === "true" || f === "1" || f === "yes";
        }
    }
    else
        r.value = (a = e == null ? void 0 : e.value) != null ? a : ""; return u.inputs.set(t, r), r; }
    function ds(t) { var e = new Map; function n(r) {
        var e_42, _a;
        var i, o, s, l, a;
        if (r.kind === "block" && r.tagName === "input" && ((o = (i = r.attrs) == null ? void 0 : i.type) != null ? o : "text").toLowerCase() === "radio") {
            var m = "radio:".concat((l = (s = r.attrs) == null ? void 0 : s.name) != null ? l : "__default__"), b = r.key;
            if (b) {
                var c = (a = e.get(m)) != null ? a : [];
                c.push(b), e.set(m, c);
            }
        }
        try {
            for (var _b = __values(r.children), _c = _b.next(); !_c.done; _c = _b.next()) {
                var d = _c.value;
                n(d);
            }
        }
        catch (e_42_1) { e_42 = { error: e_42_1 }; }
        finally {
            try {
                if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
            }
            finally { if (e_42) throw e_42.error; }
        }
    } return n(t), e; }
    function me(t) { var e = "", n = !1, r = typeof t == "string" ? t : ""; for (var i = 0; i < r.length; i += 1) {
        var o = r.charCodeAt(i);
        if (o === 32 || o === 9 || o === 10 || o === 13 || o === 12) {
            n = !0;
            continue;
        }
        n && e.length > 0 && (e += " "), e += r.charAt(i), n = !1;
    } return e; }
    function hs(t) {
        var e_43, _a;
        if (!t || typeof t != "object")
            return;
        var e = {};
        try {
            for (var _b = __values(Object.entries(t)), _c = _b.next(); !_c.done; _c = _b.next()) {
                var _d = __read(_c.value, 2), n = _d[0], r = _d[1];
                typeof n != "string" || n.length === 0 || (e[n] = typeof r == "string" ? r : "");
            }
        }
        catch (e_43_1) { e_43 = { error: e_43_1 }; }
        finally {
            try {
                if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
            }
            finally { if (e_43) throw e_43.error; }
        }
        return Object.keys(e).length > 0 ? e : void 0;
    }
    function Pi(t, e, n) { var d, f; if (!t || typeof t != "object")
        return null; var r = t, i = typeof r.kind == "string" ? r.kind : ""; if (i === "text") {
        var m = typeof r.text == "string" ? r.text : "", b = "", c = (d = n == null ? void 0 : n.rows[n.index]) != null ? d : "", M = !1;
        if (n && n.index < n.rows.length ? (n.index += 1, b = c, M = !0) : b = me(vn(m)), !M && (m.indexOf("<truesurfer-") >= 0 || m.indexOf("__trueo") >= 0) || b.startsWith("<truesurfer-") || b.startsWith("__trueo"))
            b = "";
        else if (b.length === 0) {
            var y = (f = n == null ? void 0 : n.rows[n.index]) != null ? f : "";
            n && y && (n.index += 1), y && (b = y);
        }
        return b.length > 0 ? { kind: "text", text: b } : null;
    } if (i !== "block")
        return null; var o = typeof r.tagName == "string" ? r.tagName.toLowerCase() : ""; if (o.length === 0)
        return null; var s = typeof r.key == "string" ? r.key : "".concat(e, ":").concat(o), l = [], a = Array.isArray(r.children) ? r.children : []; for (var m = 0; m < a.length; m += 1) {
        var b = Pi(a[m], "".concat(e, ".").concat(m), n);
        b && l.push(b);
    } return { kind: "block", key: s, tagName: o, attrs: hs(r.attrs), children: l }; }
    function ms(t, e) { var n = Array.isArray(t) ? t : t && typeof t == "object" && Array.isArray(t.widgetRenderTree) ? t.widgetRenderTree : [], i = { rows: Array.isArray(e) ? $e(e) : ki(e), index: 0 }, o = []; for (var s = 0; s < n.length; s += 1) {
        var l = Pi(n[s], "0.".concat(s), i);
        l && o.push(l);
    } return o; }
    function fs(t) { var n = document.createElement("canvas").getContext("2d"); if (!n)
        throw new Error("2D canvas not available"); n.font = t; var r = t.indexOf("px"), i = r; for (; i > 0;) {
        var l = t.charCodeAt(i - 1);
        if (l < 48 || l > 57)
            break;
        i -= 1;
    } var o = r > i ? Number(t.slice(i, r)) : 16, s = Math.ceil(o * 1.25); return { measure: function (l, a) {
            var e_44, _a;
            Nn += 1;
            var d = me(l).split(" ").filter(Boolean);
            if (d.length === 0)
                return { width: 0, height: s, lines: [""] };
            var f = [], m = "";
            try {
                for (var d_1 = __values(d), d_1_1 = d_1.next(); !d_1_1.done; d_1_1 = d_1.next()) {
                    var M = d_1_1.value;
                    var g = m ? "".concat(m, " ").concat(M) : M, y = n.measureText(g).width, E = a != null ? a : Number.POSITIVE_INFINITY;
                    y <= E || !m ? m = g : (f.push(m), m = M);
                }
            }
            catch (e_44_1) { e_44 = { error: e_44_1 }; }
            finally {
                try {
                    if (d_1_1 && !d_1_1.done && (_a = d_1.return)) _a.call(d_1);
                }
                finally { if (e_44) throw e_44.error; }
            }
            m && f.push(m);
            var b = Math.min(Math.max.apply(Math, __spreadArray([], __read(f.map(function (M) { return n.measureText(M).width; })), false)), a != null ? a : Number.POSITIVE_INFINITY), c = f.length * s;
            return { width: Math.ceil(b), height: Math.ceil(c), lines: f };
        }, lineHeight: s, font: t }; }
    function ps(t, e, n) { var b; F("build:start nodes=".concat(t.length, " viewport=").concat(e, "x").concat(n)); var r = 12, i = 8, o = be; F("build:measurer"); var s = fs("".concat(o.fontSize, "px ").concat(o.fontFamily)); function l(c) { return c.kind !== "block" || c.tagName === "hr" || c.tagName === "tr" || c.tagName === "td" || c.tagName === "th" ? 0 : i; } function a(c) { var M = c.kind === "text" ? "text:".concat(c.text.slice(0, 24)) : "".concat(c.tagName, ":").concat(c.key); if (F("node:".concat(M, ":start")), c.kind === "text") {
        var _1 = rt.Node.create();
        return F("node:".concat(M, ":measure-func")), _1.setMeasureFunc(function (w, G) { F("node:".concat(M, ":measure-call")); var A = G === rt.MEASURE_MODE_UNDEFINED ? void 0 : Math.max(0, w), $ = s.measure(c.text, A); return { width: $.width, height: $.height }; }), _1.setMargin(rt.EDGE_RIGHT, 6), _1.setMargin(rt.EDGE_BOTTOM, 0), { yogaNode: _1, buildBox: function () { return ({ kind: "text", text: c.text, x: _1.getComputedLeft(), y: _1.getComputedTop(), width: _1.getComputedWidth(), height: _1.getComputedHeight(), children: [] }); } };
    } if (c.tagName === "sliderlabel")
        return F("node:".concat(c.tagName, ":").concat(c.key, ":sliderlabel")), rr({ node: c, Yoga: rt, measurer: s }); F("node:".concat(c.tagName, ":").concat(c.key, ":create")); var g = rt.Node.create(); if (F("node:".concat(c.tagName, ":").concat(c.key, ":base-defaults")), g.setFlexDirection(rt.FLEX_DIRECTION_COLUMN), g.setAlignItems(rt.ALIGN_STRETCH), g.setPadding(rt.EDGE_LEFT, r), g.setPadding(rt.EDGE_RIGHT, r), g.setPadding(rt.EDGE_TOP, r), g.setPadding(rt.EDGE_BOTTOM, r), g.setMargin(rt.EDGE_BOTTOM, 0), En(c.tagName) && (F("node:".concat(c.tagName, ":").concat(c.key, ":heading-defaults")), xr(g, rt)), c.tagName === "hr" && (F("node:".concat(c.tagName, ":").concat(c.key, ":hr-defaults")), ur(g, rt)), (c.tagName === "p" || c.tagName === "label") && (F("node:".concat(c.tagName, ":").concat(c.key, ":inline-scan")), c.children.some(function (w) { return w.kind === "block" && (w.tagName === "input" || w.tagName === "button" || w.tagName === "select" || w.tagName === "textarea" || w.tagName === "timeinput" || w.tagName === "dateinput" || w.tagName === "monthinput" || w.tagName === "weekinput" || w.tagName === "datetimelocalinput" || w.tagName === "progress" || w.tagName === "meter" || w.tagName === "slider" || w.tagName === "number" || w.tagName === "color"); }) && (g.setFlexDirection(rt.FLEX_DIRECTION_ROW), g.setFlexWrap(rt.WRAP_WRAP), g.setAlignItems(rt.ALIGN_CENTER)), g.setPadding(rt.EDGE_TOP, 4), g.setPadding(rt.EDGE_BOTTOM, 4), g.setPadding(rt.EDGE_LEFT, 4), g.setPadding(rt.EDGE_RIGHT, 4)), c.tagName === "table" && (F("node:".concat(c.tagName, ":").concat(c.key, ":table-defaults")), pr(g, rt)), c.tagName === "tr" && (F("node:".concat(c.tagName, ":").concat(c.key, ":tr-defaults")), gr(g, rt)), (c.tagName === "td" || c.tagName === "th") && (F("node:".concat(c.tagName, ":").concat(c.key, ":cell-defaults")), br(g, rt)), c.tagName === "input" && (F("node:".concat(c.tagName, ":").concat(c.key, ":input-defaults")), Br(g, c, rt)), c.tagName === "textarea" && (F("node:".concat(c.tagName, ":").concat(c.key, ":textarea-defaults")), Hr(g, rt)), c.tagName === "select" && (F("node:".concat(c.tagName, ":").concat(c.key, ":select-defaults")), ei(g, rt)), c.tagName === "timeinput" || c.tagName === "dateinput" || c.tagName === "monthinput" || c.tagName === "weekinput" || c.tagName === "datetimelocalinput") {
        var _ = c.tagName === "timeinput" ? "time" : c.tagName === "monthinput" ? "month" : c.tagName === "weekinput" ? "week" : c.tagName === "dateinput" ? "date" : "datetime-local";
        F("node:".concat(c.tagName, ":").concat(c.key, ":temporal-defaults")), ii(g, rt, _);
    } c.tagName === "img" && (F("node:".concat(c.tagName, ":").concat(c.key, ":img-defaults")), Ir(g, c, rt)), c.tagName === "svg" && (F("node:".concat(c.tagName, ":").concat(c.key, ":svg-defaults")), Ar(g, c, rt)), c.tagName === "canvas" && (F("node:".concat(c.tagName, ":").concat(c.key, ":canvas-defaults")), vr(g, c, rt)), c.tagName === "iframe" && (F("node:".concat(c.tagName, ":").concat(c.key, ":iframe-defaults")), Lr(g, c, rt)), c.tagName === "button" && (F("node:".concat(c.tagName, ":").concat(c.key, ":button-defaults")), hr(g, rt)), c.tagName === "dialog" && (F("node:".concat(c.tagName, ":").concat(c.key, ":dialog-defaults")), zr(g, rt)), c.tagName === "number" && (F("node:".concat(c.tagName, ":").concat(c.key, ":number-defaults")), Vr(g, rt)), c.tagName === "color" && (F("node:".concat(c.tagName, ":").concat(c.key, ":color-defaults")), Qr(g, c, rt)), c.tagName === "searchrow" && (F("node:".concat(c.tagName, ":").concat(c.key, ":searchrow-defaults")), Xr(g, rt)), c.tagName === "searchbutton" && (F("node:".concat(c.tagName, ":").concat(c.key, ":searchbutton-defaults")), Yr(g, rt)), c.tagName === "summary" && (F("node:".concat(c.tagName, ":").concat(c.key, ":summary-defaults")), sr(g, rt)), c.tagName === "details" && (F("node:".concat(c.tagName, ":").concat(c.key, ":details-defaults")), ar(g, rt)), c.tagName === "barrow" && (F("node:".concat(c.tagName, ":").concat(c.key, ":barrow-defaults")), Ur(g, rt)), (c.tagName === "progress" || c.tagName === "meter") && (F("node:".concat(c.tagName, ":").concat(c.key, ":progress-defaults")), er(g, rt)), c.tagName === "slider" && (F("node:".concat(c.tagName, ":").concat(c.key, ":slider-defaults")), nr(g, rt)), F("node:".concat(c.tagName, ":").concat(c.key, ":children-effective")); var y = lr(c, u.detailsOpen); F("node:".concat(c.tagName, ":").concat(c.key, ":children-map count=").concat(y.length)); var E = y.map(a); F("node:".concat(c.tagName, ":").concat(c.key, ":children-insert")); for (var _ = 0; _ < E.length; _++) {
        var w = y[_], G = E[_];
        if (w && w.kind === "block") {
            var A = _ === E.length - 1 ? 0 : l(w);
            G.yogaNode.setMargin(rt.EDGE_BOTTOM, A);
        }
        g.insertChild(G.yogaNode, g.getChildCount());
    } return { yogaNode: g, buildBox: function () { return ({ kind: "block", key: c.key, tagName: c.tagName, attrs: c.attrs, x: g.getComputedLeft(), y: g.getComputedTop(), width: g.getComputedWidth(), height: g.getComputedHeight(), children: E.map(function (_) { return _.buildBox(); }) }); } }; } var d = rt.Node.create(); F("root:flex-direction"), d.setFlexDirection(rt.FLEX_DIRECTION_COLUMN), F("root:align-items"), d.setAlignItems(rt.ALIGN_STRETCH), F("root:width"), d.setWidth(e), F("root:height"), d.setHeight(n), F("root:padding-left"), d.setPadding(rt.EDGE_LEFT, 16), F("root:padding-top"), d.setPadding(rt.EDGE_TOP, 16), F("root:padding-right"), d.setPadding(rt.EDGE_RIGHT, 16 + An), F("root:padding-bottom"), d.setPadding(rt.EDGE_BOTTOM, 16), F("root:children-map count=".concat(t.length)); var f = t.map(a); F("root:children-insert"); for (var c = 0; c < f.length; c++) {
        var M = t[c], g = f[c];
        if (M && M.kind === "block") {
            var y = c === f.length - 1 ? 0 : l(M);
            g.yogaNode.setMargin(rt.EDGE_BOTTOM, y);
        }
        d.insertChild(g.yogaNode, d.getChildCount());
    } F("root:calculate"), d.calculateLayout(e, n, rt.DIRECTION_LTR), F("root:build-box"); var m = { kind: "block", tagName: "root", x: 0, y: 0, width: d.getComputedWidth(), height: d.getComputedHeight(), children: f.map(function (c) { return c.buildBox(); }) }; return F("root:free"), (b = d.freeRecursive) == null || b.call(d), F("build:done"), m; }
    function gs(t, e, n) {
        var e_45, _a, e_46, _b, e_47, _c, e_48, _d, e_49, _f;
        var U, L;
        F("render:start");
        var r = be, i = n != null ? n : t.stage;
        F("render:get-background");
        var o = St(i, "__background");
        F("render:get-content-root");
        var s = xe(i, "__contentRoot");
        F("render:get-dialog-root");
        var l = xe(i, "__dialogRoot");
        F("render:get-overlay-root");
        var a = xe(i, "__overlayRoot");
        F("render:ensure-background"), wi(i, o, 0), F("render:ensure-content-root"), He(i, s, 1), F("render:ensure-dialog-root"), He(i, l, 2), F("render:ensure-overlay-root"), He(i, a, 3), F("render:overlay-remove-children"), a.removeChildren(), F("render:overlay-removed");
        var d = [], f = [], m = ds(e);
        F("render:clear-ui-state"), u.fieldBounds.clear(), u.sliderBounds.clear(), u.dialogDragBounds.clear(), u.hoverRects.length = 0, u.hoverHandlers.clear(), u.iframeRects.length = 0, F("render:node-cache");
        var b = (U = yi.get(i)) != null ? U : new Map;
        yi.set(i, b);
        var c = new Set, M = function (h) {
            var e_50, _a;
            var q;
            var I = 0, B = function (Z, tt, Y) {
                var e_51, _a;
                var p;
                if (Z.kind === "block" && Z.tagName === "dialog")
                    return;
                var j = tt + Z.x, N = Y + Z.y;
                I = Math.max(I, N + Z.height);
                try {
                    for (var _b = __values((p = Z.children) != null ? p : []), _c = _b.next(); !_c.done; _c = _b.next()) {
                        var P = _c.value;
                        B(P, j, N);
                    }
                }
                catch (e_51_1) { e_51 = { error: e_51_1 }; }
                finally {
                    try {
                        if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
                    }
                    finally { if (e_51) throw e_51.error; }
                }
            };
            try {
                for (var _b = __values((q = h.children) != null ? q : []), _c = _b.next(); !_c.done; _c = _b.next()) {
                    var Z = _c.value;
                    B(Z, 0, 0);
                }
            }
            catch (e_50_1) { e_50 = { error: e_50_1 }; }
            finally {
                try {
                    if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
                }
                finally { if (e_50) throw e_50.error; }
            }
            return I;
        }, g = new Set;
        try {
            for (var _g = __values(u.textDrags.values()), _h = _g.next(); !_h.done; _h = _g.next()) {
                var h = _h.value;
                g.add(h.key);
            }
        }
        catch (e_45_1) { e_45 = { error: e_45_1 }; }
        finally {
            try {
                if (_h && !_h.done && (_a = _g.return)) _a.call(_g);
            }
            finally { if (e_45) throw e_45.error; }
        }
        F("render:measure");
        var y = Go(r);
        function E(h, I, B) { return Math.max(I, Math.min(B, h)); }
        var _ = function (h) {
            var e_52, _a;
            try {
                for (var _b = __values(u.textDrags.entries()), _c = _b.next(); !_c.done; _c = _b.next()) {
                    var _d = __read(_c.value, 2), I = _d[0], B = _d[1];
                    if (B.key === h)
                        return I;
                }
            }
            catch (e_52_1) { e_52 = { error: e_52_1 }; }
            finally {
                try {
                    if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
                }
                finally { if (e_52) throw e_52.error; }
            }
            return null;
        }, w = function (h) {
            var e_53, _a;
            var I = u.keyboardOwnerPointerId;
            if (u.focusedKeyByPointer.get(I) === h)
                return I;
            try {
                for (var _b = __values(u.focusedKeyByPointer.entries()), _c = _b.next(); !_c.done; _c = _b.next()) {
                    var _d = __read(_c.value, 2), B = _d[0], q = _d[1];
                    if (q === h)
                        return B;
                }
            }
            catch (e_53_1) { e_53 = { error: e_53_1 }; }
            finally {
                try {
                    if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
                }
                finally { if (e_53) throw e_53.error; }
            }
            return null;
        };
        F("render:background-clear"), Et(o), F("render:background-rect"), o.rect(0, 0, t.renderer.width, t.renderer.height), F("render:background-fill"), o.fill(r.background), F("render:content-position");
        {
            var h = u.scroll, I = h && Number(h.y || 0) || 0;
            if (I !== 0) {
                var B = s.position;
                B && (B.x = 0, B.y = -I);
            }
        }
        F("render:content-position-done");
        function G(h, I, B, q, Z, tt, Y, j, N) {
            var e_54, _a;
            if (q === void 0) { q = 0; }
            if (Z === void 0) { Z = 0; }
            var D, R, W, X, z, K, V, nt, Q, it, st, dt, yt, Rt, Gt, at, Mt, Dt, Ct, Nt;
            F("render:draw:".concat(j, ":").concat(h.kind, ":").concat(h.kind === "block" ? h.tagName : "text", ":start"));
            var p = h.kind === "block" ? h.key && h.key.length > 0 ? h.key : "".concat(j, ":").concat((D = h.tagName) != null ? D : "block") : "", P = h.kind === "block" ? "b:".concat(p) : "t:".concat(j);
            F("render:draw:".concat(j, ":cache"));
            var O = b.get(P);
            (!O || Fn(I, O)) && (F("render:draw:".concat(j, ":new-container")), O = new _t, O.label = P, b.set(P, O)), F("render:draw:".concat(j, ":ensure-child")), c.add(P), He(I, O, N), F("render:draw:".concat(j, ":children-root"));
            var C = xe(O, "__children");
            if (F("render:draw:".concat(j, ":ensure-children-root")), He(O, C, 1), F("render:draw:".concat(j, ":position")), oe(O, h.x, h.y), h.kind === "block" && h.tagName === "hr" && oe(O, Math.round(h.x), Math.round(h.y)), h.kind === "block" && h.tagName === "dialog" && h.key) {
                var mt = Qe(u.dialogs, h.key), ft = Math.max(0, h.width), ut = Math.max(0, h.height), lt = Y.x, Xt = Y.y, vt = Math.max(lt, Y.x + Y.w - ft), Bt = Math.max(Xt, Y.y + Y.h - ut);
                if (u.dialogDragBounds.set(h.key, { minX: lt, minY: Xt, maxX: vt, maxY: Bt }), Kt() && !mt.__trueosInitialPositionSeeded) {
                    var se = Y.w <= 760 && Y.h <= 800, ct = lt + Math.max(12, Math.floor((Y.w - ft) / 2)), pt = Xt + Math.max(se ? 190 : 40, Math.floor((Y.h - ut) / 2));
                    mt.x = Math.max(lt, Math.min(vt, ct)), mt.y = Math.max(Xt, Math.min(Bt, pt)), mt.__trueosInitialPositionSeeded = !0;
                }
                mt.x = Math.max(lt, Math.min(vt, mt.x)), mt.y = Math.max(Xt, Math.min(Bt, mt.y)), oe(O, mt.x, mt.y);
            }
            var x = q + O.position.x, S = Z + O.position.y;
            if (h.kind === "block") {
                F("render:draw:".concat(j, ":block:").concat(h.tagName, ":begin"));
                var mt = B;
                (h.tagName === "h1" || h.tagName === "h2" || h.tagName === "h3" || h.tagName === "summary" || h.tagName === "th") && (mt = { bold: !0 }), F("render:draw:".concat(j, ":graphics"));
                var ft = St(O, "__g");
                F("render:draw:".concat(j, ":graphics-clear")), Et(ft), F("render:draw:".concat(j, ":graphics-ensure")), wi(O, ft, 0), ft.zIndex = -10;
                var ut = Math.max(0, h.width), lt = Math.max(0, h.height), Xt = null;
                if ((h.tagName === "h1" || h.tagName === "h2" || h.tagName === "h3") && (oe(O, Math.round(h.x), Math.round(h.y)), ut = Math.round(ut), lt = Math.round(lt)), F("render:draw:".concat(j, ":widget:").concat(h.tagName)), h.tagName === "hr")
                    cr({ graphics: ft, w: ut, theme: r });
                else if (h.tagName !== "barrow") {
                    if (h.tagName !== "searchrow") {
                        if (h.tagName === "searchbutton")
                            Kr({ node: h, container: O, graphics: ft, w: ut, h: lt, theme: r, uiState: u, getPointerId: Ft, focusInputKey: (R = h.attrs) == null ? void 0 : R["data-focus-key"], requestPaint: ot });
                        else if (h.tagName === "progress" || h.tagName === "meter")
                            tr({ node: h, graphics: ft, w: ut, h: lt, theme: r });
                        else if (h.tagName === "sliderlabel")
                            ir({ node: h, container: O, theme: r, sliderStates: u.sliders });
                        else if (h.tagName === "slider")
                            Ze({ node: h, container: O, graphics: ft, w: ut, h: lt, absX: x, absY: S, theme: r, sliderStates: u.sliders, sliderBounds: u.sliderBounds, sliderDrags: u.sliderDrags, requestPaint: ot, getPointerId: Ft });
                        else if (h.tagName === "timeinput" || h.tagName === "dateinput" || h.tagName === "monthinput" || h.tagName === "weekinput" || h.tagName === "datetimelocalinput")
                            oi({ node: h, container: O, graphics: ft, w: ut, h: lt, absX: x, absY: S, theme: r, uiState: u, getPointerId: Ft, getCursorColor: Vt, temporalStates: u.temporals, yearSliderOwners: u.temporalYearOwners, getOrInitInputValue: function (J, bt) { return un(J, bt); }, requestPaint: ot, popupSink: f });
                        else if (h.tagName === "input") {
                            var J = h.key, bt = J != null ? w(J) : null, ee = J != null && u.focusedKeyByPointer.get(u.keyboardOwnerPointerId) === J, Ot = J == null ? null : ee ? u.keyboardOwnerPointerId : g.has(J) ? _(J) : null, ne = Ot != null, Jt = bt != null ? Vt(bt) : null;
                            Wr({ node: h, container: O, graphics: ft, w: ut, h: lt, absX: x, absY: S, theme: r, textMeasure: y, uiState: u, getOrInitInputState: un, clamp: E, radioGroups: m, textDrags: u.textDrags, requestPaint: ot, showCaret: ne, caretPointerId: Ot, focusColor: Jt != null ? Jt : void 0, getCursorColor: Vt, getPointerId: Ft });
                        }
                        else if (h.tagName === "textarea") {
                            var J = h.key, bt = J != null ? w(J) : null, ee = J != null && u.focusedKeyByPointer.get(u.keyboardOwnerPointerId) === J, Ot = J == null ? null : ee ? u.keyboardOwnerPointerId : g.has(J) ? _(J) : null, ne = Ot != null, Jt = bt != null ? Vt(bt) : null;
                            $r({ node: h, container: O, graphics: ft, w: ut, h: lt, absX: x, absY: S, theme: r, textMeasure: y, uiState: u, getOrInitInputState: un, clamp: E, textDrags: u.textDrags, requestPaint: ot, showCaret: ne, caretPointerId: Ot, focusColor: Jt != null ? Jt : void 0, getCursorColor: Vt, getPointerId: Ft });
                        }
                        else if (h.tagName === "select") {
                            if (h.key) {
                                var J = Number((X = (W = h.attrs) == null ? void 0 : W["data-selected-index"]) != null ? X : "0");
                                de(u.selects, h.key, Number.isFinite(J) ? J : 0);
                            }
                            nn({ node: h, container: O, graphics: ft, w: ut, h: lt, absX: x, absY: S, theme: r, selectStates: u.selects, uiState: u, getPointerId: Ft, getCursorColor: Vt, requestPaint: ot, popupSink: d });
                        }
                        else if (h.tagName === "summary")
                            h.key && u.hoverRects.push({ key: h.key, kind: "summary", cursor: "pointer", x: x, y: S, w: ut, h: lt }), or({ node: h, container: O, w: ut, h: lt, theme: r, detailsOpen: u.detailsOpen, requestRerender: dn });
                        else if (h.tagName === "dialog")
                            jr({ node: h, container: O, w: ut, h: lt, theme: r, selectedBy: u.dialogSelectedBy, getCursorColor: Vt, dialogStates: u.dialogs, dialogDrags: u.dialogDrags, bringToFront: function (J) { u.dialogZ.set(J, u.dialogZCounter++); }, requestPaint: ot, getPointerId: Ft });
                        else if (h.tagName === "img")
                            Sr({ node: h, container: O, graphics: ft, w: ut, h: lt, theme: r, requestRerender: dn });
                        else if (h.tagName === "svg") {
                            var J = (K = (z = h.attrs) == null ? void 0 : z["data-svg"]) != null ? K : "";
                            Nr({ svgMarkup: J, container: O, w: ut, h: lt, requestRerender: dn });
                        }
                        else if (h.tagName === "canvas")
                            Gr({ node: h, container: O, graphics: ft, w: ut, h: lt, theme: r });
                        else if (h.tagName === "iframe")
                            Fr({ node: h, container: O, graphics: ft, w: ut, h: lt, theme: r });
                        else if (h.tagName === "color")
                            u.color.bounds = { x: x, y: S, w: Math.max(0, ut), h: Math.max(0, lt) }, ti({ node: h, container: O, graphics: ft, w: ut, h: lt, theme: r, rgb: u.color.rgb, setRgb: function (J) { u.color.rgb = J; }, alpha: u.color.a, setAlpha: function (J) { u.color.a = Math.max(0, Math.min(255, Math.round(J))); }, pick: u.color.pick, setPick: function (J) { u.color.pick = J; }, requestPaint: ot, getPointerId: Ft, setDraggingPointerId: function (J) { u.color.draggingPointerId = J; } });
                        else if (h.tagName === "number") {
                            var J_1 = h.key, bt_1 = String((nt = (V = h.attrs) == null ? void 0 : V.channel) != null ? nt : "").toLowerCase(), ee_1 = bt_1 === "r" || bt_1 === "g" || bt_1 === "b" || bt_1 === "a";
                            J_1 && Jr({ node: h, container: O, graphics: ft, w: ut, h: lt, theme: r, getValue: function () { var Ot, ne; return ee_1 ? bt_1 === "a" ? (Ot = u.color.a) != null ? Ot : 255 : (ne = u.color.rgb[bt_1]) != null ? ne : 0 : Sn(u.numbers, J_1, h.attrs).value; }, setValue: function (Ot) { ee_1 ? bt_1 === "a" ? u.color.a = Math.max(0, Math.min(255, Math.round(Ot))) : u.color.rgb[bt_1] = Math.max(0, Math.min(255, Math.round(Ot))) : Sn(u.numbers, J_1, h.attrs).value = Ot; }, requestPaint: ot, numberHolds: u.numberHolds, getPointerId: Ft });
                        }
                        else if (h.tagName === "button")
                            h.key && u.hoverRects.push({ key: h.key, kind: "button", cursor: "pointer", x: x, y: S, w: ut, h: lt }), dr({ container: O, graphics: ft, w: ut, h: lt, label: me(Ln(h)), theme: r, registerHoverHandlers: h.key ? function (J) { u.hoverHandlers.set(h.key, J); } : void 0 });
                        else if (!En(h.tagName))
                            if (h.tagName === "table")
                                mr({ graphics: ft, w: ut, h: lt, boxBorder: r.boxBorder });
                            else if (h.tagName === "td" || h.tagName === "th")
                                fr({ nodeTag: h.tagName, graphics: ft, w: ut, h: lt, theme: r });
                            else {
                                var J = Math.max(0, Math.round(ut)), bt = Math.max(0, Math.round(lt));
                                ft.rect(0, 0, J, bt), ft.stroke({ width: 1, color: r.boxBorder, alignment: 0 });
                            }
                    }
                }
                F("render:draw:".concat(j, ":overlay-label")), Xt && O.addChild(Xt);
                var vt = null, Bt = null, se = h.tagName === "iframe" && String((it = (Q = h.attrs) == null ? void 0 : Q["data-root"]) != null ? it : "") === "1";
                if (h.tagName === "iframe" && !se) {
                    h.key && u.iframeRects.push({ key: h.key, x: x, y: S, w: Math.max(0, ut), h: Math.max(0, lt) }), vt = xe(O, "__iframeContentRoot"), oe(vt, 0, 0);
                    var Ot = St(O, "__iframeContentMask");
                    Et(Ot);
                    var ne = 0, Jt = 34, Ci = Math.max(0, ut), Oi = Math.max(0, lt - 34);
                    Ot.rect(ne, Jt, Ci, Oi), Ot.fill(16777215), Ot.alpha = 0, vt.mask = Ot;
                    var Xe_1 = (st = h.key) != null ? st : "", ht_1 = (dt = u.iframeScroll.get(Xe_1)) != null ? dt : { y: 0, contentHeight: 0, viewportHeight: 0, draggingPointerId: null, dragOffsetY: 0, track: { x: 0, y: 0, w: Pe, h: 0 }, thumb: { x: 0, y: 0, w: Pe, h: 0 }, rect: { x: x, y: S, w: Math.max(0, ut), h: Math.max(0, lt) } };
                    ht_1.rect = { x: x, y: S, w: Math.max(0, ut), h: Math.max(0, lt) }, ht_1.contentHeight = M(h), ht_1.viewportHeight = Math.max(0, lt - 34 - 8);
                    var _e_1 = Math.max(0, ht_1.contentHeight - ht_1.viewportHeight);
                    ht_1.y = Math.max(0, Math.min(ht_1.y, _e_1)), Bt = xe(vt, "__iframeScrollRoot"), oe(Bt, 0, -ht_1.y);
                    var le = St(O, "__iframeScrollbar");
                    Et(le), le.eventMode = "static";
                    var gn = An, fe = Pe, Ye = Math.max(0, ut - fe - gn), bn = 34 + gn, Oe = Math.max(0, lt - 34 - gn * 2), Bn = _e_1 > .5 && Oe > 1;
                    if (le.visible = Bn, Bn) {
                        var xn = Math.max(24, (ht_1.viewportHeight || 1) / Math.max(1, ht_1.contentHeight) * Oe), Ri = Math.max(1, Oe - xn), Di = _e_1 <= 0 ? 0 : ht_1.y / _e_1, Wn = bn + Ri * Di;
                        ht_1.track = { x: x + Ye, y: S + bn, w: fe, h: Oe }, ht_1.thumb = { x: x + Ye, y: S + Wn, w: fe, h: xn }, le.rect(Ye, bn, fe, Oe), le.fill({ color: 0, alpha: .06 }), le.rect(Ye, Wn, fe, xn), le.fill({ color: 0, alpha: .25 }), le.on("pointerdown", function (Zt) { var Un, Xn, Yn, Kn, zn, jn; if ((Zt == null ? void 0 : Zt.button) === 2)
                            return; var yn = Ft(Zt); if (yn <= 0)
                            return; var Ke = (Xn = (Un = Zt.global) == null ? void 0 : Un.x) != null ? Xn : 0, pe = (Kn = (Yn = Zt.global) == null ? void 0 : Yn.y) != null ? Kn : 0; if (!(Ke >= ht_1.track.x && Ke <= ht_1.track.x + ht_1.track.w && pe >= ht_1.track.y && pe <= ht_1.track.y + ht_1.track.h))
                            return; if (Ke >= ht_1.thumb.x && Ke <= ht_1.thumb.x + ht_1.thumb.w && pe >= ht_1.thumb.y && pe <= ht_1.thumb.y + ht_1.thumb.h) {
                            ht_1.draggingPointerId = yn, ht_1.dragOffsetY = pe - ht_1.thumb.y, u.iframeScroll.set(Xe_1, ht_1), (zn = Zt.stopPropagation) == null || zn.call(Zt);
                            return;
                        } var Hn = Math.max(1, ht_1.track.h - ht_1.thumb.h), $n = Math.max(ht_1.track.y, Math.min(ht_1.track.y + Hn, pe - ht_1.thumb.h / 2)), Ai = ($n - ht_1.track.y) / Hn; ht_1.y = Math.max(0, Math.min(_e_1, Ai * _e_1)), ht_1.draggingPointerId = yn, ht_1.dragOffsetY = pe - $n, u.iframeScroll.set(Xe_1, ht_1), ot == null || ot(), (jn = Zt.stopPropagation) == null || jn.call(Zt); });
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
                var At = (Rt = Bt != null ? Bt : vt) != null ? Rt : C, Lt = x + ((Gt = vt == null ? void 0 : vt.position.x) != null ? Gt : 0), zt = S + ((at = vt == null ? void 0 : vt.position.y) != null ? at : 0) + ((Mt = Bt == null ? void 0 : Bt.position.y) != null ? Mt : 0);
                F("render:draw:".concat(j, ":children"));
                var te = 0;
                for (var J = 0; J < ((Dt = h.children) != null ? Dt : []).length; J++) {
                    var bt = ((Ct = h.children) != null ? Ct : [])[J];
                    if (bt.kind === "block" && bt.tagName === "dialog")
                        pt.push(bt);
                    else {
                        if (h.tagName === "button" && bt.kind === "text")
                            continue;
                        G(bt, At, mt, Lt, zt, pt, Tt, "".concat(j, ".").concat(J), te++);
                    }
                }
                if ((h.tagName === "dialog" || h.tagName === "iframe" && !se) && ct.length > 0) {
                    ct.sort(function (J, bt) { var ne, Jt; var ee = J.key && (ne = u.dialogZ.get(J.key)) != null ? ne : 0, Ot = bt.key && (Jt = u.dialogZ.get(bt.key)) != null ? Jt : 0; return ee - Ot; });
                    try {
                        for (var ct_1 = __values(ct), ct_1_1 = ct_1.next(); !ct_1_1.done; ct_1_1 = ct_1.next()) {
                            var J = ct_1_1.value;
                            var bt = J.key && J.key.length > 0 ? J.key : "".concat(j, ".dlg.").concat(te);
                            G(J, At, mt, Lt, zt, ct, Tt, "".concat(j, ".dlg.").concat(bt), te++);
                        }
                    }
                    catch (e_54_1) { e_54 = { error: e_54_1 }; }
                    finally {
                        try {
                            if (ct_1_1 && !ct_1_1.done && (_a = ct_1.return)) _a.call(ct_1);
                        }
                        finally { if (e_54) throw e_54.error; }
                    }
                }
            }
            else {
                F("render:draw:".concat(j, ":text:begin"));
                var mt = It(O, "__text", function (ft) { ft.style = { fontFamily: r.fontFamily, fontSize: r.fontSize, fill: r.text, fontWeight: B.bold ? "700" : "400", wordWrap: !0, wordWrapWidth: 0 }; });
                mt.text = (Nt = h.text) != null ? Nt : "", mt.style.fontFamily = r.fontFamily, mt.style.fontSize = r.fontSize, mt.style.fill = r.text, mt.style.fontWeight = B.bold ? "700" : "400", mt.style.wordWrap = !0, mt.style.wordWrapWidth = Math.max(0, Math.ceil(h.width) + Me), oe(mt, 0, xt), F("render:draw:".concat(j, ":text:done"));
            }
        }
        F("render:root-loop");
        var A = { bold: !1 }, $ = { x: 0, y: 0, w: t.renderer.width, h: t.renderer.height }, v = [], k = s.position, T = k && Number(k.y || 0) || 0, H = 0;
        for (var h = 0; h < e.children.length; h++) {
            F("render:root-loop:".concat(h));
            var I = e.children[h];
            I && (I.kind === "block" && I.tagName === "dialog" ? v.push(I) : (F("render:root-loop:".concat(h, ":dispatch")), G(I, s, A, 0, T, v, $, "root.".concat(h), H++)));
        }
        if (F("render:root-dialogs"), v.length > 0) {
            v.sort(function (I, B) { var tt, Y; var q = I.key && (tt = u.dialogZ.get(I.key)) != null ? tt : 0, Z = B.key && (Y = u.dialogZ.get(B.key)) != null ? Y : 0; return q - Z; });
            var h = 0;
            try {
                for (var v_1 = __values(v), v_1_1 = v_1.next(); !v_1_1.done; v_1_1 = v_1.next()) {
                    var I = v_1_1.value;
                    var B = I.key && I.key.length > 0 ? I.key : "rootdlg.".concat(h);
                    G(I, l, A, 0, 0, v, $, "dlg.".concat(B), h++);
                }
            }
            catch (e_46_1) { e_46 = { error: e_46_1 }; }
            finally {
                try {
                    if (v_1_1 && !v_1_1.done && (_b = v_1.return)) _b.call(v_1);
                }
                finally { if (e_46) throw e_46.error; }
            }
        }
        if (F("render:temporal-popups"), f.length > 0 && si({ popups: f, stage: a, theme: r, viewportW: t.renderer.width, viewportH: t.renderer.height, temporalStates: u.temporals, getOrInitInputValue: function (h, I) { return un(h, I); }, sliders: u.sliders, sliderBounds: u.sliderBounds, sliderDrags: u.sliderDrags, selects: u.selects, selectPopups: d, uiFocus: u, getPointerId: Ft, getCursorColor: Vt, requestPaint: ot }), F("render:select-popups"), d.length > 0)
            try {
                for (var d_2 = __values(d), d_2_1 = d_2.next(); !d_2_1.done; d_2_1 = d_2.next()) {
                    var h = d_2_1.value;
                    ni({ popup: h, stage: a, theme: r, selectStates: u.selects, uiState: u, getPointerId: Ft, requestPaint: ot, viewportW: t.renderer.width, viewportH: t.renderer.height });
                }
            }
            catch (e_47_1) { e_47 = { error: e_47_1 }; }
            finally {
                try {
                    if (d_2_1 && !d_2_1.done && (_c = d_2.return)) _c.call(d_2);
                }
                finally { if (e_47) throw e_47.error; }
            }
        F("render:context-menus");
        var _loop_3 = function (h, I) {
            if (!(I != null && I.open))
                return "continue";
            var B = new _t;
            B.eventMode = "static", B.cursor = "default", oe(B, I.x, I.y);
            var q = 140, Z = 28, tt = 6, Y = ["Copy", "Paste", "Close"], j = new wt;
            j.rect(0, 0, q + tt * 2, Y.length * Z + tt * 2), j.fill(16777215);
            var N = 1;
            j.rect(N, N, q + tt * 2 - N * 2, Y.length * Z + tt * 2 - N * 2), j.stroke({ width: 2, color: Vt(h), alignment: 0 }), B.addChild(j), Y.forEach(function (p, P) { var O = tt + P * Z, C = new _t; C.eventMode = "static", C.cursor = "pointer", oe(C, tt, O); var x = new wt; x.rect(0, 0, q, Z), x.fill(16777215), C.addChild(x); var S = jt({ text: p, fontFamily: r.fontFamily, fontSize: r.fontSize, fill: r.text }); oe(S, 8, Math.max(0, (Z - S.height) / 2) + xt), C.addChild(S); var D = function (R) { return Ft(R) === h; }; C.on("pointerover", function (R) { D(R) && (x.clear(), x.rect(0, 0, q, Z), x.fill(15921906)); }), C.on("pointerout", function (R) { D(R) && (x.clear(), x.rect(0, 0, q, Z), x.fill(16777215)); }), C.on("pointerdown", function (R) { var V, nt, Q, it, st, dt, yt, Rt, Gt, at, Mt; if (!D(R))
                return; (V = R.stopPropagation) == null || V.call(R); var W = (nt = u.focusedKeyByPointer.get(h)) != null ? nt : null, X = W ? u.inputs.get(W) : null, z = W != null && u.fieldBounds.has(W) && X != null && typeof X.value == "string"; if (p === "Copy" && z) {
                var Dt = X, Ct = (Q = Dt.value) != null ? Q : "", Nt = (st = (it = Dt.selections) == null ? void 0 : it.get(h)) != null ? st : null, mt = Nt ? Math.max(0, Math.min(Ct.length, (dt = Nt.start) != null ? dt : 0)) : 0, ft = Nt ? Math.max(0, Math.min(Ct.length, (yt = Nt.end) != null ? yt : mt)) : mt, ut = Math.min(mt, ft), lt = Math.max(mt, ft), Xt = ut !== lt ? Ct.slice(ut, lt) : Ct;
                u.clipboards.set(h, Xt);
            }
            else if (p === "Paste" && z) {
                var Dt = (Rt = u.clipboards.get(h)) != null ? Rt : "";
                if (Dt.length > 0) {
                    var Ct = X, Nt = (Gt = Ct.value) != null ? Gt : "";
                    if (Ct.selections || (Ct.selections = new Map), !Ct.selections.has(h)) {
                        var Bt = Nt.length;
                        Ct.selections.set(h, { start: Bt, end: Bt });
                    }
                    var mt = Ct.selections.get(h), ft = Math.max(0, Math.min(Nt.length, (at = mt.start) != null ? at : Nt.length)), ut = Math.max(0, Math.min(Nt.length, (Mt = mt.end) != null ? Mt : ft)), lt = Math.min(ft, ut), Xt = Math.max(ft, ut);
                    Ct.value = Nt.slice(0, lt) + Dt + Nt.slice(Xt);
                    var vt = lt + Dt.length;
                    mt.start = vt, mt.end = vt;
                }
            } var K = u.contextMenus.get(h); K && (K.open = !1, u.contextMenus.set(h, K)), ot == null || ot(); }), B.addChild(C); }), a.addChild(B);
        };
        try {
            for (var _j = __values(u.contextMenus.entries()), _k = _j.next(); !_k.done; _k = _j.next()) {
                var _l = __read(_k.value, 2), h = _l[0], I = _l[1];
                _loop_3(h, I);
            }
        }
        catch (e_48_1) { e_48 = { error: e_48_1 }; }
        finally {
            try {
                if (_k && !_k.done && (_d = _j.return)) _d.call(_j);
            }
            finally { if (e_48) throw e_48.error; }
        }
        F("render:prune-cache");
        try {
            for (var _m = __values(b.entries()), _p = _m.next(); !_p.done; _p = _m.next()) {
                var _q = __read(_p.value, 2), h = _q[0], I = _q[1];
                if (!c.has(h)) {
                    try {
                        I.removeFromParent(), (L = I.destroy) == null || L.call(I, { children: !0 });
                    }
                    catch (B) { }
                    b.delete(h);
                }
            }
        }
        catch (e_49_1) { e_49 = { error: e_49_1 }; }
        finally {
            try {
                if (_p && !_p.done && (_f = _m.return)) _f.call(_m);
            }
            finally { if (e_49) throw e_49.error; }
        }
        F("render:done");
    }
    function bs() {
        return ze(this, null, function () {
            var t, e, n, r, _a, i_1, _b, o_3, s, l, a_2, d, f_1, m_1, b_1, c_1, g_1, y_1, E_2, _, _c, w, G, A_1, $_1, v_2, k_1, T_1, H_1, U_1, L_1, h_2, I_2, B_3, q_1, Z, tt, Y_1, j_1, p, P, O, N_4, n_3, r;
            return __generator(this, function (_d) {
                switch (_d.label) {
                    case 0:
                        _d.trys.push([0, 9, , 10]);
                        kt("main:start");
                        n = (t = document.getElementById("app")) != null ? t : document.body, r = !0;
                        kt("main:yoga");
                        if (!r) return [3 /*break*/, 1];
                        _a = cs();
                        return [3 /*break*/, 3];
                    case 1: return [4 /*yield*/, Promise.resolve().then(function () { return (bi(), gi); })];
                    case 2:
                        _a = (_d.sent()).default;
                        _d.label = 3;
                    case 3:
                        rt = _a, kt("main:create-app");
                        i_1 = r ? ls() : new Je;
                        _b = r;
                        if (_b) return [3 /*break*/, 5];
                        return [4 /*yield*/, i_1.init({ background: "#ffffff", resizeTo: window, antialias: !1, preference: "webgl" })];
                    case 4:
                        _b = (_d.sent());
                        _d.label = 5;
                    case 5:
                        _b, kt("main:attach-capture"), pi(i_1), window.__TRUEOS_PIXI_APP = i_1, kt("main:append-canvas"), n.appendChild(i_1.canvas), i_1.ticker.stop(), kt("main:capture-flags"), r && (u.harness.enabled = !1, u.virtualCursor.enabled = !1, window.__pixiCapture && (window.__pixiCapture.persist = !1)), kt("main:canvas-listeners"), i_1.canvas.addEventListener("contextmenu", function (p) { return p.preventDefault(); }), i_1.canvas.addEventListener("wheel", function (p) { var S, D; var P = (S = p.offsetX) != null ? S : 0, O = (D = p.offsetY) != null ? D : 0, C = null; for (var R = u.iframeRects.length - 1; R >= 0; R--) {
                            var W = u.iframeRects[R];
                            if (P >= W.x && P <= W.x + W.w && O >= W.y && O <= W.y + W.h) {
                                C = W.key;
                                break;
                            }
                        } if (C) {
                            var R = u.iframeScroll.get(C);
                            if (R) {
                                var W = Math.max(0, R.contentHeight - R.viewportHeight);
                                W > 0 && (R.y = Math.max(0, Math.min(W, R.y + p.deltaY)), u.iframeScroll.set(C, R), ot == null || ot(), p.preventDefault());
                                return;
                            }
                        } var x = Math.max(0, u.scroll.contentHeight - u.scroll.viewportHeight); x <= 0 || (u.scroll.y = Math.max(0, Math.min(x, u.scroll.y + p.deltaY)), ot == null || ot(), p.preventDefault()); }, { passive: !1 }), kt("main:stage:eventMode"), i_1.stage.eventMode = "static", kt("main:stage:hitArea"), i_1.stage.hitArea = i_1.screen, kt("main:stage:on:pointerdown"), i_1.stage.on("pointerdown", function (p) {
                            var e_55, _a;
                            var P, O, C, x, S, D;
                            if ((p == null ? void 0 : p.button) === 2) {
                                var R = Ft(p);
                                if (R > 0) {
                                    var W = (P = u.contextMenus.get(R)) != null ? P : { open: !1, x: 0, y: 0 };
                                    W.open = !0, W.x = (C = (O = p.global) == null ? void 0 : O.x) != null ? C : 0, W.y = (S = (x = p.global) == null ? void 0 : x.y) != null ? S : 0, u.contextMenus.set(R, W);
                                }
                                ot == null || ot(), (D = p.preventDefault) == null || D.call(p);
                                return;
                            }
                            if ((p == null ? void 0 : p.button) !== 2) {
                                var R = Ft(p), W = R > 0 ? u.contextMenus.get(R) : null;
                                W && W.open && (W.open = !1, u.contextMenus.set(R, W), ot == null || ot());
                            }
                            if ((p == null ? void 0 : p.button) !== 2) {
                                var R = !1;
                                try {
                                    for (var _b = __values(u.selects.values()), _c = _b.next(); !_c.done; _c = _b.next()) {
                                        var W = _c.value;
                                        W.open && (W.open = !1, R = !0);
                                    }
                                }
                                catch (e_55_1) { e_55 = { error: e_55_1 }; }
                                finally {
                                    try {
                                        if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
                                    }
                                    finally { if (e_55) throw e_55.error; }
                                }
                                R && (ot == null || ot());
                            }
                            (p == null ? void 0 : p.button) !== 2 && ai(u.temporals) && (ot == null || ot()), q_1();
                        }), kt("main:stage:done"), kt("main:roots");
                        o_3 = new _t, s = new _t;
                        s.eventMode = "static";
                        l = new _t;
                        l.eventMode = "none", i_1.stage.addChild(o_3), i_1.stage.addChild(s), i_1.stage.addChild(l);
                        a_2 = new wt;
                        a_2.label = "__trueosGlobalScrollbar", a_2.eventMode = "static", s.addChild(a_2);
                        d = function (p, P) { p.clear(); var O = P.half, C = P.strokeWidth, x = P.color; p.moveTo(-O, 0), p.lineTo(O, 0), p.stroke({ width: C, color: x }), p.moveTo(0, -O), p.lineTo(0, O), p.stroke({ width: C, color: x }); }, f_1 = new wt;
                        f_1.eventMode = "none", f_1.visible = !1, l.addChild(f_1);
                        m_1 = new wt;
                        m_1.eventMode = "none", m_1.visible = !1, l.addChild(m_1);
                        b_1 = new wt;
                        b_1.eventMode = "none", b_1.visible = !1, l.addChild(b_1);
                        c_1 = new wt;
                        c_1.eventMode = "none", l.addChild(c_1), kt("main:text-measure");
                        g_1 = document.createElement("canvas").getContext("2d");
                        if (!g_1)
                            throw new Error("2D canvas not available");
                        g_1.font = "".concat(be.fontSize, "px ").concat(be.fontFamily);
                        y_1 = function (p) { return g_1.measureText(p).width; }, E_2 = be.fontSize * 1.25;
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
                        Kt() && console.log("[trueos pixi widgets] input-html chars=".concat(_.length, " sample=\"").concat(hn(_), "\"")), kt("main:render-tree"), _i.clear();
                        w = qo(_), G = ts(), A_1 = ms(G.tree, w.rows);
                        if (Kt() && (console.log("[trueos pixi widgets] text-fallback source=".concat(w.source, " rows=").concat(w.rows.length, " samples=").concat(zo(w.rows))), console.log("[trueos pixi widgets] render-tree source=".concat(G.source, " nodes=").concat(A_1.length))), A_1.length === 0)
                            throw new Error("TrueSurfer widget render tree is missing");
                        $_1 = Ho(A_1), v_2 = null, k_1 = { nodes: 0, blocks: 0, text: 0, maxDepth: 0, tags: {} }, T_1 = 0, H_1 = function () { var p = Math.max(0, u.scroll.contentHeight - u.scroll.viewportHeight); u.scroll.y = Math.max(0, Math.min(u.scroll.y, p)); }, U_1 = function () { var p = i_1.renderer.width, P = i_1.renderer.height; u.scroll.viewportHeight = P; var O = u.scroll.contentHeight, C = Math.max(0, O - P), x = C > .5; if (a_2.clear(), a_2.visible = x, !x) {
                            u.scroll.track = { x: 0, y: 0, w: u.scroll.track.w, h: 0 }, u.scroll.thumb = { x: 0, y: 0, w: u.scroll.thumb.w, h: 0 };
                            return;
                        } var S = An, D = Pe, R = Math.max(0, p - D - S), W = S, X = Math.max(0, P - S * 2), K = Math.max(24, P / Math.max(P, O) * X), V = Math.max(1, X - K), nt = C <= 0 ? 0 : u.scroll.y / C, Q = W + V * nt; u.scroll.track = { x: R, y: W, w: D, h: X }, u.scroll.thumb = { x: R, y: Q, w: D, h: K }, a_2.rect(R, W, D, X), a_2.fill({ color: 0, alpha: .06 }), a_2.rect(R, Q, D, K), a_2.fill({ color: 0, alpha: .25 }); }, L_1 = function () { if (v_2) {
                            if (kt("main:paint:clamp"), H_1(), kt("main:paint:render-to-pixi"), gs(i_1, v_2, o_3), kt("main:paint:scrollbar"), U_1(), kt("main:paint:renderer-render"), i_1.renderer.render(i_1.stage), as($_1, k_1, Uo(A_1), Xo(v_2)), Kt()) {
                                var p = is(v_2);
                                window.__TRUEOS_PIXI_LAYOUT_TEXT_OVERLAYS__ = p, T_1 < 4 && (T_1 += 1, console.log("[trueos pixi widgets] layout-text-overlays count=".concat(p.length, " samples=").concat(os(p))));
                            }
                            kt("main:paint:done");
                        } };
                        Kt() && (window.__TRUEOS_REPAINT_NOW__ = function () { window.__TRUEOS_PIXI_DIRTY__ = !1, L_1(); });
                        h_2 = function () { kt("main:layout-build"); var p = ps(A_1, window.innerWidth, window.innerHeight); kt("main:layout-commit"), v_2 = p, Kt() && (window.__TRUEOS_PIXI_LAST_LAYOUT__ = p, window.__TRUEOS_PIXI_LAYOUT_TEXT_OVERLAYS__ = []), k_1 = $o(p), u.scroll.contentHeight = us(p), u.scroll.viewportHeight = window.innerHeight, L_1(); };
                        dn = function () { h_2(); };
                        I_2 = !1, B_3 = !1, q_1 = function () { if (Kt()) {
                            window.__TRUEOS_PIXI_DIRTY__ = !0;
                            return;
                        } B_3 || I_2 || (B_3 = !0, requestAnimationFrame(function () { B_3 = !1, i_1.renderer.render(i_1.stage); })); };
                        ot = function () { if (!I_2) {
                            if (Kt()) {
                                window.__TRUEOS_PIXI_DIRTY__ = !0;
                                return;
                            }
                            I_2 = !0, requestAnimationFrame(function () { I_2 = !1, L_1(); });
                        } }, kt("main:first-rerender"), h_2(), kt("main:cursor-setup");
                        Z = 2, tt = 10, Y_1 = Kt();
                        d(f_1, { half: tt, strokeWidth: Z, color: Vt(Wt) }), d(m_1, { half: tt, strokeWidth: Z, color: Vt(Ht) }), d(b_1, { half: tt, strokeWidth: Z, color: Vt(Ut) });
                        j_1 = 2;
                        if (d(c_1, { half: tt, strokeWidth: Z, color: Vt(j_1) }), u.userCursorPos.set(Wt, { x: i_1.renderer.width * .25, y: i_1.renderer.height * .5 }), u.userCursorPos.set(Ht, { x: i_1.renderer.width * .25 + 40, y: i_1.renderer.height * .5 + 20 }), u.userCursorPos.set(Ut, { x: i_1.renderer.width * .25 + 80, y: i_1.renderer.height * .5 + 40 }), f_1.visible = !Y_1, m_1.visible = !Y_1, b_1.visible = !Y_1, !Y_1) {
                            p = u.userCursorPos.get(Wt), P = u.userCursorPos.get(Ht), O = u.userCursorPos.get(Ut);
                            f_1.position.set(p.x, p.y), m_1.position.set(P.x, P.y), b_1.position.set(O.x, O.y);
                        }
                        c_1.visible = !Y_1 && u.virtualCursor.enabled;
                        N_4 = function () { if (Y_1) {
                            f_1.visible = !1, m_1.visible = !1, b_1.visible = !1, c_1.visible = !1;
                            return;
                        } var p = u.userCursorPos.get(Wt), P = u.userCursorPos.get(Ht), O = u.userCursorPos.get(Ut); p && (f_1.visible = !0, f_1.position.set(p.x, p.y)), P && (m_1.visible = !0, m_1.position.set(P.x, P.y)), O && (b_1.visible = !0, b_1.position.set(O.x, O.y)); var C = function (x, S) { var D = null, R = null; for (var W = u.hoverRects.length - 1; W >= 0; W--) {
                            var X = u.hoverRects[W];
                            if (x >= X.x && x <= X.x + X.w && S >= X.y && S <= X.y + X.h) {
                                D = X.key, R = X.cursor;
                                break;
                            }
                        } return { hitKey: D, hitCursor: R }; }; if (p) {
                            var _a = C(p.x, p.y), x = _a.hitKey, S = _a.hitCursor;
                            u.hoveredKeyByPointer.set(Wt, x), u.hoveredCursorByPointer.set(Wt, S);
                            var D = u.textDrags.has(Wt) || u.sliderDrags.has(Wt) || u.dialogDrags.has(Wt);
                            f_1.rotation = S != null || D ? Math.PI / 4 : 0;
                        } if (P) {
                            var _b = C(P.x, P.y), x = _b.hitKey, S = _b.hitCursor;
                            u.hoveredKeyByPointer.set(Ht, x), u.hoveredCursorByPointer.set(Ht, S);
                            var D = u.textDrags.has(Ht) || u.sliderDrags.has(Ht) || u.dialogDrags.has(Ht);
                            m_1.rotation = S != null || D ? Math.PI / 4 : 0;
                        } if (O) {
                            var _c = C(O.x, O.y), x = _c.hitKey, S = _c.hitCursor;
                            u.hoveredKeyByPointer.set(Ut, x), u.hoveredCursorByPointer.set(Ut, S);
                            var D = u.textDrags.has(Ut) || u.sliderDrags.has(Ut) || u.dialogDrags.has(Ut);
                            b_1.rotation = S != null || D ? Math.PI / 4 : 0;
                        } q_1(); };
                        u.harness.enabled && setInterval(function () {
                            var e_56, _a, e_57, _b;
                            var p = u.harness.activeUserPointerId, P = p === Wt ? Ht : p === Ht ? Ut : Wt;
                            if (u.harness.activeUserPointerId = P, u.lastMouse.has) {
                                var X = u.userCursorPos.get(p), z = u.userCursorPos.get(P);
                                u.userCursorPos.set(P, { x: u.lastMouse.x, y: u.lastMouse.y }), z ? u.userCursorPos.set(p, { x: z.x, y: z.y }) : X && u.userCursorPos.set(p, { x: X.x, y: X.y });
                            }
                            var O = u.textDrags.size > 0, C = u.sliderDrags.size > 0, x = u.dialogDrags.size > 0, S = u.scroll.draggingPointerId != null, D = u.color.draggingPointerId != null, R = !1;
                            try {
                                for (var _c = __values(u.iframeScroll.values()), _d = _c.next(); !_d.done; _d = _c.next()) {
                                    var X = _d.value;
                                    if (X.draggingPointerId != null) {
                                        R = !0;
                                        break;
                                    }
                                }
                            }
                            catch (e_56_1) { e_56 = { error: e_56_1 }; }
                            finally {
                                try {
                                    if (_d && !_d.done && (_a = _c.return)) _a.call(_c);
                                }
                                finally { if (e_56) throw e_56.error; }
                            }
                            var W = O || C || x || S || D || R;
                            u.textDrags.delete(Wt), u.textDrags.delete(Ht), u.textDrags.delete(Ut), u.sliderDrags.delete(Wt), u.sliderDrags.delete(Ht), u.sliderDrags.delete(Ut), u.dialogDrags.delete(Wt), u.dialogDrags.delete(Ht), u.dialogDrags.delete(Ut);
                            try {
                                for (var _f = __values([Wt, Ht, Ut]), _g = _f.next(); !_g.done; _g = _f.next()) {
                                    var X = _g.value;
                                    var z = u.numberHolds.get(X);
                                    z && (z.timeoutId != null && window.clearTimeout(z.timeoutId), z.intervalId != null && window.clearInterval(z.intervalId), u.numberHolds.delete(X));
                                }
                            }
                            catch (e_57_1) { e_57 = { error: e_57_1 }; }
                            finally {
                                try {
                                    if (_g && !_g.done && (_b = _f.return)) _b.call(_f);
                                }
                                finally { if (e_57) throw e_57.error; }
                            }
                            (u.scroll.draggingPointerId === Wt || u.scroll.draggingPointerId === Ht || u.scroll.draggingPointerId === Ut) && (u.scroll.draggingPointerId = null), (u.color.draggingPointerId === Wt || u.color.draggingPointerId === Ht || u.color.draggingPointerId === Ut) && (u.color.draggingPointerId = null), N_4(), W && (ot == null || ot());
                        }, u.harness.periodMs), !Y_1 && u.virtualCursor.enabled && i_1.ticker.add(function () { var S, D, R, W, X; var p = Math.max(0, i_1.ticker.deltaMS) / 1e3; c_1.visible = !0, u.virtualCursor.t += p; var P = i_1.renderer.width * .75, O = i_1.renderer.height * .25, C = u.virtualCursor.t * u.virtualCursor.speed, x = u.virtualCursor.radius; u.virtualCursor.x = P + Math.cos(C) * x, u.virtualCursor.y = O + Math.sin(C) * x, c_1.position.set(u.virtualCursor.x, u.virtualCursor.y); {
                            var z = j_1, K = u.virtualCursor.x, V = u.virtualCursor.y, nt = null, Q = null;
                            for (var dt = u.hoverRects.length - 1; dt >= 0; dt--) {
                                var yt = u.hoverRects[dt];
                                if (K >= yt.x && K <= yt.x + yt.w && V >= yt.y && V <= yt.y + yt.h) {
                                    nt = yt.key, Q = yt.cursor;
                                    break;
                                }
                            }
                            var it = (S = u.hoveredKeyByPointer.get(z)) != null ? S : null;
                            it !== nt && (it && ((R = (D = u.hoverHandlers.get(it)) == null ? void 0 : D.out) == null || R.call(D)), nt && ((X = (W = u.hoverHandlers.get(nt)) == null ? void 0 : W.over) == null || X.call(W)), u.hoveredKeyByPointer.set(z, nt)), u.hoveredCursorByPointer.set(z, Q);
                            var st = u.textDrags.has(z) || u.sliderDrags.has(z) || u.dialogDrags.has(z);
                            c_1.rotation = Q != null || st ? Math.PI / 4 : 0;
                        } }), u.virtualCursor.x = i_1.renderer.width * .75 + u.virtualCursor.radius, u.virtualCursor.y = i_1.renderer.height * .25, c_1.position.set(u.virtualCursor.x, u.virtualCursor.y), Kt() && L_1(), i_1.stage.on("pointerup", function (p) {
                            var e_58, _a;
                            var C, x, S;
                            var P = Ft(p), O = (x = (C = u.sliderDrags.get(P)) == null ? void 0 : C.key) != null ? x : null;
                            u.textDrags.delete(P), u.sliderDrags.delete(P), u.dialogDrags.delete(P), u.scroll.draggingPointerId === P && (u.scroll.draggingPointerId = null), u.color.draggingPointerId === P && (u.color.draggingPointerId = null);
                            try {
                                for (var _b = __values(u.iframeScroll.values()), _c = _b.next(); !_c.done; _c = _b.next()) {
                                    var D = _c.value;
                                    D.draggingPointerId === P && (D.draggingPointerId = null);
                                }
                            }
                            catch (e_58_1) { e_58 = { error: e_58_1 }; }
                            finally {
                                try {
                                    if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
                                }
                                finally { if (e_58) throw e_58.error; }
                            }
                            {
                                var D = u.numberHolds.get(P);
                                D && (D.timeoutId != null && window.clearTimeout(D.timeoutId), D.intervalId != null && window.clearInterval(D.intervalId), u.numberHolds.delete(P));
                            }
                            if (O) {
                                var D = (S = u.temporalYearOwners.get(O)) != null ? S : null;
                                if (D) {
                                    var R = u.temporals.get(D);
                                    R && R.openYear && (R.openYear = !1, u.temporals.set(D, R), ot == null || ot());
                                }
                            }
                            q_1();
                        }), i_1.stage.on("pointerupoutside", function (p) {
                            var e_59, _a;
                            var C, x, S;
                            var P = Ft(p), O = (x = (C = u.sliderDrags.get(P)) == null ? void 0 : C.key) != null ? x : null;
                            u.textDrags.delete(P), u.sliderDrags.delete(P), u.dialogDrags.delete(P), u.scroll.draggingPointerId === P && (u.scroll.draggingPointerId = null), u.color.draggingPointerId === P && (u.color.draggingPointerId = null);
                            try {
                                for (var _b = __values(u.iframeScroll.values()), _c = _b.next(); !_c.done; _c = _b.next()) {
                                    var D = _c.value;
                                    D.draggingPointerId === P && (D.draggingPointerId = null);
                                }
                            }
                            catch (e_59_1) { e_59 = { error: e_59_1 }; }
                            finally {
                                try {
                                    if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
                                }
                                finally { if (e_59) throw e_59.error; }
                            }
                            {
                                var D = u.numberHolds.get(P);
                                D && (D.timeoutId != null && window.clearTimeout(D.timeoutId), D.intervalId != null && window.clearInterval(D.intervalId), u.numberHolds.delete(P));
                            }
                            if (O) {
                                var D = (S = u.temporalYearOwners.get(O)) != null ? S : null;
                                if (D) {
                                    var R = u.temporals.get(D);
                                    R && R.openYear && (R.openYear = !1, u.temporals.set(D, R), ot == null || ot());
                                }
                            }
                            q_1();
                        }), a_2.on("pointerdown", function (p) { var V, nt, Q, it, st, dt; if ((p == null ? void 0 : p.button) === 2)
                            return; var P = Ft(p); if (P <= 0)
                            return; var O = (nt = (V = p.global) == null ? void 0 : V.x) != null ? nt : 0, C = (it = (Q = p.global) == null ? void 0 : Q.y) != null ? it : 0, x = u.scroll.track, S = u.scroll.thumb; if (!(O >= x.x && O <= x.x + x.w && C >= x.y && C <= x.y + x.h))
                            return; var R = Math.max(0, u.scroll.contentHeight - u.scroll.viewportHeight); if (R <= .5)
                            return; if (O >= S.x && O <= S.x + S.w && C >= S.y && C <= S.y + S.h) {
                            u.scroll.draggingPointerId = P, u.scroll.dragOffsetY = C - S.y, (st = p.stopPropagation) == null || st.call(p);
                            return;
                        } var X = Math.max(1, x.h - S.h), z = Math.max(x.y, Math.min(x.y + X, C - S.h / 2)), K = (z - x.y) / X; u.scroll.y = Math.max(0, Math.min(R, K * R)), u.scroll.draggingPointerId = P, u.scroll.dragOffsetY = C - z, ot == null || ot(), (dt = p.stopPropagation) == null || dt.call(p); }), i_1.stage.on("pointermove", function (p) {
                            var e_60, _a;
                            var D, R, W, X, z, K, V, nt, Q, it, st, dt, yt, Rt, Gt, at, Mt, Dt, Ct, Nt, mt, ft, ut, lt, Xt, vt, Bt, se;
                            var P = Number((W = (R = p == null ? void 0 : p.pointerId) != null ? R : (D = p == null ? void 0 : p.data) == null ? void 0 : D.pointerId) != null ? W : 1);
                            if (String((K = (z = p == null ? void 0 : p.pointerType) != null ? z : (X = p == null ? void 0 : p.data) == null ? void 0 : X.pointerType) != null ? K : "").toLowerCase() === "mouse" || P === 1) {
                                var ct = (nt = (V = p.global) == null ? void 0 : V.x) != null ? nt : 0, pt = (it = (Q = p.global) == null ? void 0 : Q.y) != null ? it : 0;
                                u.lastMouse.x = ct, u.lastMouse.y = pt, u.lastMouse.has = !0, u.primaryMousePointerId = P;
                                var Tt = u.harness.enabled ? u.harness.activeUserPointerId : P;
                                u.userCursorPos.set(Tt, { x: ct, y: pt }), N_4();
                            }
                            var x = Ft(p);
                            if (x <= 0)
                                return;
                            var S = !1;
                            {
                                var ct = u.textDrags.get(x);
                                if (ct) {
                                    var pt = ct.key, Tt = u.fieldBounds.get(pt), At = u.inputs.get(pt);
                                    if (Tt && At && typeof At.value == "string") {
                                        var Lt = Tt.isPassword ? "\u2022".repeat(At.value.length) : At.value, zt = ue(ce(Lt, Math.max(0, Tt.innerWidth), y_1), Tt.maxLines), te = ((dt = (st = p.global) == null ? void 0 : st.x) != null ? dt : 0) - Tt.x - Tt.innerLeft, J = ((Rt = (yt = p.global) == null ? void 0 : yt.y) != null ? Rt : 0) - Tt.y - Tt.innerTop, bt = Ee({ fullText: Lt, lines: zt, localX: te, localY: J, lineHeight: E_2, measure: y_1 });
                                        At.selections || (At.selections = new Map), At.selections.set(x, { start: ct.anchor, end: bt }), S = !0;
                                    }
                                }
                            }
                            {
                                var ct = u.sliderDrags.get(x);
                                if (ct) {
                                    var pt = ct.key, Tt = u.sliderBounds.get(pt);
                                    if (Tt) {
                                        var Lt = ((at = (Gt = p.global) == null ? void 0 : Gt.x) != null ? at : 0) - Tt.x, zt = Math.max(1, Tt.w - Tt.innerPad * 2), te = (Lt - Tt.innerPad) / zt, J = ye(u.sliders, pt, void 0);
                                        J.value = Math.max(0, Math.min(1, te)), S = !0;
                                    }
                                }
                            }
                            {
                                var ct = u.color.draggingPointerId;
                                if (ct != null && ct === x) {
                                    var pt = u.color.bounds;
                                    if (pt) {
                                        var Tt = (Dt = (Mt = p.global) == null ? void 0 : Mt.x) != null ? Dt : 0, At = (Nt = (Ct = p.global) == null ? void 0 : Ct.y) != null ? Nt : 0, Lt = Tt - pt.x, zt = At - pt.y, te = In({ lx: Lt, ly: zt, w: pt.w, h: pt.h });
                                        te && (u.color.rgb = te, u.color.pick = { x: Lt, y: zt }, S = !0);
                                    }
                                }
                            }
                            {
                                var ct = u.scroll.draggingPointerId;
                                if (ct != null && ct === x) {
                                    var pt = u.scroll.track, Tt = u.scroll.thumb, At = Math.max(0, u.scroll.contentHeight - u.scroll.viewportHeight);
                                    if (At > .5 && pt.h > 0 && Tt.h > 0) {
                                        var Lt = (ft = (mt = p.global) == null ? void 0 : mt.y) != null ? ft : 0, zt = Math.max(1, pt.h - Tt.h), J = (Math.max(pt.y, Math.min(pt.y + zt, Lt - u.scroll.dragOffsetY)) - pt.y) / zt;
                                        u.scroll.y = Math.max(0, Math.min(At, J * At)), S = !0;
                                    }
                                }
                            }
                            try {
                                for (var _b = __values(u.iframeScroll.values()), _c = _b.next(); !_c.done; _c = _b.next()) {
                                    var ct = _c.value;
                                    if (ct.draggingPointerId == null || ct.draggingPointerId !== x)
                                        continue;
                                    var pt = Math.max(0, ct.contentHeight - ct.viewportHeight);
                                    if (pt <= .5 || ct.track.h <= 0 || ct.thumb.h <= 0)
                                        continue;
                                    var Tt = (lt = (ut = p.global) == null ? void 0 : ut.y) != null ? lt : 0, At = Math.max(1, ct.track.h - ct.thumb.h), zt = (Math.max(ct.track.y, Math.min(ct.track.y + At, Tt - ct.dragOffsetY)) - ct.track.y) / At;
                                    ct.y = Math.max(0, Math.min(pt, zt * pt)), S = !0;
                                }
                            }
                            catch (e_60_1) { e_60 = { error: e_60_1 }; }
                            finally {
                                try {
                                    if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
                                }
                                finally { if (e_60) throw e_60.error; }
                            }
                            {
                                var ct = u.dialogDrags.get(x);
                                if (ct) {
                                    var pt = Qe(u.dialogs, ct.key), Tt = (vt = (Xt = p.global) == null ? void 0 : Xt.x) != null ? vt : 0, At = (se = (Bt = p.global) == null ? void 0 : Bt.y) != null ? se : 0;
                                    pt.x = ct.originX + (Tt - ct.startGX), pt.y = ct.originY + (At - ct.startGY);
                                    var Lt = u.dialogDragBounds.get(ct.key);
                                    Lt && (pt.x = Math.max(Lt.minX, Math.min(Lt.maxX, pt.x)), pt.y = Math.max(Lt.minY, Math.min(Lt.maxY, pt.y))), S = !0;
                                }
                            }
                            S && (ot == null || ot());
                        }), kt("main:input-listeners"), window.addEventListener("keydown", function (p) {
                            var Q, it, st, dt, yt, Rt, Gt;
                            var P = u.keyboardOwnerPointerId, O = (Q = u.focusedKeyByPointer.get(P)) != null ? Q : null;
                            if (!O)
                                return;
                            var C = u.inputs.get(O);
                            if (!C || typeof C.value != "string")
                                return;
                            if (C.selections || (C.selections = new Map), !C.selections.has(P)) {
                                var at = C.value.length;
                                C.selections.set(P, { start: at, end: at });
                            }
                            var x = C.selections.get(P), S = C.value.length, D = function (at) { return Math.max(0, Math.min(S, at)); }, R = D((it = x.start) != null ? it : S), W = D((st = x.end) != null ? st : R);
                            x.start = R, x.end = W;
                            var X = Math.min(R, W), z = Math.max(R, W), K = X !== z, V = function (at) { var Mt = Math.max(0, Math.min(C.value.length, at)); x.start = Mt, x.end = Mt; }, nt = function (at, Mt) { x.start = Math.max(0, Math.min(C.value.length, at)), x.end = Math.max(0, Math.min(C.value.length, Mt)); };
                            if (p.key.toLowerCase() === "a" && (p.ctrlKey || p.metaKey)) {
                                nt(0, C.value.length), p.preventDefault(), L_1();
                                return;
                            }
                            if (p.key === "ArrowLeft" || p.key === "ArrowRight") {
                                var at = p.key === "ArrowLeft" ? -1 : 1;
                                if (p.shiftKey) {
                                    var Mt = (dt = x.start) != null ? dt : S, Dt = ((yt = x.end) != null ? yt : Mt) + at;
                                    nt(Mt, Dt);
                                }
                                else
                                    V((K ? X : z) + at);
                                p.preventDefault(), h_2();
                                return;
                            }
                            if (p.key === "Home") {
                                p.shiftKey ? nt((Rt = x.start) != null ? Rt : S, 0) : V(0), p.preventDefault(), h_2();
                                return;
                            }
                            if (p.key === "End") {
                                p.shiftKey ? nt((Gt = x.start) != null ? Gt : 0, C.value.length) : V(C.value.length), p.preventDefault(), h_2();
                                return;
                            }
                            if (p.key === "Backspace") {
                                if (K)
                                    C.value = C.value.slice(0, X) + C.value.slice(z), V(X);
                                else {
                                    var at = z;
                                    at > 0 && (C.value = C.value.slice(0, at - 1) + C.value.slice(at), V(at - 1));
                                }
                                p.preventDefault(), h_2();
                                return;
                            }
                            if (p.key === "Enter") {
                                var at = "\n";
                                if (K)
                                    C.value = C.value.slice(0, X) + at + C.value.slice(z), V(X + at.length);
                                else {
                                    var Mt = z;
                                    C.value = C.value.slice(0, Mt) + at + C.value.slice(Mt), V(Mt + at.length);
                                }
                                p.preventDefault(), h_2();
                                return;
                            }
                            if (p.key === "Delete") {
                                if (K)
                                    C.value = C.value.slice(0, X) + C.value.slice(z), V(X);
                                else {
                                    var at = z;
                                    at < C.value.length && (C.value = C.value.slice(0, at) + C.value.slice(at + 1), V(at));
                                }
                                p.preventDefault(), h_2();
                                return;
                            }
                            if (p.key === "Escape") {
                                u.focusedKeyByPointer.set(P, null), h_2();
                                return;
                            }
                            if (p.key.length === 1 && !p.ctrlKey && !p.metaKey && !p.altKey) {
                                if (K)
                                    C.value = C.value.slice(0, X) + p.key + C.value.slice(z), V(X + 1);
                                else {
                                    var at = z;
                                    C.value = C.value.slice(0, at) + p.key + C.value.slice(at), V(at + 1);
                                }
                                p.preventDefault(), h_2();
                            }
                        }), window.addEventListener("resize", function () { h_2(), c_1.visible = u.virtualCursor.enabled; }), kt("main:done"), r && (window.__TRUEOS_PIXI_APP_READY__ = !0, window.__TRUEOS_PIXI_APP_ERROR__ = "", window.__TRUEOS_PIXI_APP_PHASE__ = "ready");
                        return [3 /*break*/, 10];
                    case 9:
                        n_3 = _d.sent();
                        window.__TRUEOS_PIXI_APP_READY__ = !1, window.__TRUEOS_PIXI_APP_ERROR__ = Ii(n_3);
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
    bs().then(function () { window.__TRUEOS_PIXI_APP_ERROR__ || (window.__TRUEOS_PIXI_APP_READY__ = !0, window.__TRUEOS_PIXI_APP_ERROR__ = "", window.__TRUEOS_PIXI_APP_PHASE__ = "ready"); }).catch(function (t) { var n; window.__TRUEOS_PIXI_APP_READY__ = !1, window.__TRUEOS_PIXI_APP_ERROR__ = Ii(t), console.error(t); var e = document.createElement("pre"); e.textContent = String((n = t == null ? void 0 : t.stack) != null ? n : t), document.body.appendChild(e); });
})();
