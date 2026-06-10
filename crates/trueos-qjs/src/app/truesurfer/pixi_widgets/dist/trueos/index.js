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
    var zi = Object.getOwnPropertyDescriptors;
    var er = Object.getOwnPropertySymbols;
    var Ki = Object.prototype.hasOwnProperty, ji = Object.prototype.propertyIsEnumerable;
    var kn = function (t, e, n) { return e in t ? nr(t, e, { enumerable: !0, configurable: !0, writable: !0, value: n }) : t[e] = n; }, qt = function (t, e) {
        var e_1, _a;
        for (var n in e || (e = {}))
            Ki.call(e, n) && kn(t, n, e[n]);
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
    }, xe = function (t, e) { return Yi(t, zi(e)); };
    var Vi = function (t, e) { return function () { return (t && (e = t(t = 0)), e); }; };
    var Ji = function (t, e) { for (var n in e)
        nr(t, n, { get: e[n], enumerable: !0 }); };
    var gt = function (t, e, n) { return kn(t, typeof e != "symbol" ? e + "" : e, n); };
    var Ze = function (t, e, n) { return new Promise(function (r, i) { var o = function (a) { try {
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
    var Ti = {};
    Ji(Ti, { default: function () { return zo; } });
    var zo, Ei = Vi(function () { zo = {}; });
    var $e = /** @class */ (function () {
        function $e(e, n) {
            if (e === void 0) { e = 0; }
            if (n === void 0) { n = 0; }
            gt(this, "x");
            gt(this, "y");
            this.x = Number(e) || 0, this.y = Number(n) || 0;
        }
        $e.prototype.set = function (e, n) {
            if (e === void 0) { e = 0; }
            if (n === void 0) { n = e; }
            this.x = Number(e) || 0, this.y = Number(n) || 0;
        };
        return $e;
    }()), bt = /** @class */ (function () {
        function bt(e, n, r, i) {
            if (e === void 0) { e = 0; }
            if (n === void 0) { n = 0; }
            if (r === void 0) { r = 0; }
            if (i === void 0) { i = 0; }
            gt(this, "x");
            gt(this, "y");
            gt(this, "width");
            gt(this, "height");
            this.x = Number(e) || 0, this.y = Number(n) || 0, this.width = Number(r) || 0, this.height = Number(i) || 0;
        }
        return bt;
    }()), Rn = /** @class */ (function () {
        function Rn() {
            gt(this, "parent");
            gt(this, "children");
            gt(this, "label");
            gt(this, "name");
            gt(this, "position");
            gt(this, "scale");
            gt(this, "pivot");
            gt(this, "visible");
            gt(this, "alpha");
            gt(this, "mask");
            gt(this, "rotation");
            gt(this, "zIndex");
            gt(this, "eventMode");
            gt(this, "cursor");
            gt(this, "hitArea");
            gt(this, "listeners");
            this.parent = null, this.position = new $e, this.scale = new $e(1, 1), this.pivot = new $e, this.visible = !0, this.alpha = 1, this.mask = null, this.rotation = 0, this.zIndex = 0, this.eventMode = null, this.cursor = null, this.hitArea = null, this.listeners = {};
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
    }()), Tt = /** @class */ (function (_super) {
        __extends(Tt, _super);
        function Tt() {
            var _this = _super.call(this) || this;
            gt(_this, "children");
            gt(_this, "sortableChildren");
            _this.children = [], _this.sortableChildren = !1;
            return _this;
        }
        Tt.prototype.addChild = function () {
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
        Tt.prototype.addChildAt = function (n, r) { var o; (o = n.parent) == null || o.removeChild(n), n.parent = this; var i = Math.max(0, Math.min(Number(r) | 0, this.children.length)); return this.children.splice(i, 0, n), n; };
        Tt.prototype.removeChild = function () {
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
        Tt.prototype.removeChildren = function (n, r) {
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
        Tt.prototype.setChildIndex = function (n, r) { var i = this.children.indexOf(n); if (i < 0)
            return; this.children.splice(i, 1); var o = Math.max(0, Math.min(Number(r) | 0, this.children.length)); this.children.splice(o, 0, n); };
        Tt.prototype.getChildIndex = function (n) { return this.children.indexOf(n); };
        Tt.prototype.getChildByLabel = function (n) { for (var r = 0; r < this.children.length; r += 1) {
            var i = this.children[r];
            if (i && i.label === n)
                return i;
        } return null; };
        return Tt;
    }(Rn)), yt = /** @class */ (function (_super) {
        __extends(yt, _super);
        function yt() {
            var _this = _super.call(this) || this;
            gt(_this, "commands");
            _this.commands = [];
            return _this;
        }
        yt.prototype.clear = function () { return this.commands.length = 0, this; };
        yt.prototype.rect = function (n, r, i, o) { return this.commands.push(["rect", n, r, i, o]), this; };
        yt.prototype.roundRect = function (n, r, i, o, s) {
            if (s === void 0) { s = 0; }
            return this.commands.push(["roundRect", n, r, i, o, s]), this;
        };
        yt.prototype.circle = function (n, r, i) { return this.commands.push(["circle", n, r, i]), this; };
        yt.prototype.ellipse = function (n, r, i, o) { return this.commands.push(["ellipse", n, r, i, o]), this; };
        yt.prototype.moveTo = function (n, r) { return this.commands.push(["moveTo", n, r]), this; };
        yt.prototype.lineTo = function (n, r) { return this.commands.push(["lineTo", n, r]), this; };
        yt.prototype.closePath = function () { return this.commands.push(["closePath"]), this; };
        yt.prototype.poly = function (n) { return this.commands.push(["poly", n]), this; };
        yt.prototype.fill = function (n) { return this.commands.push(["fill", n]), this; };
        yt.prototype.stroke = function (n) { return this.commands.push(["stroke", n]), this; };
        yt.prototype.image = function (n, r, i, o, s) { return this.commands.push(["image", n, r, i, o, s]), this; };
        yt.prototype.svg = function (n) { return this.commands.push(["svg", n]), this; };
        return yt;
    }(Tt)), jt = /** @class */ (function (_super) {
        __extends(jt, _super);
        function jt(n) {
            if (n === void 0) { n = ""; }
            var r, i;
            var _this = _super.call(this) || this;
            gt(_this, "_text");
            gt(_this, "_style");
            gt(_this, "_resolution");
            _this._text = "", _this._style = {}, _this._resolution = 1, typeof n == "string" ? _this._text = n : (_this._text = String((r = n.text) != null ? r : ""), _this._style = qt({}, (i = n.style) != null ? i : {}));
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
    }(Tt)), Me = /** @class */ (function () {
        function Me(e) {
            if (e === void 0) { e = {}; }
            gt(this, "options");
            this.options = e;
        }
        Me.prototype.addAttribute = function (e, n) { return this; };
        Me.prototype.destroy = function () { };
        return Me;
    }()), Qe = /** @class */ (function (_super) {
        __extends(Qe, _super);
        function Qe(n) {
            if (n === void 0) { n = {}; }
            var r, i;
            var _this = _super.call(this) || this;
            gt(_this, "geometry");
            gt(_this, "shader");
            _this.geometry = (r = n.geometry) != null ? r : new Me, _this.shader = (i = n.shader) != null ? i : new Be;
            return _this;
        }
        return Qe;
    }(Tt)), qe = /** @class */ (function () {
        function qe(e) {
            if (e === void 0) { e = {}; }
            gt(this, "options");
            this.options = e;
        }
        return qe;
    }()), Cn = { VERTEX: 1, COPY_DST: 2 }, Be = /** @class */ (function () {
        function Be(e) {
            if (e === void 0) { e = {}; }
            gt(this, "options");
            this.options = e;
        }
        return Be;
    }());
    var rr = "", ir = "", or = "", tn = /** @class */ (function () {
        function tn() {
            var _this = this;
            gt(this, "stage");
            gt(this, "screen");
            gt(this, "canvas");
            gt(this, "renderer");
            gt(this, "ticker");
            var e = Math.max(1, Number(globalThis.innerWidth || 1920) | 0), n = Math.max(1, Number(globalThis.innerHeight || 1080) | 0);
            this.stage = new Tt, this.screen = new bt(0, 0, e, n), this.canvas = document.createElement("canvas"), this.ticker = { stop: function () { }, add: function () { }, remove: function () { } }, this.renderer = { width: e, height: n, screen: this.screen, render: function (r) { return r; }, resize: function (r, i) { var o = Math.max(1, Number(r || e) | 0), s = Math.max(1, Number(i || n) | 0); _this.renderer.width = o, _this.renderer.height = s, _this.screen.width = o, _this.screen.height = s; } };
        }
        tn.prototype.init = function (e) { return Ze(this, null, function () { return __generator(this, function (_a) {
            return [2 /*return*/];
        }); }); };
        return tn;
    }());
    var ye = { fontFamily: "system-ui, -apple-system, Segoe UI, Arial", fontSize: 16, background: 16777215, text: 1118481, mutedText: 6710886, boxBorder: 14540253, hr: 13421772, control: { border: 0, focusBorder: 3900150, background: 16777215, accent: 3900150, radius: 0, button: { fill: 15921906, hoverFill: 15395562, activeFill: 14737632, border: 6710886, text: 1118481, radius: 0 }, progress: { border: 10066329, background: 16777215, fill: 6990335 }, table: { border: 10066329, cellBorder: 11579568, headerFill: 16250871 } } };
    var Ie = 24, _t = 1;
    function Kt(t) { var i, o; var e = t.wrapWidth, n = (i = t.wordWrap) != null ? i : e != null, r = (o = t.wordWrapWidth) != null ? o : e == null ? void 0 : Math.max(0, Math.ceil(e) + Ie); return new jt({ text: t.text, style: { fontFamily: t.fontFamily, fontSize: t.fontSize, fill: t.fill, fontWeight: t.bold ? "700" : "400", wordWrap: n, wordWrapWidth: r } }); }
    function On(t, e) { var n = t.children; if (!Array.isArray(n))
        return null; for (var r = 0; r < n.length; r += 1) {
        var i = n[r];
        if (i && i.label === e)
            return i;
    } return null; }
    function ae(t, e) { var n = On(t, e); if (n)
        return n; var r = new Tt; return r.label = e, t.addChild(r), r; }
    function Ot(t, e) { var n = On(t, e); if (n)
        return n; var r = new yt; return r.label = e, t.addChild(r), r; }
    function At(t, e, n) { var r = On(t, e); if (r)
        return r; var i = new jt({ text: "" }); return i.label = e, n == null || n(i), t.addChild(i), i; }
    function St(t) { t.clear(), t.removeAllListeners(), t.hitArea = null; }
    function Ut(t) { t.removeAllListeners(); }
    function le(t, e, n) {
        var r = String(t != null ? t : ""), i = [], o = 0;
        for (var s = 0; s <= r.length; s++) {
            if (!(s === r.length || r[s] === "\n"))
                continue;
            var a = o, d = s;
            if (a === d)
                i.push({ start: a, end: d, text: "" });
            else {
                var f = a, h = -1;
                for (var g = f; g < d; g++) {
                    r[g] === " " && (h = g);
                    var M = r.slice(f, g + 1);
                    if (n(M) <= e || g === f)
                        continue;
                    var c = h >= f ? h + 1 : g;
                    c <= f && (c = Math.min(d, f + 1)), i.push({ start: f, end: c, text: r.slice(f, c) }), f = c, g = f - 1, h = -1;
                }
                f <= d && i.push({ start: f, end: d, text: r.slice(f, d) });
            }
            o = s + 1;
        }
        return i;
    }
    function ce(t, e) { return e <= 0 ? [] : t.length <= e ? t : t.slice(0, e); }
    function Pe(t) { var e = t.fullText, n = t.lines, r = t.localX, i = t.localY, o = t.lineHeight, s = t.measure; if (n.length === 0)
        return 0; var l = Math.max(0, r), a = Math.max(0, i), d = Math.max(1, o), f = Math.max(0, Math.min(n.length - 1, Math.floor(a / d))), h = n[f], g = h.start, x = Number.POSITIVE_INFINITY; for (var M = h.start; M <= h.end; M++) {
        var c = s(e.slice(h.start, M)), y = Math.abs(c - l);
        y < x && (x = y, g = M);
    } return g; }
    function sr(t) { var M, c, y, b; var e = t.node, n = t.graphics, r = t.w, i = t.h, o = t.theme, s = Math.max(0, Math.round(r)), l = Math.max(0, Math.round(i)); n.rect(.5, .5, Math.max(0, s - 1), Math.max(0, l - 1)), n.fill(o.control.progress.background), n.stroke({ width: 1, color: o.control.progress.border }); var a = Number((c = (M = e.attrs) == null ? void 0 : M.value) != null ? c : "0"), d = Number((b = (y = e.attrs) == null ? void 0 : y.max) != null ? b : "1"), f = d > 0 ? Math.max(0, Math.min(1, a / d)) : 0, h = 3, g = Math.max(0, s - h * 2), x = Math.max(0, l - h * 2); n.rect(h, h, Math.max(0, g * f), x), n.fill(o.control.progress.fill); }
    function ar(t, e) { t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0), t.setHeight(14), t.setMinWidth(240); }
    function _e(t, e, n) { var l; var r = t.get(e); if (r)
        return r; var i = Number((l = n == null ? void 0 : n.value) != null ? l : "0"), o = Number.isFinite(i) ? i : 0, s = { value: Math.max(0, Math.min(1, o)) }; return t.set(e, s), s; }
    function lr(t, e) { t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0), t.setHeight(14), t.setMinWidth(240); }
    function cr(t) { var e = t.node, n = t.Yoga, r = t.measurer, i = n.Node.create(); return i.setPadding(n.EDGE_LEFT, 0), i.setPadding(n.EDGE_RIGHT, 0), i.setPadding(n.EDGE_TOP, 0), i.setPadding(n.EDGE_BOTTOM, 0), i.setMargin(n.EDGE_RIGHT, 6), i.setMeasureFunc(function () { var o = r.measure("100"); return { width: o.width, height: o.height }; }), { yogaNode: i, buildBox: function () { return ({ kind: "block", key: e.key, tagName: e.tagName, attrs: e.attrs, x: i.getComputedLeft(), y: i.getComputedTop(), width: i.getComputedWidth(), height: i.getComputedHeight(), children: [] }); } }; }
    function ur(t) { var d, f; var e = t.node, n = t.container, r = t.theme, i = t.sliderStates, o = (d = e.attrs) == null ? void 0 : d["data-slider-key"], s = null; if (o) {
        var h = i.get(o);
        if (h)
            s = h;
        else {
            var g = (f = e.attrs) == null ? void 0 : f["data-slider-init"];
            s = _e(i, o, g != null ? { value: String(g) } : void 0);
        }
    } var l = s ? Math.round(s.value * 100) : 0, a = At(n, "__pct", function (h) { h.style = { fontFamily: r.fontFamily, fontSize: r.fontSize, fill: r.text, fontWeight: "400", wordWrap: !1 }; }); a.text = String(l), a.position.set(0, _t); }
    function en(t) { var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.absX, l = t.absY, a = t.theme, d = t.sliderStates, f = t.sliderBounds, h = t.sliderDrags, g = t.requestPaint, x = e.key, M = x ? _e(d, x, e.attrs) : null, c = Math.max(0, Math.round(i)), y = Math.max(0, Math.round(o)), b = 3; x && f.set(x, { x: s, y: l, w: c, h: y, innerPad: b }), r.rect(.5, .5, Math.max(0, c - 1), Math.max(0, y - 1)), r.fill(a.control.progress.background), r.stroke({ width: 1, color: a.control.progress.border }); var P = M ? Math.max(0, Math.min(1, M.value)) : 0, I = Math.max(0, c - b * 2), W = Math.max(0, y - b * 2); r.rect(b, b, Math.max(0, I * P), W), r.fill(a.control.progress.fill); var G = b + I * P, w = W / 2; r.moveTo(G, b - w), r.lineTo(G, b + W + w), r.stroke({ width: 2, color: a.text }), x && (Ut(n), n.eventMode = "static", n.cursor = "pointer", n.hitArea = new bt(0, 0, Math.max(0, c), Math.max(0, y)), n.on("pointerdown", function (k) {
        var e_5, _a;
        var U, rt, ot, q, X, tt;
        if ((k == null ? void 0 : k.button) === 2)
            return;
        var E = t.getPointerId ? t.getPointerId(k) : Number((ot = (rt = k == null ? void 0 : k.pointerId) != null ? rt : (U = k == null ? void 0 : k.data) == null ? void 0 : U.pointerId) != null ? ot : 0);
        if (E <= 0)
            return;
        try {
            for (var _b = __values(h.entries()), _c = _b.next(); !_c.done; _c = _b.next()) {
                var _d = __read(_c.value, 2), T = _d[0], L = _d[1];
                L.key === x && T !== E && h.delete(T);
            }
        }
        catch (e_5_1) { e_5 = { error: e_5_1 }; }
        finally {
            try {
                if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
            }
            finally { if (e_5) throw e_5.error; }
        }
        h.set(E, { key: x });
        var _ = f.get(x), B = (X = (q = k.global) == null ? void 0 : q.x) != null ? X : 0, K = _ ? B - _.x : 0, V = _ ? Math.max(1, _.w - _.innerPad * 2) : 1, m = (K - ((tt = _ == null ? void 0 : _.innerPad) != null ? tt : 0)) / V, C = _e(d, x, e.attrs);
        C.value = Math.max(0, Math.min(1, m)), g == null || g();
    })); }
    function dr(t) { var W; var e = t.node, n = t.container, r = t.w, i = t.h, o = t.theme, s = t.detailsOpen, l = t.requestRerender, a = (W = e.attrs) == null ? void 0 : W["data-details-key"], d = e.attrs ? Object.prototype.hasOwnProperty.call(e.attrs, "data-details-open") : !1, f = a && s.has(a) ? s.get(a) === !0 : d, h = function (G) { var E; if (!a || (G == null ? void 0 : G.button) === 2)
        return; var k = !(s.has(a) ? s.get(a) === !0 : d); s.set(a, k), l == null || l(), (E = G == null ? void 0 : G.stopPropagation) == null || E.call(G); }, g = 16, x = Ot(n, "__arrow"); St(x); var M = 2, c = 3, y = c, b = c, P = g - c, I = g - c; f ? (x.moveTo(y, b), x.lineTo((y + P) / 2, I), x.lineTo(P, b)) : (x.moveTo(y, b), x.lineTo(P, (b + I) / 2), x.lineTo(y, I)), x.stroke({ width: M, color: o.text }), x.position.set(4, Math.max(0, (i - g) / 2)), x.eventMode = "static", x.cursor = "pointer", x.hitArea = new bt(0, 0, g + 8, g + 8), x.on("pointerdown", h), a && (Ut(n), n.eventMode = "static", n.cursor = "pointer", n.hitArea = new bt(0, 0, Math.max(0, r), Math.max(0, i)), n.on("pointerdown", h)); }
    function hr(t, e) { t.setFlexDirection(e.FLEX_DIRECTION_ROW), t.setAlignItems(e.ALIGN_CENTER), t.setJustifyContent(e.JUSTIFY_SPACE_BETWEEN), t.setPadding(e.EDGE_TOP, 6), t.setPadding(e.EDGE_BOTTOM, 6), t.setPadding(e.EDGE_LEFT, 26), t.setPadding(e.EDGE_RIGHT, 12), t.setMinHeight(36); }
    function mr(t, e) { t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0); }
    function fr(t, e) { var i, o, s; if (!t || t.tagName !== "details" || !t.key)
        return (i = t == null ? void 0 : t.children) != null ? i : []; var n = t.attrs ? Object.prototype.hasOwnProperty.call(t.attrs, "open") : !1; return (e.has(t.key) ? e.get(t.key) === !0 : n) ? (o = t.children) != null ? o : [] : ((s = t.children) != null ? s : []).filter(function (l) { return l && l.kind === "block" && l.tagName === "summary"; }); }
    function pr(t) { var e = t.graphics, n = t.w, r = t.theme; e.rect(0, 0, Math.round(n), 1), e.fill(r.hr); }
    function gr(t, e) { t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0), t.setMargin(e.EDGE_TOP, 2), t.setMargin(e.EDGE_BOTTOM, 2), t.setHeight(1); }
    function br(t) { var y, b; var e = t.container, n = t.graphics, r = t.w, i = t.h, o = t.label, s = t.theme, l = t.registerHoverHandlers, a = function (P) { n.clear(); var I = 1, W = I / 2; s.control.button.radius > 0 ? n.roundRect(W, W, Math.max(0, r - I), Math.max(0, i - I), s.control.button.radius) : n.rect(W, W, Math.max(0, r - I), Math.max(0, i - I)), n.fill(P), n.stroke({ width: I, color: s.control.button.border }); }; a(s.control.button.fill); var d = At(e, "__label", function (P) { P.style = { fontFamily: s.fontFamily, fontSize: s.fontSize, fill: s.control.button.text, fontWeight: "400", wordWrap: !1, wordWrapWidth: 0 }; }), f = String(o != null ? o : "").trim(); d.text = f, d.visible = f.length > 0, d.style = xe(qt({}, d.style), { fontFamily: s.fontFamily, fontSize: s.fontSize, fill: s.control.button.text, wordWrap: !1, wordWrapWidth: Math.max(0, Math.ceil(r - 16)) }); var h = Number((y = d.width) != null ? y : 0), g = Number((b = d.height) != null ? b : 0), x = s.fontSize * 1.25; d.position.set(h > 0 ? Math.max(8, Math.floor((r - h) / 2)) : 8, Math.max(0, Math.floor((i - (g > 0 ? g : x)) / 2)) + _t); var M = function () { return a(s.control.button.hoverFill); }, c = function () { return a(s.control.button.fill); }; l == null || l({ over: M, out: c }), Ut(e), e.eventMode = "static", e.cursor = "pointer", e.on("pointerover", M), e.on("pointerout", c), e.on("pointerdown", function () { return a(s.control.button.activeFill); }), e.on("pointerup", function () { return a(s.control.button.hoverFill); }); }
    function xr(t, e) { t.setFlexDirection(e.FLEX_DIRECTION_ROW), t.setPadding(e.EDGE_TOP, 6), t.setPadding(e.EDGE_BOTTOM, 6), t.setMinHeight(36), t.setMinWidth(100), t.setAlignItems(e.ALIGN_CENTER), t.setJustifyContent(e.JUSTIFY_CENTER); }
    function yr(t) { var e = t.graphics, n = t.w, r = t.h, i = t.boxBorder, o = Math.max(0, Math.round(n)), s = Math.max(0, Math.round(r)); e.rect(0, 0, o, s), e.stroke({ width: 1, color: i, alignment: 0 }); }
    function _r(t) { var e = t.nodeTag, n = t.graphics, r = t.w, i = t.h, o = t.theme; e === "th" && (n.rect(0, 0, r, i), n.fill(o.control.table.headerFill)), n.rect(0, 0, r, i), n.stroke({ width: 1, color: o.control.table.cellBorder }); }
    function wr(t, e) { t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0); }
    function Tr(t, e) { t.setFlexDirection(e.FLEX_DIRECTION_ROW), t.setFlexWrap(e.WRAP_NO_WRAP), t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0), t.setMargin(e.EDGE_BOTTOM, 0); }
    function Er(t, e) { t.setFlexGrow(1), t.setFlexShrink(1), t.setMinWidth(80), t.setPadding(e.EDGE_LEFT, 8), t.setPadding(e.EDGE_RIGHT, 8), t.setPadding(e.EDGE_TOP, 6), t.setPadding(e.EDGE_BOTTOM, 6), t.setMargin(e.EDGE_BOTTOM, 0); }
    function An(t) { var e = String(t != null ? t : "").toLowerCase(); if (e.length !== 2 || e.charAt(0) !== "h")
        return !1; var n = e.charCodeAt(1); return n >= 49 && n <= 54; }
    function Mr(t, e) { t.setPadding(e.EDGE_TOP, 6), t.setPadding(e.EDGE_BOTTOM, 6), t.setMinHeight(36), t.setJustifyContent(e.JUSTIFY_CENTER); }
    function Ir(t, e) {
        var n = Math.max(1, Math.floor(t)), r = Math.max(1, Math.floor(e));
        return "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<svg viewBox=\"0 0 ".concat(n, " ").concat(r, "\" xmlns=\"http://www.w3.org/2000/svg\">\n  <rect x=\"0\" y=\"0\" width=\"").concat(n, "\" height=\"").concat(r, "\" fill=\"#f6f6f6\"/>\n  <rect x=\"0.5\" y=\"0.5\" width=\"").concat(Math.max(0, n - 1), "\" height=\"").concat(Math.max(0, r - 1), "\" fill=\"none\" stroke=\"#999\"/>\n  <path d=\"M2 2 L").concat(Math.max(2, n - 2), " ").concat(Math.max(2, r - 2), "\" stroke=\"#c8c8c8\"/>\n  <path d=\"M").concat(Math.max(2, n - 2), " 2 L2 ").concat(Math.max(2, r - 2), "\" stroke=\"#c8c8c8\"/>\n</svg>");
    }
    function Pr(_a) {
        var _b = _a === void 0 ? {} : _a, _c = _b.ring, t = _c === void 0 ? 34 : _c, _d = _b.core, e = _d === void 0 ? 14 : _d, _f = _b.hueA, n = _f === void 0 ? "#00e5ff" : _f, _g = _b.hueB, r = _g === void 0 ? "#ff2bd6" : _g;
        var i = Math.max(0, t - 10), o = Math.max(0, e * .35);
        return "\n<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 100 100\">\n  <rect width=\"100\" height=\"100\" fill=\"#ffffff\"/>\n  <rect width=\"100\" height=\"100\" fill=\"".concat(n, "\" opacity=\"0.08\"/>\n\n  <circle cx=\"50\" cy=\"50\" r=\"").concat(t, "\" fill=\"none\" stroke=\"").concat(r, "\" stroke-width=\"4\" opacity=\"0.95\"/>\n  <circle cx=\"50\" cy=\"50\" r=\"").concat(i, "\" fill=\"none\" stroke=\"").concat(n, "\" stroke-width=\"1\" opacity=\"0.35\"/>\n\n  <circle cx=\"50\" cy=\"50\" r=\"").concat(e, "\" fill=\"").concat(n, "\" opacity=\"0.9\"/>\n  <circle cx=\"43\" cy=\"43\" r=\"").concat(o, "\" fill=\"#ffffff\" opacity=\"0.55\"/>\n\n  <path d=\"M50 16 L52 22 L58 24 L52 26 L50 32 L48 26 L42 24 L48 22 Z\" fill=\"#ffffff\" opacity=\"0.85\"/>\n  <path d=\"M82 52 L85 56 L90 57 L85 58 L82 62 L79 58 L74 57 L79 56 Z\" fill=\"#ffffff\" opacity=\"0.70\"/>\n  <path d=\"M20 70 L22 74 L27 75 L22 76 L20 80 L18 76 L13 75 L18 74 Z\" fill=\"#ffffff\" opacity=\"0.65\"/>\n</svg>\n");
    }
    var Sr = new Map;
    function We() { var t = globalThis; return !0; }
    function Cr(t) { var e = globalThis, n = String(t != null ? t : "").trim(); if (!n)
        return null; var r = e.__TRUEOS_READY_IMAGE_TEXTURE_CACHE__; (!r || typeof r.get != "function" || typeof r.set != "function") && (r = new Map, e.__TRUEOS_READY_IMAGE_TEXTURE_CACHE__ = r); var i = r.get(n); if (i)
        return i; var o = e.__trueosResolveReadyImageTexture; if (typeof o != "function")
        return null; var s = o(n); if (s && typeof s.then == "function") {
        var l = s.then(function (a) { return (r.set(n, a), a); }).catch(function (a) { throw r.delete(n), a; });
        return r.set(n, l), l;
    } return r.set(n, s), s; }
    function Zi(t) { var s; var e = globalThis, n = String(t != null ? t : "").trim(); if (!n)
        return null; var r = e.__trueosPeekReadyImageTexture; if (typeof r != "function")
        return null; var i = r(n); return !i || typeof i.then == "function" ? null : Math.max(0, Number((s = i == null ? void 0 : i.texId) != null ? s : 0) | 0) > 0 ? i : null; }
    function Qi(t, e) { var n = globalThis, r = String(t != null ? t : "").trim(); if (!r || !e)
        return; var i = n.__TRUEOS_READY_IMAGE_TEXTURE_CACHE__; (!i || typeof i.get != "function" || typeof i.set != "function") && (i = new Map, n.__TRUEOS_READY_IMAGE_TEXTURE_CACHE__ = i), i.set(r, e); }
    function Or(t, e) { var r, i, o, s, l; var n = Math.max(0, Number((r = e == null ? void 0 : e.texId) != null ? r : 0) | 0); if (n <= 0)
        throw new Error("image texture missing"); t.state = "ready", t.texId = n, t.width = Math.max(0, Number((o = (i = e == null ? void 0 : e.pixelWidth) != null ? i : e == null ? void 0 : e.width) != null ? o : 0) | 0), t.height = Math.max(0, Number((l = (s = e == null ? void 0 : e.pixelHeight) != null ? s : e == null ? void 0 : e.height) != null ? l : 0) | 0); }
    function qi(t, e) { var n = Zi(t) || Cr(t); return !n || typeof n.then == "function" ? !1 : (Or(e, n), Qi(t, n), !0); }
    function kr(t, e) { var n = String(t != null ? t : "").trim(); if (!n)
        return null; var r = Sr.get(n); if (r) {
        if (We() && r.state === "loading")
            try {
                qi(n, r);
            }
            catch (l) {
                r.state = "error";
            }
        return r;
    } if (We())
        return null; var i = { state: "loading", texId: 0, width: 0, height: 0 }; Sr.set(n, i); var o = function (l) { Or(i, l), We() || e == null || e(); }, s = function () { i.state = "error", We() || e == null || e(); }; try {
        var l = Cr(n);
        if (!l)
            return i;
        if (l && typeof l.then == "function") {
            if (We())
                return i;
            l.then(o).catch(s);
        }
        else
            o(l);
    }
    catch (l) {
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
    function Ar(t) { var W, G, w, k; var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.theme, l = t.requestRerender, a = (G = (W = e.attrs) == null ? void 0 : W.alt) != null ? G : "", d = (k = (w = e.attrs) == null ? void 0 : w.src) != null ? k : "", f = d.trim().length > 0, h = a.trim().length > 0 ? a : d.trim().length > 0 ? d : "img", g = r.image, x = f ? kr(d, l) : null; if ((x == null ? void 0 : x.state) === "ready" && x.texId > 0 && typeof g == "function") {
        g.call(r, x.texId, 0, 0, Math.max(0, i), Math.max(0, o));
        var E = n.getChildByLabel ? n.getChildByLabel("__label") : n.children.find(function (_) { return (_ == null ? void 0 : _.label) === "__label"; });
        E && (E.visible = !1);
        return;
    } var M = f ? to(d) : null, c = eo(M != null ? M : f ? Ir(i, o) : Pr({ ring: 34, core: 14 })), y = Ot(n, "__svg"), b = kr(no(c), l); if ((b == null ? void 0 : b.state) === "ready" && b.texId > 0 && typeof y.image == "function") {
        var E = "texture:".concat(b.texId, ":").concat(Math.round(i), "x").concat(Math.round(o));
        if (y.__key !== E && (St(y), y.image(b.texId, 0, 0, Math.max(0, i), Math.max(0, o)), y.__key = E), y.scale.set(1), y.position.set(0, 0), !f) {
            var _ = n.getChildByLabel ? n.getChildByLabel("__label") : n.children.find(function (B) { return (B == null ? void 0 : B.label) === "__label"; });
            _ && (_.visible = !1);
            return;
        }
        if (h.trim().length > 0) {
            var _ = At(n, "__label", function (B) { B.style = { fontFamily: s.fontFamily, fontSize: s.fontSize, fill: s.mutedText, fontWeight: "400", wordWrap: !0, wordWrapWidth: 0 }; });
            _.text = h, _.style.wordWrapWidth = Math.max(0, Math.ceil(i - 16) + Ie), _.position.set(8, 8 + _t), _.visible = !0;
        }
        return;
    }
    else
        St(y); var P = y.svg; if (0 && y.__key !== E)
        try { }
        catch (B) { } r.rect(.5, .5, Math.max(0, i - 1), Math.max(0, o - 1)), r.fill(16185078), r.stroke({ width: 1, color: s.control.border }); var I = At(n, "__label", function (E) { E.style = { fontFamily: s.fontFamily, fontSize: s.fontSize, fill: s.mutedText, fontWeight: "400", wordWrap: !0, wordWrapWidth: 0 }; }); I.text = h, I.style.wordWrapWidth = Math.max(0, Math.ceil(i - 16) + Ie), I.position.set(8, 8 + _t); }
    function Dr(t, e, n) { var d, f, h, g; t.setPadding(n.EDGE_LEFT, 0), t.setPadding(n.EDGE_RIGHT, 0), t.setPadding(n.EDGE_TOP, 0), t.setPadding(n.EDGE_BOTTOM, 0); var r = Number((f = (d = e.attrs) == null ? void 0 : d.width) != null ? f : "0"), i = Number((g = (h = e.attrs) == null ? void 0 : h.height) != null ? g : "0"), o = Number.isFinite(r) && r > 0, s = Number.isFinite(i) && i > 0, l = o ? r : 240, a = s ? i : 140; (o || s) && (t.setAlignSelf(n.ALIGN_FLEX_START), t.setFlexGrow(0), t.setFlexShrink(0)), t.setWidth(l), t.setHeight(a), t.setMinWidth(120), t.setMinHeight(80); }
    var Nr = new Map;
    function He() { var t = globalThis; return !0; }
    function vr(t) { var e = globalThis, n = String(t != null ? t : "").trim(); if (!n)
        return null; var r = e.__TRUEOS_READY_IMAGE_TEXTURE_CACHE__; (!r || typeof r.get != "function" || typeof r.set != "function") && (r = new Map, e.__TRUEOS_READY_IMAGE_TEXTURE_CACHE__ = r); var i = r.get(n); if (i)
        return i; var o = e.__trueosResolveReadyImageTexture; if (typeof o != "function")
        return null; var s = o(n); if (s && typeof s.then == "function") {
        var l = s.then(function (a) { return (r.set(n, a), a); }).catch(function (a) { throw r.delete(n), a; });
        return r.set(n, l), l;
    } return r.set(n, s), s; }
    function ro(t) { var s; var e = globalThis, n = String(t != null ? t : "").trim(); if (!n)
        return null; var r = e.__trueosPeekReadyImageTexture; if (typeof r != "function")
        return null; var i = r(n); return !i || typeof i.then == "function" ? null : Math.max(0, Number((s = i == null ? void 0 : i.texId) != null ? s : 0) | 0) > 0 ? i : null; }
    function io(t, e) { var n = globalThis, r = String(t != null ? t : "").trim(); if (!r || !e)
        return; var i = n.__TRUEOS_READY_IMAGE_TEXTURE_CACHE__; (!i || typeof i.get != "function" || typeof i.set != "function") && (i = new Map, n.__TRUEOS_READY_IMAGE_TEXTURE_CACHE__ = i), i.set(r, e); }
    function Gr(t, e) { var r, i, o, s, l; var n = Math.max(0, Number((r = e == null ? void 0 : e.texId) != null ? r : 0) | 0); if (n <= 0)
        throw new Error("svg texture missing"); t.state = "ready", t.texId = n, t.width = Math.max(0, Number((o = (i = e == null ? void 0 : e.pixelWidth) != null ? i : e == null ? void 0 : e.width) != null ? o : 0) | 0), t.height = Math.max(0, Number((l = (s = e == null ? void 0 : e.pixelHeight) != null ? s : e == null ? void 0 : e.height) != null ? l : 0) | 0); }
    function oo(t, e) { var n = ro(t) || vr(t); return !n || typeof n.then == "function" ? !1 : (Gr(e, n), io(t, n), !0); }
    function so(t) { return Lr(Lr(String(t), "tspan"), "text"); }
    function Lr(t, e) { var n = "", r = 0, i = t.toLowerCase(), o = "<" + e, s = "</" + e; for (; r < t.length;) {
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
    function $r(t) { var e = String(t), r = e.toLowerCase().indexOf("viewbox"); if (r < 0)
        return null; var i = e.indexOf("=", r + 7); if (i < 0)
        return null; var o = i + 1; for (; o < e.length;) {
        var x = e.charCodeAt(o);
        if (x !== 32 && x !== 9 && x !== 10 && x !== 13 && x !== 12)
            break;
        o += 1;
    } var s = e.charAt(o); if (s !== '"' && s !== "'")
        return null; var l = e.indexOf(s, o + 1); if (l < 0)
        return null; var a = ao(e.slice(o + 1, l)); if (a.length < 4)
        return null; var d = Number(a[0]), f = Number(a[1]), h = Number(a[2]), g = Number(a[3]); return ![d, f, h, g].every(function (x) { return Number.isFinite(x); }) || h <= 0 || g <= 0 ? null : { minX: d, minY: f, w: h, h: g }; }
    function ao(t) { var e = [], n = ""; for (var r = 0; r < t.length; r += 1) {
        var i = t.charCodeAt(r);
        i === 32 || i === 9 || i === 10 || i === 13 || i === 12 ? n.length > 0 && (e.push(n), n = "") : n += t.charAt(r);
    } return n.length > 0 && e.push(n), e; }
    function lo(t, e) { var n = String(t != null ? t : ""); if (!n.trim())
        return null; var r = Nr.get(n), i = "data:image/svg+xml;charset=utf-8,".concat(encodeURIComponent(n)); if (r) {
        if (He() && r.state === "loading")
            try {
                oo(i, r);
            }
            catch (a) {
                r.state = "error";
            }
        return r;
    } if (He())
        return null; var o = { state: "loading", texId: 0, width: 0, height: 0 }; Nr.set(n, o); var s = function (a) { Gr(o, a), He() || e == null || e(); }, l = function () { o.state = "error", He() || e == null || e(); }; try {
        var a = vr(i);
        if (!a)
            return o;
        if (a && typeof a.then == "function") {
            if (He())
                return o;
            a.then(s).catch(l);
        }
        else
            s(a);
    }
    catch (a) {
        l();
    } return o; }
    function co(t, e, n) { var r = Math.max(0, e), i = Math.max(0, n), o = $r(t); if (!o || r <= 0 || i <= 0)
        return { x: 0, y: 0, w: r, h: i }; var s = r / o.w, l = i / o.h, a = Math.min(s, l), d = Math.max(0, o.w * a), f = Math.max(0, o.h * a); return { x: Math.max(0, (r - d) / 2), y: Math.max(0, (i - f) / 2), w: d, h: f }; }
    function Br(t, e, n) { var d, f, h, g; t.setPadding(n.EDGE_LEFT, 0), t.setPadding(n.EDGE_RIGHT, 0), t.setPadding(n.EDGE_TOP, 0), t.setPadding(n.EDGE_BOTTOM, 0); var r = Number((f = (d = e.attrs) == null ? void 0 : d.width) != null ? f : "0"), i = Number((g = (h = e.attrs) == null ? void 0 : h.height) != null ? g : "0"), o = Number.isFinite(r) && r > 0, s = Number.isFinite(i) && i > 0, l = o ? r : 300, a = s ? i : 150; (o || s) && (t.setAlignSelf(n.ALIGN_FLEX_START), t.setFlexGrow(0), t.setFlexShrink(0)), t.setWidth(l), t.setHeight(a), t.setMinWidth(Math.min(120, l)), t.setMinHeight(Math.min(80, a)); }
    function Wr(t) { var e = t.svgMarkup, n = t.container, r = t.w, i = t.h, o = t.requestRerender, s = so(e), l = Ot(n, "__svg"), a = l.__svgString, d = l.__w, f = l.__h, h = a !== s, g = lo(s, o); if (l.scale.set(1), l.position.set(0, 0), (g == null ? void 0 : g.state) === "ready" && g.texId > 0 && typeof l.image == "function") {
        if (h || d !== r || f !== i || l.__texId !== g.texId) {
            var M = co(s, r, i);
            St(l), l.image(g.texId, M.x, M.y, M.w, M.h), l.__svgString = s, l.__w = r, l.__h = i, l.__texId = g.texId;
        }
        return;
    } St(l); return; if (typeof x == "function") {
        if (h || d !== r || f !== i) {
            St(l);
            var c = void 0;
            try {
                c = x.call(l, s);
            }
            catch (y) {
                c = null;
            }
            c && typeof c.then == "function" && c.then(function () { return o == null ? void 0 : o(); }).catch(function () { }), l.__svgString = s, l.__w = r, l.__h = i;
        }
        var M = $r(s);
        if (M) {
            var c = r / M.w, y = i / M.h, b = Math.min(c, y), P = M.w * b, I = M.h * b;
            l.scale.set(b), l.position.set(-M.minX * b + (r - P) / 2, -M.minY * b + (i - I) / 2);
        }
        return;
    } }
    function Hr(t, e, n) { var d, f, h, g; t.setPadding(n.EDGE_LEFT, 0), t.setPadding(n.EDGE_RIGHT, 0), t.setPadding(n.EDGE_TOP, 0), t.setPadding(n.EDGE_BOTTOM, 0); var r = Number((f = (d = e.attrs) == null ? void 0 : d.width) != null ? f : "0"), i = Number((g = (h = e.attrs) == null ? void 0 : h.height) != null ? g : "0"), o = Number.isFinite(r) && r > 0, s = Number.isFinite(i) && i > 0, l = o ? r : 300, a = s ? i : 150; (o || s) && (t.setAlignSelf(n.ALIGN_FLEX_START), t.setFlexGrow(0), t.setFlexShrink(0)), t.setWidth(l), t.setHeight(a), t.setMinWidth(Math.min(120, l)), t.setMinHeight(Math.min(80, a)); }
    function Fr(t) { var e = t.graphics, n = t.container, r = t.w, i = t.h, o = t.theme, s = 1, l = s / 2; e.rect(l, l, Math.max(0, r - s), Math.max(0, i - s)), e.fill(16777215), e.stroke({ width: s, color: o.control.border, alignment: 0 }), e.moveTo(6, i - 6), e.lineTo(r - 6, 6), e.stroke({ width: 1, color: 0, alpha: .1 }); var a = Kt({ text: "canvas", fontFamily: o.fontFamily, fontSize: Math.max(10, Math.floor(o.fontSize * .85)), fill: o.mutedText, wordWrap: !1 }); a.position.set(8, 8 + _t), n.addChild(a); }
    function Ur(t, e, n) { var f, h, g, x, M, c; var r = String((h = (f = e.attrs) == null ? void 0 : f["data-root"]) != null ? h : "") === "1"; if (t.setFlexDirection(n.FLEX_DIRECTION_COLUMN), t.setAlignItems(n.ALIGN_STRETCH), r) {
        t.setPadding(n.EDGE_LEFT, 0), t.setPadding(n.EDGE_RIGHT, 0), t.setPadding(n.EDGE_TOP, 0), t.setPadding(n.EDGE_BOTTOM, 0), t.setAlignSelf(n.ALIGN_STRETCH), t.setFlexGrow(1), t.setFlexShrink(1), t.setMinWidth(0), t.setMinHeight(0);
        return;
    } t.setPadding(n.EDGE_LEFT, 8), t.setPadding(n.EDGE_RIGHT, 8), t.setPadding(n.EDGE_BOTTOM, 8), t.setPadding(n.EDGE_TOP, 34); var i = Number((x = (g = e.attrs) == null ? void 0 : g.width) != null ? x : "0"), o = Number((c = (M = e.attrs) == null ? void 0 : M.height) != null ? c : "0"), s = Number.isFinite(i) && i > 0, l = Number.isFinite(o) && o > 0, a = s ? i : 420, d = l ? o : 240; (s || l) && (t.setAlignSelf(n.ALIGN_FLEX_START), t.setFlexGrow(0), t.setFlexShrink(0)), t.setWidth(a), t.setHeight(d), t.setMinWidth(Math.min(200, a)), t.setMinHeight(Math.min(160, d)); }
    function Xr(t) { var x, M, c, y; var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.theme; if (String((M = (x = e.attrs) == null ? void 0 : x["data-root"]) != null ? M : "") === "1")
        return; var a = 1, d = a / 2; r.rect(d, d, Math.max(0, i - a), Math.max(0, o - a)), r.fill(16777215), r.stroke({ width: a, color: s.control.border, alignment: 0 }), r.rect(d, d, Math.max(0, i - a), 26), r.fill({ color: 0, alpha: .04 }); var h = String((y = (c = e.attrs) == null ? void 0 : c.srcdoc) != null ? y : "").trim().length > 0 ? "srcdoc" : "empty", g = At(n, "__title", function (b) { b.style = { fontFamily: s.fontFamily, fontSize: Math.max(10, Math.floor(s.fontSize * .85)), fill: s.mutedText, fontWeight: "400", wordWrap: !1 }; }); g.text = "iframe (".concat(h, ")"), g.position.set(8, 6 + _t), n.eventMode = "static", n.cursor = "default", n.hitArea = new bt(0, 0, Math.max(0, i), Math.max(0, o)); }
    function Yr(t, e, n) { var i, o; var r = ((o = (i = e.attrs) == null ? void 0 : i.type) != null ? o : "text").toLowerCase(); r === "checkbox" || r === "radio" ? (t.setPadding(n.EDGE_LEFT, 0), t.setPadding(n.EDGE_RIGHT, 0), t.setPadding(n.EDGE_TOP, 0), t.setPadding(n.EDGE_BOTTOM, 0), t.setWidth(16), t.setHeight(16), t.setMinWidth(16), t.setMargin(n.EDGE_RIGHT, 6)) : (t.setPadding(n.EDGE_TOP, 6), t.setPadding(n.EDGE_BOTTOM, 6), t.setHeight(36), t.setMinHeight(36), t.setMinWidth(220)); }
    function zr(t) {
        var e_6, _a, e_7, _b;
        var K, V, m, C, U, rt, ot, q, X, tt;
        var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.absX, l = t.absY, a = t.theme, d = t.textMeasure, f = t.uiState, h = t.getOrInitInputState, g = t.clamp, x = t.radioGroups, M = t.textDrags, c = t.requestPaint, y = ((V = (K = e.attrs) == null ? void 0 : K.type) != null ? V : "text").toLowerCase(), b = e.key, P = b ? h(b, e.attrs) : void 0, I = (m = t.showCaret) != null ? m : !1, W = (C = t.caretPointerId) != null ? C : null, G = t.focusColor, w = t.getCursorColor;
        n.sortableChildren = !0;
        try {
            for (var _c = __values(n.children), _d = _c.next(); !_d.done; _d = _c.next()) {
                var T = _d.value;
                var L = T.label;
                L && (L.startsWith("__sel:") || L === "__caret") && (T.visible = !1);
            }
        }
        catch (e_6_1) { e_6 = { error: e_6_1 }; }
        finally {
            try {
                if (_d && !_d.done && (_a = _c.return)) _a.call(_c);
            }
            finally { if (e_6) throw e_6.error; }
        }
        var k = 8, E = 6 + _t, _ = 5, B = a.fontSize * 1.25;
        if (y === "checkbox")
            r.rect(.5, .5, Math.max(0, i - 1), Math.max(0, o - 1)), r.fill(a.control.background), r.stroke({ width: 1, color: a.control.border }), P != null && P.indeterminate ? (r.moveTo(4, 4), r.lineTo(Math.max(4, i - 4), Math.max(4, o - 4)), r.stroke({ width: 2, color: a.control.accent }), r.moveTo(Math.max(4, i - 4), 4), r.lineTo(4, Math.max(4, o - 4)), r.stroke({ width: 2, color: a.control.accent })) : P != null && P.checked && (r.rect(3, 3, Math.max(0, i - 3 * 2), Math.max(0, o - 3 * 2)), r.fill(a.control.accent));
        else if (y === "radio") {
            {
                var Z = Math.max(0, Math.min(i, o) / 2 - .5);
                r.circle(i / 2, o / 2, Z), r.fill(a.control.background), r.stroke({ width: 1, color: a.control.border });
            }
            if (P != null && P.checked) {
                var T = Math.max(0, Math.min(i, o) / 2 - 4.5);
                r.circle(i / 2, o / 2, T), r.fill(a.control.accent);
            }
        }
        else {
            var T = G != null ? 2 : 1, L = T / 2;
            a.control.radius > 0 ? r.roundRect(L, L, Math.max(0, i - T), Math.max(0, o - T), a.control.radius) : r.rect(L, L, Math.max(0, i - T), Math.max(0, o - T)), r.fill(a.control.background), r.stroke({ width: T, color: G != null ? G : a.control.border });
            var Z = y === "password" ? "\u2022".repeat(((U = P == null ? void 0 : P.value) != null ? U : "").length) : (rt = P == null ? void 0 : P.value) != null ? rt : "", z = Math.max(0, i - k * 2);
            b && f.fieldBounds.set(b, { x: s, y: l, w: i, h: o, innerLeft: k, innerTop: E, innerWidth: z, maxLines: _, isPassword: y === "password" });
            var dt = le(Z, z, d), A = ce(dt, _), p = A.length > 0 ? A[A.length - 1].end : 0;
            if (b && P && typeof P.value == "string") {
                var S = P.selections;
                if (S && S.size > 0)
                    try {
                        for (var _f = __values(S.entries()), _g = _f.next(); !_g.done; _g = _f.next()) {
                            var _h = __read(_g.value, 2), v = _h[0], N = _h[1];
                            var O = g((ot = N.start) != null ? ot : 0, 0, Z.length), $ = g((q = N.end) != null ? q : O, 0, Z.length), J = g(Math.min(O, $), 0, p), Y = g(Math.max(O, $), 0, p);
                            if (J === Y)
                                continue;
                            var H = Ot(n, "__sel:".concat(v));
                            St(H), H.zIndex = 0, H.visible = !0;
                            for (var Q = 0; Q < A.length; Q++) {
                                var nt = A[Q], ht = Math.max(J, nt.start), wt = Math.min(Y, nt.end);
                                if (ht >= wt)
                                    continue;
                                var Mt = k + d(Z.slice(nt.start, ht)), Rt = d(Z.slice(ht, wt));
                                H.rect(Mt, E + Q * B, Rt, B);
                            }
                            H.fill({ color: w(v), alpha: .22 });
                        }
                    }
                    catch (e_7_1) { e_7 = { error: e_7_1 }; }
                    finally {
                        try {
                            if (_g && !_g.done && (_b = _f.return)) _b.call(_f);
                        }
                        finally { if (e_7) throw e_7.error; }
                    }
                if (I && W != null) {
                    var v = (X = P.selections) == null ? void 0 : X.get(W), N = v ? v.end : 0, O = g(N, 0, p), $ = Math.max(0, A.length - 1);
                    for (var Q = 0; Q < A.length; Q++) {
                        var nt = A[Q];
                        if (O >= nt.start && O <= nt.end) {
                            $ = Q;
                            break;
                        }
                    }
                    var J = (tt = A[$]) != null ? tt : { start: 0, end: 0, text: "" }, Y = k + d(Z.slice(J.start, O)), H = Ot(n, "__caret");
                    St(H), H.zIndex = 2, H.visible = !0, H.moveTo(Y, E + $ * B), H.lineTo(Y, E + $ * B + B), H.stroke({ width: 1, color: G != null ? G : a.control.focusBorder });
                }
            }
            var R = A.map(function (S) { return S.text; }).join("\n"), D = At(n, "__valueText", function (S) { S.style = { fontFamily: a.fontFamily, fontSize: a.fontSize, fill: a.text, fontWeight: "400", wordWrap: !1 }, S.zIndex = 1; });
            D.text = R, D.position.set(k, E);
        }
        b && (Ut(n), n.eventMode = "static", n.cursor = "text", n.on("pointerdown", function (T) {
            var e_8, _a, e_9, _b, e_10, _c;
            var Z, z, dt, A, p, R, D, S, v, N, O, $, J;
            if ((T == null ? void 0 : T.button) === 2)
                return;
            var L = t.getPointerId ? t.getPointerId(T) : Number((dt = (z = T == null ? void 0 : T.pointerId) != null ? z : (Z = T == null ? void 0 : T.data) == null ? void 0 : Z.pointerId) != null ? dt : 0);
            if (!(L <= 0)) {
                if (f.focusedKeyByPointer.set(L, b), f.keyboardOwnerPointerId = L, y === "checkbox") {
                    var Y = h(b, e.attrs), H = Y.indeterminate === !0, Q = Y.checked === !0;
                    !Q && !H ? (Y.checked = !0, Y.indeterminate = !1) : Q && !H ? (Y.checked = !1, Y.indeterminate = !0) : (Y.checked = !1, Y.indeterminate = !1);
                }
                else if (y === "radio") {
                    var H = "radio:".concat((p = (A = e.attrs) == null ? void 0 : A.name) != null ? p : "__default__"), Q = (R = x.get(H)) != null ? R : [];
                    try {
                        for (var Q_1 = __values(Q), Q_1_1 = Q_1.next(); !Q_1_1.done; Q_1_1 = Q_1.next()) {
                            var nt = Q_1_1.value;
                            var ht = h(nt, void 0);
                            ht.checked = nt === b;
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
                    var Y = h(b, e.attrs);
                    if (typeof Y.value == "string") {
                        try {
                            for (var _d = __values(f.inputs.entries()), _f = _d.next(); !_f.done; _f = _d.next()) {
                                var _g = __read(_f.value, 2), kt = _g[0], xt = _g[1];
                                kt !== b && ((D = xt.selections) == null || D.delete(L));
                            }
                        }
                        catch (e_9_1) { e_9 = { error: e_9_1 }; }
                        finally {
                            try {
                                if (_f && !_f.done && (_b = _d.return)) _b.call(_d);
                            }
                            finally { if (e_9) throw e_9.error; }
                        }
                        var H = y === "password" ? "\u2022".repeat(Y.value.length) : Y.value, Q = f.fieldBounds.get(b), nt = (S = Q == null ? void 0 : Q.innerWidth) != null ? S : Math.max(0, i - k * 2), ht = ce(le(H, nt, d), _), wt = ((N = (v = T.global) == null ? void 0 : v.x) != null ? N : 0) - s - k, Mt = (($ = (O = T.global) == null ? void 0 : O.y) != null ? $ : 0) - l - E, Rt = Pe({ fullText: H, lines: ht, localX: wt, localY: Mt, lineHeight: B, measure: d });
                        Y.selections || (Y.selections = new Map), Y.selections.set(L, { start: Rt, end: Rt });
                        try {
                            for (var _h = __values(M.entries()), _j = _h.next(); !_j.done; _j = _h.next()) {
                                var _k = __read(_j.value, 2), kt = _k[0], xt = _k[1];
                                xt.key === b && kt !== L && M.delete(kt);
                            }
                        }
                        catch (e_10_1) { e_10 = { error: e_10_1 }; }
                        finally {
                            try {
                                if (_j && !_j.done && (_c = _h.return)) _c.call(_h);
                            }
                            finally { if (e_10) throw e_10.error; }
                        }
                        M.set(L, { key: b, anchor: Rt });
                    }
                }
                (y === "checkbox" || y === "radio") && ((J = T.stopPropagation) == null || J.call(T)), c == null || c();
            }
        }), (y === "checkbox" || y === "radio") && (n.cursor = "pointer"), n.hitArea = new bt(0, 0, Math.max(0, i), Math.max(0, o)));
    }
    function Kr(t, e) { t.setPadding(e.EDGE_TOP, 6), t.setPadding(e.EDGE_BOTTOM, 6), t.setHeight(108), t.setMinHeight(108), t.setMinWidth(220); }
    function jr(t) {
        var e_11, _a, e_12, _b;
        var q, X, tt, T, L, Z, z, dt;
        var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.absX, l = t.absY, a = t.theme, d = t.textMeasure, f = t.uiState, h = t.getOrInitInputState, g = t.clamp, x = t.textDrags, M = t.requestPaint, c = e.key, y = c ? h(c, xe(qt({}, (q = e.attrs) != null ? q : {}), { type: "text" })) : void 0, b = (X = t.showCaret) != null ? X : !1, P = (tt = t.caretPointerId) != null ? tt : null, I = t.focusColor, W = t.getCursorColor;
        n.sortableChildren = !0;
        try {
            for (var _c = __values(n.children), _d = _c.next(); !_d.done; _d = _c.next()) {
                var A = _d.value;
                var p = A.label;
                p && (p.startsWith("__sel:") || p === "__caret") && (A.visible = !1);
            }
        }
        catch (e_11_1) { e_11 = { error: e_11_1 }; }
        finally {
            try {
                if (_d && !_d.done && (_a = _c.return)) _a.call(_c);
            }
            finally { if (e_11) throw e_11.error; }
        }
        var G = 8, w = 6 + _t, k = 5, E = a.fontSize * 1.25, _ = I != null ? 2 : 1, B = _ / 2;
        a.control.radius > 0 ? r.roundRect(B, B, Math.max(0, i - _), Math.max(0, o - _), a.control.radius) : r.rect(B, B, Math.max(0, i - _), Math.max(0, o - _)), r.fill(a.control.background), r.stroke({ width: _, color: I != null ? I : a.control.border });
        var K = (T = y == null ? void 0 : y.value) != null ? T : "", V = Math.max(0, i - G * 2);
        c && f.fieldBounds.set(c, { x: s, y: l, w: i, h: o, innerLeft: G, innerTop: w, innerWidth: V, maxLines: k, isPassword: !1 });
        var m = le(K, V, d), C = ce(m, k), U = C.length > 0 ? C[C.length - 1].end : 0;
        if (c && y && typeof y.value == "string") {
            var A = y.selections;
            if (A && A.size > 0)
                try {
                    for (var _f = __values(A.entries()), _g = _f.next(); !_g.done; _g = _f.next()) {
                        var _h = __read(_g.value, 2), p = _h[0], R = _h[1];
                        var D = g((L = R.start) != null ? L : 0, 0, K.length), S = g((Z = R.end) != null ? Z : D, 0, K.length), v = g(Math.min(D, S), 0, U), N = g(Math.max(D, S), 0, U);
                        if (v === N)
                            continue;
                        var O = Ot(n, "__sel:".concat(p));
                        St(O), O.zIndex = 0, O.visible = !0;
                        for (var $ = 0; $ < C.length; $++) {
                            var J = C[$], Y = Math.max(v, J.start), H = Math.min(N, J.end);
                            if (Y >= H)
                                continue;
                            var Q = G + d(K.slice(J.start, Y)), nt = d(K.slice(Y, H));
                            O.rect(Q, w + $ * E, nt, E);
                        }
                        O.fill({ color: W(p), alpha: .22 });
                    }
                }
                catch (e_12_1) { e_12 = { error: e_12_1 }; }
                finally {
                    try {
                        if (_g && !_g.done && (_b = _f.return)) _b.call(_f);
                    }
                    finally { if (e_12) throw e_12.error; }
                }
            if (b && P != null) {
                var p = (z = y.selections) == null ? void 0 : z.get(P), R = p ? p.end : 0, D = g(R, 0, U), S = Math.max(0, C.length - 1);
                for (var $ = 0; $ < C.length; $++) {
                    var J = C[$];
                    if (D >= J.start && D <= J.end) {
                        S = $;
                        break;
                    }
                }
                var v = (dt = C[S]) != null ? dt : { start: 0, end: 0, text: "" }, N = G + d(K.slice(v.start, D)), O = Ot(n, "__caret");
                St(O), O.zIndex = 2, O.visible = !0, O.moveTo(N, w + S * E), O.lineTo(N, w + S * E + E), O.stroke({ width: 1, color: I != null ? I : a.control.focusBorder });
            }
        }
        var rt = C.map(function (A) { return A.text; }).join("\n"), ot = At(n, "__valueText", function (A) { A.style = { fontFamily: a.fontFamily, fontSize: a.fontSize, fill: a.text, fontWeight: "400", wordWrap: !1 }, A.zIndex = 1; });
        ot.text = rt, ot.position.set(G, w), c && (Ut(n), n.eventMode = "static", n.cursor = "text", n.hitArea = new bt(0, 0, Math.max(0, i), Math.max(0, o)), n.on("pointerdown", function (A) {
            var e_13, _a, e_14, _b;
            var D, S, v, N, O, $, J, Y, H, Q;
            if ((A == null ? void 0 : A.button) === 2)
                return;
            var p = t.getPointerId ? t.getPointerId(A) : Number((v = (S = A == null ? void 0 : A.pointerId) != null ? S : (D = A == null ? void 0 : A.data) == null ? void 0 : D.pointerId) != null ? v : 0);
            if (p <= 0)
                return;
            f.focusedKeyByPointer.set(p, c), f.keyboardOwnerPointerId = p;
            var R = h(c, xe(qt({}, (N = e.attrs) != null ? N : {}), { type: "text" }));
            if (typeof R.value == "string") {
                try {
                    for (var _c = __values(f.inputs.entries()), _d = _c.next(); !_d.done; _d = _c.next()) {
                        var _f = __read(_d.value, 2), Ct = _f[0], Nt = _f[1];
                        Ct !== c && ((O = Nt.selections) == null || O.delete(p));
                    }
                }
                catch (e_13_1) { e_13 = { error: e_13_1 }; }
                finally {
                    try {
                        if (_d && !_d.done && (_a = _c.return)) _a.call(_c);
                    }
                    finally { if (e_13) throw e_13.error; }
                }
                var nt = f.fieldBounds.get(c), ht = ($ = nt == null ? void 0 : nt.innerWidth) != null ? $ : Math.max(0, i - G * 2), wt = R.value, Mt = ce(le(wt, ht, d), k), Rt = ((Y = (J = A.global) == null ? void 0 : J.x) != null ? Y : 0) - s - G, kt = ((Q = (H = A.global) == null ? void 0 : H.y) != null ? Q : 0) - l - w, xt = Pe({ fullText: wt, lines: Mt, localX: Rt, localY: kt, lineHeight: E, measure: d });
                R.selections || (R.selections = new Map), R.selections.set(p, { start: xt, end: xt });
                try {
                    for (var _g = __values(x.entries()), _h = _g.next(); !_h.done; _h = _g.next()) {
                        var _j = __read(_h.value, 2), Ct = _j[0], Nt = _j[1];
                        Nt.key === c && Ct !== p && x.delete(Ct);
                    }
                }
                catch (e_14_1) { e_14 = { error: e_14_1 }; }
                finally {
                    try {
                        if (_h && !_h.done && (_b = _g.return)) _b.call(_g);
                    }
                    finally { if (e_14) throw e_14.error; }
                }
                x.set(p, { key: c, anchor: xt });
            }
            M == null || M();
        }));
    }
    function Vr(t, e) { t.setFlexDirection(e.FLEX_DIRECTION_ROW), t.setAlignItems(e.ALIGN_CENTER), t.setJustifyContent(e.JUSTIFY_FLEX_START), t.setPadding(e.EDGE_LEFT, 8), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0); }
    function uo(t, e, n, r, i) { t.circle(e, n, r), t.stroke({ width: 2, color: i }); var o = e + r * .65, s = n + r * .65, l = e + r * 1.55, a = n + r * 1.55; t.moveTo(o, s), t.lineTo(l, a), t.stroke({ width: 2, color: i }); }
    function Jr(t, e) { t.setFlexDirection(e.FLEX_DIRECTION_ROW), t.setFlexWrap(e.WRAP_NO_WRAP), t.setAlignItems(e.ALIGN_CENTER), t.setJustifyContent(e.JUSTIFY_FLEX_START), t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0); }
    function Zr(t, e) { t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0), t.setWidth(36), t.setHeight(36), t.setMinWidth(36), t.setMinHeight(36), t.setFlexGrow(0), t.setFlexShrink(0), t.setMargin(e.EDGE_RIGHT, 6); }
    function Qr(t) { var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.theme, l = t.uiState, a = t.getPointerId, d = t.focusInputKey, f = t.requestPaint, h = function (x) { r.clear(); var M = 1, c = M / 2; s.control.button.radius > 0 ? r.roundRect(c, c, Math.max(0, i - M), Math.max(0, o - M), s.control.button.radius) : r.rect(c, c, Math.max(0, i - M), Math.max(0, o - M)), r.fill(x), r.stroke({ width: M, color: s.control.button.border }); var y = i / 2 - 2, b = o / 2 - 2, P = Math.max(5, Math.min(7, Math.min(i, o) * .22)); uo(r, y, b, P, s.text); }; h(s.control.button.fill), Ut(n), n.eventMode = "static", n.cursor = "pointer", n.hitArea = new bt(0, 0, Math.max(0, i), Math.max(0, o)), n.on("pointerover", function () { return h(s.control.button.hoverFill); }), n.on("pointerout", function () { return h(s.control.button.fill); }), n.on("pointerdown", function (x) { var M; if ((x == null ? void 0 : x.button) !== 2) {
        if (h(s.control.button.activeFill), d) {
            var c = a(x);
            c > 0 && (l.focusedKeyByPointer.set(c, d), l.keyboardOwnerPointerId = c);
        }
        f == null || f(), (M = x.stopPropagation) == null || M.call(x);
    } }), n.on("pointerup", function () { return h(s.control.button.hoverFill); }); var g = e.attrs; }
    function nn(t, e) { var n = t.get(e); if (n)
        return n; var r = { x: 0, y: 0 }; return t.set(e, r), r; }
    function qr(t, e) { t.setPositionType(e.POSITION_TYPE_ABSOLUTE), t.setPosition(e.EDGE_LEFT, 0), t.setPosition(e.EDGE_TOP, 0), t.setAlignSelf(e.ALIGN_FLEX_START), t.setFlexGrow(0), t.setFlexShrink(0), t.setPadding(e.EDGE_LEFT, 12), t.setPadding(e.EDGE_RIGHT, 12), t.setPadding(e.EDGE_TOP, 12), t.setPadding(e.EDGE_BOTTOM, 12), t.setWidth(540), t.setMinWidth(360), t.setMinHeight(148); }
    function ti(t) { var W, G; var e = t.node, n = t.container, r = t.w, i = t.h, o = t.theme, s = t.selectedBy, l = t.getCursorColor, a = t.dialogStates, d = t.dialogDrags, f = t.bringToFront, h = t.requestPaint, g = e.key; if (!g)
        return; var x = s.get(g), M = x == null ? o.boxBorder : l(x), c = Math.max(0, Math.round(r)), y = Math.max(0, Math.round(i)), b = Ot(n, "__dialogBorder"); St(b), b.rect(0, 0, c, y), b.fill({ color: 16777215, alpha: .8 }); var P = x == null ? 1 : 2, I = P / 2; b.rect(I, I, Math.max(0, c - P), Math.max(0, y - P)), b.stroke({ width: P, color: M, alignment: 0 }), b.eventMode = "static", b.cursor = "move", b.hitArea = new bt(0, 0, c, y), b.on("pointerdown", function (w) {
        var e_15, _a;
        var B, K, V, m, C, U, rt, ot;
        var k = function (q) { try {
            typeof console != "undefined" && typeof console.log == "function" && console.log("[dialog pointerdown] ".concat(q));
        }
        catch (X) { } };
        if (k("start"), (w == null ? void 0 : w.button) === 2)
            return;
        k("pointer-id");
        var E = t.getPointerId ? t.getPointerId(w) : Number((V = (K = w == null ? void 0 : w.pointerId) != null ? K : (B = w == null ? void 0 : w.data) == null ? void 0 : B.pointerId) != null ? V : 0);
        if (E <= 0 || E <= 0)
            return;
        k("clear-other-drags");
        try {
            for (var _b = __values(d.entries()), _c = _b.next(); !_c.done; _c = _b.next()) {
                var _d = __read(_c.value, 2), q = _d[0], X = _d[1];
                X.key === g && q !== E && d.delete(q);
            }
        }
        catch (e_15_1) { e_15 = { error: e_15_1 }; }
        finally {
            try {
                if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
            }
            finally { if (e_15) throw e_15.error; }
        }
        k("select"), s.set(g, E), k("bring-to-front"), f == null || f(g), k("state");
        var _ = nn(a, g);
        k("set-drag"), d.set(E, { key: g, startGX: (C = (m = w.global) == null ? void 0 : m.x) != null ? C : 0, startGY: (rt = (U = w.global) == null ? void 0 : U.y) != null ? rt : 0, originX: _.x, originY: _.y }), k("request-paint"), h == null || h(), k("stop-propagation"), (ot = w.stopPropagation) == null || ot.call(w), k("done");
    }); {
        var w = n.getChildByLabel, k = (G = (W = w == null ? void 0 : w.call(n, "__children")) != null ? W : n.children.find(function (E) { return E && E.label === "__children"; })) != null ? G : null;
        if (k && b.parent === n) {
            var E = n.getChildIndex(k), _ = Math.max(0, n.children.length - 1), B = Math.max(0, Math.min(E - 1, _));
            n.getChildIndex(b) > B && n.setChildIndex(b, B);
        }
    } }
    function Nn(t, e, n) { var l; var r = t.get(e); if (r)
        return r; var i = Number((l = n == null ? void 0 : n.value) != null ? l : "0"), s = { value: Number.isFinite(i) ? i : 0 }; return t.set(e, s), s; }
    function ei(t, e) { t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0), t.setHeight(36), t.setMinHeight(36), t.setMinWidth(140), t.setFlexGrow(0), t.setFlexShrink(0); }
    function ho(t, e, n) { return Math.max(e, Math.min(n, t)); }
    function Dn(t, e, n) { var i; var r = Number((i = t == null ? void 0 : t[e]) != null ? i : ""); return Number.isFinite(r) ? r : n; }
    function mo(t, e, n, r, i, o) { var l = e + 3, a = e + r - 3, d = n + 3, f = n + i - 3; t.moveTo(l, f), t.lineTo((l + a) / 2, d), t.lineTo(a, f), t.stroke({ width: 2, color: o }); }
    function fo(t, e, n, r, i, o) { var l = e + 3, a = e + r - 3, d = n + 3, f = n + i - 3; t.moveTo(l, d), t.lineTo((l + a) / 2, f), t.lineTo(a, d), t.stroke({ width: 2, color: o }); }
    function ni(t) { var V; var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.theme, l = t.getValue, a = t.setValue, d = t.requestPaint, f = e.key, h = e.attrs, g = Dn(h, "min", 0), x = Dn(h, "max", 255), M = Math.max(1e-9, Dn(h, "step", 1)), c = l(), y = 1, b = y / 2; r.rect(b, b, Math.max(0, i - y), Math.max(0, o - y)), r.fill(s.control.background), r.stroke({ width: y, color: s.control.border }); var P = 22, I = Math.max(0, i - P); r.moveTo(I + .5, 0), r.lineTo(I + .5, o), r.stroke({ width: 1, color: s.control.border, alignment: 0 }); var W = Ot(n, "__arrows"); St(W), mo(W, I, 0, P, o / 2, s.text), fo(W, I, o / 2, P, o / 2, s.text); var G = ((V = h == null ? void 0 : h.channel) != null ? V : "").toLowerCase(), w = G === "r" ? "R" : G === "g" ? "G" : G === "b" ? "B" : G === "a" ? "A" : "", k = At(n, "__valueText", function (m) { m.style = { fontFamily: s.fontFamily, fontSize: s.fontSize, fill: s.text, fontWeight: "400", wordWrap: !1 }; }); if (k.text = w ? "".concat(w, ": ").concat(Math.round(c)) : String(Math.round(c)), k.position.set(8, 9 + _t), !f)
        return; var E = new bt(I, 0, P, o / 2), _ = new bt(I, o / 2, P, o / 2), B = function (m) { var C = l(), U = ho(C + m * M, g, x); a(U), d == null || d(); }, K = Ot(n, "__hit"); St(K), K.eventMode = "static", K.cursor = "default", K.hitArea = new bt(0, 0, Math.max(0, i), Math.max(0, o)), K.on("pointerdown", function (m) {
        var e_16, _a;
        var tt, T, L, Z, z, dt;
        if ((m == null ? void 0 : m.button) === 2)
            return;
        var C = t.getPointerId ? t.getPointerId(m) : Number((L = (T = m == null ? void 0 : m.pointerId) != null ? T : (tt = m == null ? void 0 : m.data) == null ? void 0 : tt.pointerId) != null ? L : 0);
        if (C <= 0)
            return;
        var U = n.toLocal(m.global), rt = (Z = U == null ? void 0 : U.x) != null ? Z : 0, ot = (z = U == null ? void 0 : U.y) != null ? z : 0, q = E.contains(rt, ot) ? 1 : _.contains(rt, ot) ? -1 : null;
        if (!q)
            return;
        B(q);
        var X = t.numberHolds;
        if (X && f) {
            try {
                for (var _b = __values(X.entries()), _c = _b.next(); !_c.done; _c = _b.next()) {
                    var _d = __read(_c.value, 2), R = _d[0], D = _d[1];
                    R !== C && (D.timeoutId != null && window.clearTimeout(D.timeoutId), D.intervalId != null && window.clearInterval(D.intervalId), X.delete(R));
                }
            }
            catch (e_16_1) { e_16 = { error: e_16_1 }; }
            finally {
                try {
                    if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
                }
                finally { if (e_16) throw e_16.error; }
            }
            var A = X.get(C);
            A && (A.timeoutId != null && window.clearTimeout(A.timeoutId), A.intervalId != null && window.clearInterval(A.intervalId));
            var p_1 = { key: f, timeoutId: null, intervalId: null };
            p_1.timeoutId = window.setTimeout(function () { p_1.timeoutId = null, p_1.intervalId = window.setInterval(function () { B(q); }, 250); }, 500), X.set(C, p_1);
        }
        (dt = m.stopPropagation) == null || dt.call(m);
    }); }
    var rn = null;
    function ri() { return rn || (rn = new qe({ data: ee, label: "attribute-color-picker-colors", shrinkToFit: !1, usage: Cn.VERTEX | Cn.COPY_DST }), rn); }
    function ii(t, e, n) { var d, f, h, g; t.setPadding(n.EDGE_LEFT, 0), t.setPadding(n.EDGE_RIGHT, 0), t.setPadding(n.EDGE_TOP, 0), t.setPadding(n.EDGE_BOTTOM, 0); var r = Number((f = (d = e.attrs) == null ? void 0 : d.width) != null ? f : "0"), i = Number((g = (h = e.attrs) == null ? void 0 : h.height) != null ? g : "0"), o = Number.isFinite(r) && r > 0, s = Number.isFinite(i) && i > 0, l = o ? r : 240, a = s ? i : 200; (o || s) && (t.setAlignSelf(n.ALIGN_FLEX_START), t.setFlexGrow(0), t.setFlexShrink(0)), t.setWidth(l), t.setHeight(a), t.setMinWidth(Math.min(240, l)), t.setMinHeight(Math.min(200, a)); }
    function oe(t) { return Number.isFinite(t) ? Math.max(0, Math.min(255, Math.round(t))) : 0; }
    function on(t) { return oe(t).toString(16).padStart(2, "0"); }
    function po(t, e, n, r, i, o, s, l) { var a = s - n, d = l - r, f = i - n, h = o - r, g = t - n, x = e - r, M = a * a + d * d, c = a * f + d * h, y = a * g + d * x, b = f * f + h * h, P = f * g + h * x, I = 1 / (M * b - c * c), W = (b * y - c * P) * I, G = (M * P - c * y) * I; return W >= 0 && G >= 0 && W + G <= 1; }
    function go(t, e, n, r, i, o, s, l) { var a = i - n, d = o - r, f = s - n, h = l - r, g = t - n, x = e - r, M = a * h - f * d; if (Math.abs(M) < 1e-9)
        return { w0: 1, w1: 0, w2: 0 }; var c = (g * h - f * x) / M, y = (a * x - g * d) / M; return { w0: 1 - c - y, w1: c, w2: y }; }
    var bo = { name: "solid-out", fragment: { main: "\n      outColor = vec4(1.0);\n    " } }, sn = null;
    function xo() { if (sn)
        return sn; var t = { name: "color-picker-vertex-color", bits: [ir, or, rr, bo] }; return sn = new Be({ glProgram: t, resources: {} }), sn; }
    function oi(t, e, n) { var r = new Float32Array(12), i = [-90, -30, 30, 90, 150, 210]; for (var o = 0; o < 6; o++) {
        var s = i[o] * Math.PI / 180;
        r[o * 2 + 0] = t + Math.cos(s) * n, r[o * 2 + 1] = e + Math.sin(s) * n;
    } return r; }
    var ee = new Uint8Array([255, 0, 0, 255, 128, 128, 0, 255, 0, 255, 0, 255, 0, 128, 128, 255, 0, 0, 255, 255, 128, 0, 128, 255]), Se = new Uint32Array([0, 1, 2, 0, 2, 3, 0, 3, 4, 0, 4, 5]);
    function Ln(t) { var e = t.lx, n = t.ly, r = t.w, i = t.h, o = 10, s = Math.max(0, r - o * 2), l = Math.max(0, i - o * 2), a = o + s / 2, d = o + l / 2, f = Math.max(0, Math.min(s, l) / 2 - 2), h = oi(a, d, f); for (var g = 0; g < Se.length; g += 3) {
        var x = Se[g + 0], M = Se[g + 1], c = Se[g + 2], y = h[x * 2 + 0], b = h[x * 2 + 1], P = h[M * 2 + 0], I = h[M * 2 + 1], W = h[c * 2 + 0], G = h[c * 2 + 1];
        if (!po(e, n, y, b, P, I, W, G))
            continue;
        var w = go(e, n, y, b, P, I, W, G), k = x * 4, E = M * 4, _ = c * 4, B = w.w0 * ee[k + 0] + w.w1 * ee[E + 0] + w.w2 * ee[_ + 0], K = w.w0 * ee[k + 1] + w.w1 * ee[E + 1] + w.w2 * ee[_ + 1], V = w.w0 * ee[k + 2] + w.w1 * ee[E + 2] + w.w2 * ee[_ + 2];
        return { r: oe(B), g: oe(K), b: oe(V) };
    } return null; }
    function si(t) { var X, tt; var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.theme, l = t.rgb, a = t.setRgb, d = t.alpha, f = t.setAlpha, h = t.pick, g = t.setPick, x = t.requestPaint, M = t.getPointerId, c = t.setDraggingPointerId, y = 1, b = y / 2; r.rect(b, b, Math.max(0, i - y), Math.max(0, o - y)), r.fill(16777215), r.stroke({ width: y, color: s.control.border, alignment: 0 }); var P = 10, I = Math.max(0, i - P * 2), W = Math.max(0, o - P * 2), G = P + I / 2, w = P + W / 2, k = Math.max(0, Math.min(I, W) / 2 - 2), E = oi(G, w, k), _ = "".concat(Math.round(i), "x").concat(Math.round(o)), B = n.getChildByLabel, K = B ? B.call(n, "__mesh") : n.children.find(function (T) { return (T == null ? void 0 : T.label) === "__mesh"; }); if (K) {
        if (K.__sizeKey !== _) {
            var T = new Float32Array(E.length), L = new Me({ positions: E, uvs: T, indices: Se });
            L.addAttribute("aColor", { buffer: ri(), format: "unorm8x4", stride: 4, offset: 0 });
            try {
                (tt = (X = K.geometry) == null ? void 0 : X.destroy) == null || tt.call(X);
            }
            catch (Z) { }
            K.geometry = L, K.__sizeKey = _;
        }
    }
    else {
        var T = new Float32Array(E.length), L = new Me({ positions: E, uvs: T, indices: Se });
        L.addAttribute("aColor", { buffer: ri(), format: "unorm8x4", stride: 4, offset: 0 }), K = new Qe({ geometry: L, shader: xo() }), K.label = "__mesh", n.addChild(K), K.__sizeKey = _;
    } K.removeAllListeners(), K.eventMode = "static", K.cursor = "crosshair", K.hitArea = new bt(P, P, I, W), K.on("pointerdown", function (T) { var p, R, D; if ((T == null ? void 0 : T.button) === 2)
        return; var L = M(T); if (L <= 0)
        return; var Z = n.toLocal(T.global), z = (p = Z == null ? void 0 : Z.x) != null ? p : 0, dt = (R = Z == null ? void 0 : Z.y) != null ? R : 0, A = Ln({ lx: z, ly: dt, w: i, h: o }); A && (g({ x: z, y: dt }), a(A), c(L), x == null || x(), (D = T.stopPropagation) == null || D.call(T)); }); {
        var T = Ot(n, "__border");
        St(T), T.moveTo(E[0], E[1]);
        for (var L = 1; L < 6; L++)
            T.lineTo(E[L * 2 + 0], E[L * 2 + 1]);
        T.closePath(), T.stroke({ width: 2, color: 0 });
    } var V = Ot(n, "__overlay"); St(V); var m = 44, C = 18, U = Math.max(P, i - P - m), rt = P; V.rect(U, rt, m, C), V.fill({ color: oe(l.r) << 16 | oe(l.g) << 8 | oe(l.b), alpha: Math.max(0, Math.min(1, oe(d) / 255)) }), V.rect(U + .5, rt + .5, m - 1, C - 1), V.stroke({ width: 1, color: s.control.border, alignment: 0 }), h && (V.circle(h.x, h.y, 4), V.stroke({ width: 2, color: 16777215 }), V.circle(h.x, h.y, 4), V.stroke({ width: 1, color: 0 })); var ot = "#".concat(on(l.r)).concat(on(l.g)).concat(on(l.b)).concat(on(d)).toUpperCase(), q = At(n, "__label", function (T) { T.style = { fontFamily: s.fontFamily, fontSize: Math.max(10, Math.floor(s.fontSize * .75)), fill: s.mutedText, fontWeight: "400", wordWrap: !1 }; }); q.text = ot, q.position.set(P, Math.max(P, o - P - q.height)), f && f(oe(d)); }
    function ue(t, e, n) { var r = t.get(e); if (r)
        return r; var i = { selectedIndex: Math.max(0, n | 0), open: !1 }; return t.set(e, i), i; }
    function ai(t, e) { t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0), t.setHeight(36), t.setMinHeight(36), t.setMinWidth(220); }
    function yo(t, e, n, r, i, o) { var l = e + 4, a = e + r - 4, d = n + 4, f = n + i - 4; t.moveTo(l, (d + f) / 2 - 2), t.lineTo((l + a) / 2, (d + f) / 2 + 2), t.lineTo(a, (d + f) / 2 - 2), t.stroke({ width: 2, color: o }); }
    function _o(t) {
        var r;
        var n = String((r = t == null ? void 0 : t["data-options"]) != null ? r : "").split("\n").map(function (i) { return i.trim(); }).filter(function (i) { return i.length > 0; });
        return n.length > 0 ? n : ["(empty)"];
    }
    function wo(t) { var n; var e = Number((n = t == null ? void 0 : t["data-selected-index"]) != null ? n : "0"); return Number.isFinite(e) ? Math.max(0, e | 0) : 0; }
    function an(t) { var K; var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.absX, l = t.absY, a = t.theme, d = t.selectStates, f = t.uiState, h = t.getPointerId, g = t.getCursorColor, x = t.requestPaint, M = t.popupSink, c = e.key; if (!c)
        return; var y = _o(e.attrs), b = wo(e.attrs), P = ue(d, c, b); P.selectedIndex = Math.max(0, Math.min(y.length - 1, P.selectedIndex | 0)); var I = (function () {
        var e_17, _a;
        var V = f.keyboardOwnerPointerId;
        if (f.focusedKeyByPointer.get(V) === c)
            return V;
        try {
            for (var _b = __values(f.focusedKeyByPointer.entries()), _c = _b.next(); !_c.done; _c = _b.next()) {
                var _d = __read(_c.value, 2), m = _d[0], C = _d[1];
                if (C === c)
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
    })(), W = I != null ? g(I) : null, G = W != null ? 2 : 1, w = G / 2; a.control.radius > 0 ? r.roundRect(w, w, Math.max(0, i - G), Math.max(0, o - G), a.control.radius) : r.rect(w, w, Math.max(0, i - G), Math.max(0, o - G)), r.fill(a.control.background), r.stroke({ width: G, color: W != null ? W : a.control.border }); var k = 22, E = Math.max(0, i - k); r.moveTo(E + .5, 0), r.lineTo(E + .5, o), r.stroke({ width: 1, color: a.control.border, alignment: 0 }), yo(r, E, 0, k, o, a.text); var _ = (K = y[P.selectedIndex]) != null ? K : "", B = At(n, "__label", function (V) { V.style = { fontFamily: a.fontFamily, fontSize: a.fontSize, fill: a.text, fontWeight: "400", wordWrap: !1 }; }); B.text = _, B.position.set(8, 9 + _t), Ut(n), n.eventMode = "static", n.cursor = "pointer", n.hitArea = new bt(0, 0, Math.max(0, i), Math.max(0, o)), n.on("pointerdown", function (V) { var C; if ((V == null ? void 0 : V.button) === 2)
        return; var m = h(V); m <= 0 || (f.focusedKeyByPointer.set(m, c), f.keyboardOwnerPointerId = m, P.open = !P.open, x == null || x(), (C = V.stopPropagation) == null || C.call(V)); }), P.open && M.push({ key: c, absX: s, absY: l, w: i, h: o, options: y, selectedIndex: P.selectedIndex }); }
    function li(t) { var P; var e = t.popup, n = t.stage, r = t.theme, i = t.selectStates, o = t.uiState, s = t.getPointerId, l = t.requestPaint, a = t.viewportW, d = t.viewportH, f = 30, g = Math.min(7, e.options.length), x = g * f, M = e.absX, c = e.absY + e.h; M = Math.max(0, Math.min(M, Math.max(0, a - e.w))), c + x > d - 4 && (c = e.absY - x), c = Math.max(0, Math.min(c, Math.max(0, d - x))); var y = new Tt; y.position.set(M, c), n.addChild(y); var b = new yt; b.rect(0, 0, e.w, x), b.fill(16777215), b.rect(.5, .5, Math.max(0, e.w - 1), Math.max(0, x - 1)), b.stroke({ width: 1, color: r.control.border, alignment: 0 }), y.addChild(b), y.eventMode = "static", y.cursor = "pointer", y.hitArea = new bt(0, 0, e.w, x), y.on("pointerdown", function (I) { var B, K, V; if ((I == null ? void 0 : I.button) === 2)
        return; var W = s(I), G = y.toLocal(I.global), w = (B = G == null ? void 0 : G.x) != null ? B : -1, k = (K = G == null ? void 0 : G.y) != null ? K : -1; if (w < 0 || w > e.w || k < 0 || k > x)
        return; var E = Math.max(0, Math.min(e.options.length - 1, Math.floor(k / f))), _ = i.get(e.key); _ && (_.selectedIndex = E, _.open = !1), W > 0 && (o.focusedKeyByPointer.set(W, e.key), o.keyboardOwnerPointerId = W), l == null || l(), (V = I.stopPropagation) == null || V.call(I); }); for (var I = 0; I < g; I++) {
        var W = I * f;
        if (I === e.selectedIndex) {
            var w = new yt;
            w.rect(1, W + 1, Math.max(0, e.w - 2), f - 2), w.fill({ color: 0, alpha: .06 }), y.addChild(w);
        }
        var G = Kt({ text: (P = e.options[I]) != null ? P : "", fontFamily: r.fontFamily, fontSize: r.fontSize, fill: r.text, wordWrap: !1 });
        G.position.set(8, W + 7 + _t), y.addChild(G);
    } }
    function Dt(t, e, n) { var r = Number.isFinite(t) ? t | 0 : 0; return Math.max(e, Math.min(n, r)); }
    function zt(t) { var e = Dt(t, 0, 99); return e < 10 ? "0".concat(e) : String(e); }
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
    function Mo(t) { var e = String(t != null ? t : "").trim().split("-"); if (e.length !== 3)
        return null; var n = un(e[0]), r = Zt(e[1], 1, 12), i = Zt(e[2], 1, 31); if (n == null || r == null || i == null)
        return null; var o = Dt(Math.floor((i - 1) / 7) + 1, 1, 4); return { year2: n, month: r, weekIndex: o }; }
    function Io(t) { var e = String(t != null ? t : "").trim(), n = e.indexOf("-W"); if (n < 0)
        return null; var r = un(e.slice(0, n)), i = Zt(e.slice(n + 2), 1, 48); if (r == null || i == null)
        return null; var o = Dt(Math.floor((i - 1) / 4) + 1, 1, 12), s = Dt((i - 1) % 4 + 1, 1, 4); return { year2: r, month: o, weekIndex: s }; }
    function Po(t) { var e = String(t != null ? t : "").trim(), n = e.indexOf("T"); if (n < 0 && (n = e.indexOf(" ")), n < 0)
        return null; var r = e.slice(0, n).split("-"), i = e.slice(n + 1).split(":"); if (r.length !== 3 || i.length !== 2 && i.length !== 3)
        return null; var o = un(r[0]), s = Zt(r[1], 1, 12), l = Zt(r[2], 1, 31), a = Zt(i[0], 0, 23), d = Zt(i[1], 0, 59), f = i.length === 3 ? Zt(i[2], 0, 59) : 0; if (o == null || s == null || l == null || a == null || d == null || f == null)
        return null; var h = Dt(Math.floor((l - 1) / 7) + 1, 1, 4); return { year2: o, month: s, weekIndex: h, hour: a, minute: d, second: f }; }
    function ln(t) { return "20".concat(zt(t.year2), "-").concat(zt(t.month)); }
    function So(t) { return (Dt(t.month, 1, 12) - 1) * 4 + Dt(t.weekIndex, 1, 4); }
    function cn(t) { return "20".concat(zt(t.year2), "-W").concat(zt(So(t))); }
    function ke(t) { var e = (Dt(t.weekIndex, 1, 4) - 1) * 7 + 1; return "20".concat(zt(t.year2), "-").concat(zt(t.month), "-").concat(zt(e)); }
    function Xe(t) { return "".concat(zt(t.hour), ":").concat(zt(t.minute), ":").concat(zt(t.second)); }
    function Fe(t) { return "".concat(ke(t), "T").concat(Xe(t)); }
    function ko(t) { var f; var e = t.map, n = t.yearSliderOwners, r = t.inputKey, i = t.kind, o = t.attrs, s = e.get(r); if (s)
        return s.kind = i, s; var l = new Date, a = { kind: i, year2: Dt(l.getFullYear() - 2e3, 0, 99), month: Dt(l.getMonth() + 1, 1, 12), weekIndex: 1, hour: Dt(l.getHours(), 0, 23), minute: Dt(l.getMinutes(), 0, 59), second: Dt(l.getSeconds(), 0, 59), openPanel: null, openYear: !1, openMonthGrid: !1, yearSliderKey: "".concat(r, ":year-slider") }, d = String((f = o == null ? void 0 : o.value) != null ? f : ""); if (d.trim().length > 0) {
        if (i === "time") {
            var h = To(d);
            h && (a.hour = h.hour, a.minute = h.minute, a.second = h.second);
        }
        else if (i === "month") {
            var h = Eo(d);
            h && (a.year2 = h.year2, a.month = h.month);
        }
        else if (i === "week") {
            var h = Io(d);
            h && (a.year2 = h.year2, a.month = h.month, a.weekIndex = h.weekIndex);
        }
        else if (i === "date") {
            var h = Mo(d);
            h && (a.year2 = h.year2, a.month = h.month, a.weekIndex = h.weekIndex);
        }
        else if (i === "datetime-local") {
            var h = Po(d);
            h && (a.year2 = h.year2, a.month = h.month, a.weekIndex = h.weekIndex, a.hour = h.hour, a.minute = h.minute, a.second = h.second);
        }
    } return e.set(r, a), n.set(a.yearSliderKey, r), a; }
    function ui(t, e, n) { t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0), t.setHeight(36), t.setMinHeight(36), t.setMinWidth(n === "datetime-local" ? 340 : 220); }
    function Ro(t, e, n, r, i) { var o = i != null ? 2 : 1, s = o / 2; e.control.radius > 0 ? t.roundRect(s, s, Math.max(0, n - o), Math.max(0, r - o), e.control.radius) : t.rect(s, s, Math.max(0, n - o), Math.max(0, r - o)), t.fill(e.control.background), t.stroke({ width: o, color: i != null ? i : e.control.border }); }
    function ci(t, e, n, r, i) { var o = e + r / 2, s = n + r / 2; t.moveTo(e, s - 2), t.lineTo(o, s + 2), t.lineTo(e + r, s - 2), t.stroke({ width: 2, color: i }); }
    function di(t) { var E, _; var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.absX, l = t.absY, a = t.theme, d = t.uiState, f = t.getPointerId, h = t.getCursorColor, g = t.temporalStates, x = t.yearSliderOwners, M = t.getOrInitInputValue, c = t.requestPaint, y = t.popupSink, b = e.key; if (!b || !e.tagName)
        return; var P = e.tagName === "timeinput" ? "time" : e.tagName === "monthinput" ? "month" : e.tagName === "weekinput" ? "week" : e.tagName === "dateinput" ? "date" : "datetime-local", I = ko({ map: g, yearSliderOwners: x, inputKey: b, kind: P, attrs: e.attrs }), W = M(b, xe(qt({}, (E = e.attrs) != null ? E : {}), { type: "text" })); P === "time" ? W.value = Xe(I) : P === "month" ? W.value = ln(I) : P === "week" ? W.value = cn(I) : P === "date" ? W.value = ke(I) : W.value = Fe(I); var G = (function () {
        var e_18, _a;
        var B = d.keyboardOwnerPointerId;
        if (d.focusedKeyByPointer.get(B) === b)
            return B;
        try {
            for (var _b = __values(d.focusedKeyByPointer.entries()), _c = _b.next(); !_c.done; _c = _b.next()) {
                var _d = __read(_c.value, 2), K = _d[0], V = _d[1];
                if (V === b)
                    return K;
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
    })(), w = G != null ? h(G) : null; Ro(r, a, i, o, w); var k = 8; if (P !== "datetime-local") {
        var B = (_ = W.value) != null ? _ : "", K = At(n, "__shown", function (C) { C.style = { fontFamily: a.fontFamily, fontSize: a.fontSize, fill: a.text, fontWeight: "400", wordWrap: !1 }; });
        K.text = B, K.visible = !0, K.position.set(k, 9 + _t);
        var V = n.getChildByLabel ? n.getChildByLabel("__date") : n.children.find(function (C) { return (C == null ? void 0 : C.label) === "__date"; }), m = n.getChildByLabel ? n.getChildByLabel("__time") : n.children.find(function (C) { return (C == null ? void 0 : C.label) === "__time"; });
        V && (V.visible = !1), m && (m.visible = !1), ci(r, Math.max(0, i - 18), 11, 10, a.text);
    }
    else {
        var B = Math.max(0, Math.round(i * .52));
        r.moveTo(B + .5, 0), r.lineTo(B + .5, o), r.stroke({ width: 1, color: a.control.border, alignment: 0 });
        var K = ke(I), V = Xe(I), m = At(n, "__date", function (rt) { rt.style = { fontFamily: a.fontFamily, fontSize: a.fontSize, fill: a.text, fontWeight: "400", wordWrap: !1 }; });
        m.text = K, m.visible = !0, m.position.set(k, 9 + _t);
        var C = At(n, "__time", function (rt) { rt.style = { fontFamily: a.fontFamily, fontSize: a.fontSize, fill: a.text, fontWeight: "400", wordWrap: !1 }; });
        C.text = V, C.visible = !0, C.position.set(B + k, 9 + _t);
        var U = n.getChildByLabel ? n.getChildByLabel("__shown") : n.children.find(function (rt) { return (rt == null ? void 0 : rt.label) === "__shown"; });
        U && (U.visible = !1), ci(r, Math.max(B + 0, B + (i - B) - 18), 11, 10, a.text);
    } Ut(n), n.eventMode = "static", n.cursor = "pointer", n.hitArea = new bt(0, 0, Math.max(0, i), Math.max(0, o)), n.on("pointerdown", function (B) { var V, m, C; if ((B == null ? void 0 : B.button) === 2)
        return; var K = f(B); if (!(K <= 0)) {
        if (d.focusedKeyByPointer.set(K, b), d.keyboardOwnerPointerId = K, P !== "datetime-local")
            I.openPanel = I.openPanel ? null : P === "time" ? "time" : P === "month" ? "month" : "week", I.openYear = !1, I.openMonthGrid = !1;
        else {
            var ot = ((m = (V = B.global) == null ? void 0 : V.x) != null ? m : 0) - s <= i * .52;
            I.openPanel = ot ? I.openPanel === "week" ? null : "week" : I.openPanel === "time" ? null : "time", I.openYear = !1, I.openMonthGrid = !1;
        }
        g.set(b, I), c == null || c(), (C = B.stopPropagation) == null || C.call(B);
    } }), I.openPanel === "month" ? y.push({ kind: "month-panel", inputKey: b, absX: s, absY: l, anchorW: i, anchorH: o }) : I.openPanel === "week" ? y.push({ kind: "week-panel", inputKey: b, absX: s, absY: l, anchorW: i, anchorH: o }) : I.openPanel === "time" && y.push({ kind: "time-panel", inputKey: b, absX: s, absY: l, anchorW: i, anchorH: o }); }
    function Ue(t, e, n, r) { t.rect(0, 0, n, r), t.fill(e.control.background), t.rect(.5, .5, Math.max(0, n - 1), Math.max(0, r - 1)), t.stroke({ width: 1, color: e.control.border, alignment: 0 }); }
    function Co(t) { var e = t.stage, n = t.theme, r = t.popup, i = t.st, o = t.viewportW, s = t.viewportH, l = t.getPointerId, a = t.requestPaint, d = t.onPick, f = 4, h = 3, g = 44, x = 34, M = 8, c = M * 2 + f * g, y = M * 2 + h * x, b = r.absX, P = r.absY + r.anchorH; b = Math.max(0, Math.min(b, Math.max(0, o - c))), P + y > s - 4 && (P = r.absY - y), P = Math.max(0, Math.min(P, Math.max(0, s - y))); var I = new Tt; I.position.set(b, P), e.addChild(I); var W = new yt; Ue(W, n, c, y), I.addChild(W); for (var G = 0; G < 12; G++) {
        var w = G + 1, k = M + G % f * g, E = M + Math.floor(G / f) * x;
        if (w === i.month) {
            var B = new yt;
            B.rect(k + 1, E + 1, g - 2, x - 2), B.fill({ color: 0, alpha: .06 }), I.addChild(B);
        }
        var _ = Kt({ text: String(w), fontFamily: n.fontFamily, fontSize: n.fontSize, fill: n.text, wordWrap: !1 });
        _.position.set(k + 14, E + 8 + _t), I.addChild(_), W.rect(k, E, g, x), W.stroke({ width: 1, color: n.control.border, alignment: 0 });
    } I.eventMode = "static", I.cursor = "pointer", I.hitArea = new bt(0, 0, c, y), I.on("pointerdown", function (G) { var rt, ot, q; if ((G == null ? void 0 : G.button) === 2 || l(G) <= 0)
        return; var k = I.toLocal(G.global), E = (rt = k == null ? void 0 : k.x) != null ? rt : -1, _ = (ot = k == null ? void 0 : k.y) != null ? ot : -1, B = E - M, K = _ - M; if (B < 0 || K < 0)
        return; var V = Math.floor(B / g), m = Math.floor(K / x); if (V < 0 || V >= f || m < 0 || m >= h)
        return; var U = m * f + V + 1; U < 1 || U > 12 || (d(U), a == null || a(), (q = G.stopPropagation) == null || q.call(G)); }); }
    function Oo(t) {
        var e_19, _a;
        var e = t.stage, n = t.theme, r = t.popup, i = t.st, o = t.viewportW, s = t.viewportH, l = t.sliders, a = t.sliderBounds, d = t.sliderDrags, f = t.getPointerId, h = t.requestPaint, g = t.onChange, x = 10, M = 250, c = 78, y = r.absX, b = r.absY;
        y = r.absX + r.anchorW + 6, b = r.absY, y = Math.max(0, Math.min(y, Math.max(0, o - M))), b = Math.max(0, Math.min(b, Math.max(0, s - c)));
        var P = new Tt;
        P.position.set(y, b), e.addChild(P);
        var I = new yt;
        Ue(I, n, M, c), P.addChild(I);
        var W = Kt({ text: "20".concat(zt(i.year2)), fontFamily: n.fontFamily, fontSize: n.fontSize, fill: n.text, wordWrap: !1 });
        W.position.set(x, 8 + _t), P.addChild(W);
        var G = i.yearSliderKey, w = Math.max(0, Math.min(1, Dt(i.year2, 0, 99) / 99)), k = _e(l, G, { value: String(w) }), E = !1;
        try {
            for (var _b = __values(d.values()), _c = _b.next(); !_c.done; _c = _b.next()) {
                var V = _c.value;
                if (V.key === G) {
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
        E || (k.value = w);
        var _ = new Tt;
        _.position.set(x, 40), P.addChild(_);
        var B = new yt;
        _.addChild(B), en({ node: { key: G, attrs: { value: String(k.value) } }, container: _, graphics: B, w: M - x * 2, h: 14, absX: y + x, absY: b + 40, theme: { text: n.text, control: { progress: n.control.progress } }, sliderStates: l, sliderBounds: a, sliderDrags: d, requestPaint: h, getPointerId: f });
        var K = Dt(Math.round(k.value * 99), 0, 99);
        K !== i.year2 && g(K), P.eventMode = "static", P.hitArea = new bt(0, 0, M, c), P.on("pointerdown", function (V) { var m; (m = V.stopPropagation) == null || m.call(V); });
    }
    function Ao(t) { var e = t.panel, n = t.theme, r = t.x, i = t.y, o = t.w, s = t.st, l = t.onPick, a = 30, d = 6, f = []; for (var h = 0; h < 4; h++) {
        var g = h + 1, x = i + h * (a + d), M = new yt;
        M.rect(r, x, o, a), M.fill({ color: 0, alpha: g === s.weekIndex ? .06 : .03 }), M.rect(r + .5, x + .5, Math.max(0, o - 1), Math.max(0, a - 1)), M.stroke({ width: 1, color: n.control.border, alignment: 0 }), e.addChild(M);
        var c = (Dt(s.month, 1, 12) - 1) * 4 + g, y = Kt({ text: "".concat(g, " [").concat(zt(c), "]"), fontFamily: n.fontFamily, fontSize: n.fontSize, fill: n.text, wordWrap: !1 });
        y.position.set(r + 10, x + 7 + _t), e.addChild(y), f.push({ x: r, y: x, w: o, h: a, weekIndex: g });
    } return { hitRects: f }; }
    function hi(t) {
        var e_20, _a, e_21, _b;
        var P, I, W, G, w, k;
        var e = t.popups, n = t.stage, r = t.theme, i = t.viewportW, o = t.viewportH, s = t.temporalStates, l = t.getOrInitInputValue, a = t.sliders, d = t.sliderBounds, f = t.sliderDrags, h = t.selects, g = t.selectPopups, x = t.getCursorColor, M = t.uiFocus, c = t.getPointerId, y = t.requestPaint, b = [];
        var _loop_1 = function (E) {
            var _ = s.get(E.inputKey);
            if (_) {
                if (E.kind === "month-panel") {
                    var X = E.absX, tt = E.absY + E.anchorH;
                    X = Math.max(0, Math.min(X, Math.max(0, i - 196))), tt + 156 > o - 4 && (tt = E.absY - 156), tt = Math.max(0, Math.min(tt, Math.max(0, o - 156)));
                    var T_1 = new Tt;
                    T_1.position.set(X, tt), n.addChild(T_1);
                    var L = new yt;
                    Ue(L, r, 196, 156), T_1.addChild(L);
                    var Z_1 = { x: 10, y: 10, w: 132, h: 24 };
                    {
                        var A = new yt;
                        A.rect(Z_1.x, Z_1.y, Z_1.w, Z_1.h), A.fill({ color: 0, alpha: .03 }), A.rect(Z_1.x + .5, Z_1.y + .5, Math.max(0, Z_1.w - 1), Math.max(0, Z_1.h - 1)), A.stroke({ width: 1, color: r.control.border, alignment: 0 }), T_1.addChild(A);
                        var p = Kt({ text: "Year 20".concat(zt(_.year2)), fontFamily: r.fontFamily, fontSize: r.fontSize, fill: r.text, wordWrap: !1 });
                        p.position.set(Z_1.x + 8, Z_1.y + 4 + _t), T_1.addChild(p);
                    }
                    var z_1 = 10, dt_1 = 44;
                    for (var A = 0; A < 12; A++) {
                        var p = A + 1, R = z_1 + A % 4 * 44, D = dt_1 + Math.floor(A / 4) * 34;
                        if (p === _.month) {
                            var v = new yt;
                            v.rect(R + 1, D + 1, 42, 32), v.fill({ color: 0, alpha: .06 }), T_1.addChild(v);
                        }
                        var S = Kt({ text: String(p), fontFamily: r.fontFamily, fontSize: r.fontSize, fill: r.text, wordWrap: !1 });
                        S.position.set(R + 14, D + 8 + _t), T_1.addChild(S), L.rect(R, D, 44, 34), L.stroke({ width: 1, color: r.control.border, alignment: 0 });
                    }
                    T_1.eventMode = "static", T_1.cursor = "pointer", T_1.hitArea = new bt(0, 0, 196, 156), T_1.on("pointerdown", function (A) { var nt, ht, wt, Mt; if ((A == null ? void 0 : A.button) === 2)
                        return; var p = c(A); if (p <= 0)
                        return; M.focusedKeyByPointer.set(p, E.inputKey), M.keyboardOwnerPointerId = p; var R = T_1.toLocal(A.global), D = (nt = R == null ? void 0 : R.x) != null ? nt : -1, S = (ht = R == null ? void 0 : R.y) != null ? ht : -1; if (D >= Z_1.x && D <= Z_1.x + Z_1.w && S >= Z_1.y && S <= Z_1.y + Z_1.h) {
                        _.openYear = !0, s.set(E.inputKey, _), y == null || y(), (wt = A.stopPropagation) == null || wt.call(A);
                        return;
                    } var N = D - z_1, O = S - dt_1; if (N < 0 || O < 0)
                        return; var $ = Math.floor(N / 44), J = Math.floor(O / 34); if ($ < 0 || $ >= 4 || J < 0 || J >= 3)
                        return; var H = J * 4 + $ + 1; if (H < 1 || H > 12)
                        return; _.month = H, _.openPanel = null, _.openYear = !1, _.openMonthGrid = !1, s.set(E.inputKey, _); var Q = l(E.inputKey, { type: "text" }); Q.value = ln(_), y == null || y(), (Mt = A.stopPropagation) == null || Mt.call(A); }), T_1.on("pointerdown", function (A) { var p; (p = A.stopPropagation) == null || p.call(A); }), _.openYear && b.push({ kind: "year-panel", inputKey: E.inputKey, absX: X, absY: tt, anchorW: 196, anchorH: 0 });
                }
                if (E.kind === "week-panel") {
                    var m = E.absX, C = E.absY + E.anchorH;
                    m = Math.max(0, Math.min(m, Math.max(0, i - 280))), C + 192 > o - 4 && (C = E.absY - 192), C = Math.max(0, Math.min(C, Math.max(0, o - 192)));
                    var U_1 = new Tt;
                    U_1.position.set(m, C), n.addChild(U_1);
                    var rt = new yt;
                    Ue(rt, r, 280, 192), U_1.addChild(rt);
                    var ot_1 = { x: 10, y: 10, w: 104, h: 24 }, q_1 = { x: 10 + ot_1.w + 10, y: 10, w: 120, h: 24 }, X = function (L, Z) { var z = new yt; z.rect(L.x, L.y, L.w, L.h), z.fill({ color: 0, alpha: .03 }), z.rect(L.x + .5, L.y + .5, Math.max(0, L.w - 1), Math.max(0, L.h - 1)), z.stroke({ width: 1, color: r.control.border, alignment: 0 }), U_1.addChild(z); var dt = Kt({ text: Z, fontFamily: r.fontFamily, fontSize: r.fontSize, fill: r.text, wordWrap: !1 }); dt.position.set(L.x + 8, L.y + 4 + _t), U_1.addChild(dt); };
                    X(ot_1, "Month ".concat(_.month)), X(q_1, "Year 20".concat(zt(_.year2)));
                    var tt = 44, T_2 = Ao({ panel: U_1, theme: r, x: 10, y: tt, w: 280 - 10 * 2, st: _, onPick: function () { } }).hitRects;
                    U_1.eventMode = "static", U_1.cursor = "pointer", U_1.hitArea = new bt(0, 0, 280, 192), U_1.on("pointerdown", function (L) {
                        var e_23, _a;
                        var R, D, S, v, N;
                        if ((L == null ? void 0 : L.button) === 2)
                            return;
                        var Z = c(L);
                        if (Z <= 0)
                            return;
                        M.focusedKeyByPointer.set(Z, E.inputKey), M.keyboardOwnerPointerId = Z;
                        var z = U_1.toLocal(L.global), dt = (R = z == null ? void 0 : z.x) != null ? R : -1, A = (D = z == null ? void 0 : z.y) != null ? D : -1, p = function (O) { return dt >= O.x && dt <= O.x + O.w && A >= O.y && A <= O.y + O.h; };
                        if (p(ot_1)) {
                            _.openMonthGrid = !_.openMonthGrid, s.set(E.inputKey, _), y == null || y(), (S = L.stopPropagation) == null || S.call(L);
                            return;
                        }
                        if (p(q_1)) {
                            _.openYear = !0, s.set(E.inputKey, _), y == null || y(), (v = L.stopPropagation) == null || v.call(L);
                            return;
                        }
                        try {
                            for (var T_3 = (e_23 = void 0, __values(T_2)), T_3_1 = T_3.next(); !T_3_1.done; T_3_1 = T_3.next()) {
                                var O = T_3_1.value;
                                if (p(O)) {
                                    _.weekIndex = O.weekIndex;
                                    var $ = l(E.inputKey, { type: "text" });
                                    _.kind === "week" ? $.value = cn(_) : _.kind === "date" ? $.value = ke(_) : $.value = Fe(_), _.openPanel = null, _.openYear = !1, _.openMonthGrid = !1, s.set(E.inputKey, _), y == null || y(), (N = L.stopPropagation) == null || N.call(L);
                                    return;
                                }
                            }
                        }
                        catch (e_23_1) { e_23 = { error: e_23_1 }; }
                        finally {
                            try {
                                if (T_3_1 && !T_3_1.done && (_a = T_3.return)) _a.call(T_3);
                            }
                            finally { if (e_23) throw e_23.error; }
                        }
                    }), _.openMonthGrid && b.push({ kind: "month-grid", inputKey: E.inputKey, absX: m, absY: C + ot_1.y + ot_1.h + 4, anchorW: 0, anchorH: 0 }), _.openYear && b.push({ kind: "year-panel", inputKey: E.inputKey, absX: m + q_1.x, absY: C + q_1.y, anchorW: q_1.w, anchorH: 0 });
                }
                if (E.kind === "time-panel") {
                    var m_1 = E.absX, C_1 = E.absY + E.anchorH;
                    m_1 = Math.max(0, Math.min(m_1, Math.max(0, i - 330))), C_1 + 80 > o - 4 && (C_1 = E.absY - 80), C_1 = Math.max(0, Math.min(C_1, Math.max(0, o - 80)));
                    var U_2 = new Tt;
                    U_2.position.set(m_1, C_1), n.addChild(U_2);
                    var rt = new yt;
                    Ue(rt, r, 330, 80), U_2.addChild(rt);
                    var ot = Kt({ text: "Time", fontFamily: r.fontFamily, fontSize: r.fontSize, fill: r.text, wordWrap: !1 });
                    ot.position.set(10, 8 + _t), U_2.addChild(ot);
                    var q_2 = function (J) { return Array.from({ length: J }, function (Y, H) { return zt(H); }).join("\n"); }, X = E.inputKey, tt = "".concat(X, ":time-h"), T = "".concat(X, ":time-m"), L = "".concat(X, ":time-s"), Z = ue(h, tt, Dt(_.hour, 0, 23)), z = ue(h, T, Dt(_.minute, 0, 59)), dt = ue(h, L, Dt(_.second, 0, 59));
                    Z.selectedIndex = Dt(_.hour, 0, 23), z.selectedIndex = Dt(_.minute, 0, 59), dt.selectedIndex = Dt(_.second, 0, 59);
                    var A_1 = 96, p_2 = 36, R_1 = 32, D = 8, S = function (J, Y, H) { var Q = new Tt; Q.position.set(Y, R_1), U_2.addChild(Q); var nt = new yt; Q.addChild(nt), an({ node: { key: J, attrs: { "data-options": q_2(H), "data-selected-index": String(ue(h, J, 0).selectedIndex) } }, container: Q, graphics: nt, w: A_1, h: p_2, absX: m_1 + Y, absY: C_1 + R_1, theme: r, selectStates: h, uiState: M, getPointerId: c, getCursorColor: x, requestPaint: y, popupSink: g }); };
                    S(tt, 10, 24), S(T, 10 + A_1 + D, 60), S(L, 10 + (A_1 + D) * 2, 60);
                    var v = Dt((I = (P = h.get(tt)) == null ? void 0 : P.selectedIndex) != null ? I : _.hour, 0, 23), N = Dt((G = (W = h.get(T)) == null ? void 0 : W.selectedIndex) != null ? G : _.minute, 0, 59), O = Dt((k = (w = h.get(L)) == null ? void 0 : w.selectedIndex) != null ? k : _.second, 0, 59);
                    _.hour = v, _.minute = N, _.second = O, s.set(E.inputKey, _);
                    var $ = l(E.inputKey, { type: "text" });
                    _.kind === "time" ? $.value = Xe(_) : $.value = Fe(_), U_2.eventMode = "static", U_2.hitArea = new bt(0, 0, 330, 80), U_2.on("pointerdown", function (J) { var Y; (Y = J.stopPropagation) == null || Y.call(J); });
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
            var _ = s.get(E.inputKey);
            _ && (E.kind === "month-grid" && Co({ stage: n, theme: r, popup: E, st: _, viewportW: i, viewportH: o, getPointerId: c, requestPaint: y, onPick: function (B) { _.month = B, _.openMonthGrid = !1, s.set(E.inputKey, _); var K = l(E.inputKey, { type: "text" }); _.kind === "month" ? K.value = ln(_) : _.kind === "week" ? K.value = cn(_) : _.kind === "date" ? K.value = ke(_) : K.value = Fe(_); } }), E.kind === "year-panel" && Oo({ stage: n, theme: r, popup: E, st: _, viewportW: i, viewportH: o, sliders: a, sliderBounds: d, sliderDrags: f, getPointerId: c, requestPaint: y, onChange: function (B) { _.year2 = B, s.set(E.inputKey, _); var K = l(E.inputKey, { type: "text" }); _.kind === "month" ? K.value = ln(_) : _.kind === "week" ? K.value = cn(_) : _.kind === "date" ? K.value = ke(_) : _.kind === "time" ? K.value = Xe(_) : K.value = Fe(_); } }));
        };
        try {
            for (var b_1 = __values(b), b_1_1 = b_1.next(); !b_1_1.done; b_1_1 = b_1.next()) {
                var E = b_1_1.value;
                _loop_2(E);
            }
        }
        catch (e_21_1) { e_21 = { error: e_21_1 }; }
        finally {
            try {
                if (b_1_1 && !b_1_1.done && (_b = b_1.return)) _b.call(b_1);
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
    var fi = 5e4, Ye = new WeakMap, gi = new Map, Do = 1, bi = 0, No = 0, pi = !1, we = [], vn = null;
    function Oe(t) { return t instanceof yt ? "Graphics" : t instanceof jt ? "Text" : t instanceof Tt ? "Container" : "Object"; }
    function Lo(t) { var e = t && typeof t == "object" ? t.label : void 0, n = t && typeof t == "object" ? Oe(t) : "Object"; return e ? "".concat(n, ":").concat(String(e).slice(0, 80)) : n; }
    function de(t) { var e = Ye.get(t); return e || (e = Do++, Ye.set(t, e)), gi.set(e, t), e; }
    function dn(t) { var e, n, r, i, o, s; if (t == null || typeof t == "number" || typeof t == "string" || typeof t == "boolean")
        return t; if (Array.isArray(t))
        return t.slice(0, 16).map(dn); if (typeof t == "object") {
        var l = t;
        return "color" in l || "alpha" in l || "width" in l && !("x" in l) && !("y" in l) && !("height" in l) ? { color: l.color, alpha: l.alpha, width: l.width } : "x" in l || "y" in l || "width" in l || "height" in l ? { x: Number((e = l.x) != null ? e : 0), y: Number((n = l.y) != null ? n : 0), w: Number((i = (r = l.width) != null ? r : l.w) != null ? i : 0), h: Number((s = (o = l.height) != null ? o : l.h) != null ? s : 0) } : Oe(l);
    } return String(t); }
    function Ce(t, e, n) {
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
            return Oe(t);
        if (n.add(t), Array.isArray(t))
            return t.slice(0, 256).map(function (i) { return Ce(i, e + 1, n); });
        var r = {};
        try {
            for (var _b = __values(Object.entries(t).slice(0, 128)), _c = _b.next(); !_c.done; _c = _b.next()) {
                var _d = __read(_c.value, 2), i = _d[0], o = _d[1];
                r[i] = Ce(o, e + 1, n);
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
    function $n(t) { if (t != null)
        return typeof t == "symbol" ? t.toString() : String(t); }
    function xi(t) { if (t != null)
        return typeof t == "function" ? { type: "function", name: t.name || void 0, arity: t.length } : typeof t == "object" ? { id: de(t), type: Oe(t) } : { type: typeof t }; }
    function vo(t) { if (t != null)
        return typeof t == "object" ? { id: de(t), type: Oe(t) } : typeof t == "function" ? { type: "function" } : { type: typeof t }; }
    function Go(t) { var e = { event: $n(t[0]), listener: xi(t[1]) }; return t.length > 2 && (e.context = vo(t[2])), [e]; }
    function $o(t) { return String(t != null ? t : "").slice(0, 240); }
    function Bo(t) {
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
    function Wo(t) { var s, l, a, d, f, h; if (!t || typeof t != "object")
        return; var e = t, n = Number((s = e.x) != null ? s : 0), r = Number((l = e.y) != null ? l : 0), i = Number((d = (a = e.width) != null ? a : e.w) != null ? d : 0), o = Number((h = (f = e.height) != null ? f : e.h) != null ? h : 0); if (!(!Number.isFinite(n) || !Number.isFinite(r) || !Number.isFinite(i) || !Number.isFinite(o)) && !(i <= 0 || o <= 0))
        return { x: n, y: r, w: i, h: o }; }
    function Ho(t, e) { if (e) {
        if (t === "addChild" || t === "removeChild")
            return e.map(function (n) { return n && typeof n == "object" ? de(n) : 0; });
        if (t === "mask") {
            var n = e[0];
            return [n && typeof n == "object" ? de(n) : 0];
        }
        if (t === "addChildAt" || t === "setChildIndex") {
            var n = e[0];
            return [n && typeof n == "object" ? de(n) : 0, Number(e[1]) || 0];
        }
        return t === "on" ? Go(e) : t === "snapshot" ? e : t === "text.text.set" ? e.length ? [$o(e[0])] : [] : t === "text.style.set" ? e.length ? [Bo(e[0])] : [] : e.map(dn);
    } }
    function hn(t, e, n) { var r, i; try {
        window.__TRUEOS_PIXI_CAPTURE_STEP__ = "record:".concat(e, ":begin");
        var o = window.__pixiCapture;
        if (!(o != null && o.enabled))
            return;
        o.counts[e] = ((r = o.counts[e]) != null ? r : 0) + 1;
        var s = { frame: bi, seq: ++No, op: e, id: t && typeof t == "object" ? de(t) : void 0, target: Lo(t), event: e === "on" && (n != null && n.length) ? $n(n[0]) : void 0, listener: e === "on" && (n != null && n.length) ? xi(n[1]) : void 0, args: Ho(e, n) };
        window.__TRUEOS_PIXI_CAPTURE_STEP__ = "record:".concat(e, ":push"), o.commands.push(s), o.persist && Fo(s), o.commands.length > fi && o.commands.splice(0, o.commands.length - fi), window.__TRUEOS_PIXI_CAPTURE_STEP__ = "record:".concat(e, ":done");
    }
    catch (o) {
        try {
            window.__TRUEOS_PIXI_CAPTURE_ERROR__ = "record:".concat(e, ":").concat(String((i = o == null ? void 0 : o.message) != null ? i : o));
        }
        catch (s) { }
    } }
    function Fo(t) { if (we.push(t), t.op === "snapshot") {
        ze();
        return;
    } if (we.length >= 512) {
        ze();
        return;
    } vn == null && (vn = window.setTimeout(function () { vn = null, ze(); }, 50)); }
    function ze() {
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
            var o = $n(n[0]), s = n[1];
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
            var o = Math.max(0, Number((r = n[0]) != null ? r : 0) | 0), s = Array.isArray(t.children) ? t.children.length : o, l = Math.max(o, Math.min(Number((i = n[1]) != null ? i : s) | 0, s)), a = t.children.splice(o, l - o);
            try {
                for (var a_1 = __values(a), a_1_1 = a_1.next(); !a_1_1.done; a_1_1 = a_1.next()) {
                    var d = a_1_1.value;
                    d.parent = null;
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
    function Xo() { var t = function () { return !!(window.__TRUEOS_PIXI_REPAINT_REQUIRED__ || window.__TRUEOS_PIXI_SCROLL_REPAINT_REQUIRED__); }; window.__TRUEOS_DISPATCH_PIXI_POINTER__ = function (e, n, r, i, o, s, l) {
        var e_30, _a;
        if (l === void 0) { l = 0; }
        var b, P, I, W, G, w, k, E, _, B, K, V, m, C, U, rt, ot, q, X, tt;
        var a = function (T) { try {
            window.__TRUEOS_PIXI_POINTER_DISPATCH_STEP__ = T, typeof console != "undefined" && typeof console.log == "function" && console.log("[trueos pointer dispatch] ".concat(T));
        }
        catch (L) { } };
        a("start node=".concat(Number(e) || 0, " event=").concat(String(n || "")));
        var d = window.__TRUEOS_PIXI_APP;
        if (String(n || "") === "wheel") {
            var T = d == null ? void 0 : d.canvas;
            if (!T || typeof T.dispatchEvent != "function")
                return a("wheel-canvas-missing"), { handled: 0, listenerCount: 0, painted: 0, targetFound: 0 };
            var L = (I = (P = (b = window.__pixiCapture) == null ? void 0 : b.commands) == null ? void 0 : P.length) != null ? I : 0, Z = { type: "wheel", deltaX: 0, deltaY: Number(l) || 0, deltaMode: 0, offsetX: Number(r) || 0, offsetY: Number(i) || 0, clientX: Number(r) || 0, clientY: Number(i) || 0, pointerId: Number(o) || 1, buttons: Number(s) || 0, defaultPrevented: !1, propagationStopped: !1, preventDefault: function () { this.defaultPrevented = !0; }, stopPropagation: function () { this.propagationStopped = !0; } };
            a("wheel-dispatch deltaY=".concat(Z.deltaY)), T.dispatchEvent(Z);
            var z = 0;
            if (window.__TRUEOS_CAPTURE_ONLY__) {
                var D = window.__TRUEOS_REPAINT_NOW__;
                t() && typeof D == "function" && (a("wheel-repaint-call"), D(), a("wheel-repaint-return"), z = 1);
            }
            else
                (W = d == null ? void 0 : d.renderer) != null && W.render && (d != null && d.stage) && (d.renderer.render(d.stage), z = 1);
            var dt = (k = (w = (G = window.__pixiCapture) == null ? void 0 : G.commands) == null ? void 0 : w.length) != null ? k : L, A = (E = T.listeners) == null ? void 0 : E.wheel, p = Array.isArray(A) ? A.length : typeof A == "function" ? 1 : 0, R = Z.defaultPrevented || p > 0 ? 1 : 0;
            return a("wheel-done handled=".concat(R, " listeners=").concat(p, " painted=").concat(z)), { handled: R, listenerCount: p, painted: dt > L || z ? 1 : 0, targetFound: 1 };
        }
        var f = gi.get(Number(e) || 0), h = 0, g = 0, x = 0;
        if (!f)
            return a("target-missing"), { handled: h, listenerCount: g, painted: x, targetFound: 0 };
        var M = { type: String(n || ""), button: Number(s) & 2 ? 2 : 0, buttons: Number(s) || 0, pointerId: Number(o) || 1, pointerType: "mouse", global: { x: Number(r) || 0, y: Number(i) || 0 }, data: { pointerId: Number(o) || 1, pointerType: "mouse", global: { x: Number(r) || 0, y: Number(i) || 0 } }, target: f, currentTarget: f, defaultPrevented: !1, propagationStopped: !1, preventDefault: function () { this.defaultPrevented = !0; }, stopPropagation: function () { this.propagationStopped = !0; } }, c = (K = (B = (_ = window.__pixiCapture) == null ? void 0 : _.commands) == null ? void 0 : B.length) != null ? K : 0;
        a("target-found label=".concat(String((V = f.label) != null ? V : "")));
        for (var T = f; T; T = T.parent) {
            M.currentTarget = T;
            var L = (m = T.listeners) == null ? void 0 : m[M.type];
            if (!(!Array.isArray(L) || L.length === 0)) {
                g += L.length, a("listeners node=".concat((C = Ye.get(T)) != null ? C : 0, " count=").concat(L.length));
                try {
                    for (var _b = (e_30 = void 0, __values(L.slice())), _c = _b.next(); !_c.done; _c = _b.next()) {
                        var Z = _c.value;
                        if (typeof Z == "function" && (h = 1, a("listener-call node=".concat((U = Ye.get(T)) != null ? U : 0)), Z.call(T, M), a("listener-return node=".concat((rt = Ye.get(T)) != null ? rt : 0)), M.propagationStopped))
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
                if (M.propagationStopped)
                    break;
            }
        }
        if (window.__TRUEOS_CAPTURE_ONLY__) {
            var T = window.__TRUEOS_REPAINT_NOW__;
            t() && typeof T == "function" && (a("capture-repaint-call"), T(), a("capture-repaint-return"), x = 1);
        }
        else
            (ot = d == null ? void 0 : d.renderer) != null && ot.render && (d != null && d.stage) && (a("paint-call"), d.renderer.render(d.stage), a("paint-return"), x = 1);
        return x = ((tt = (X = (q = window.__pixiCapture) == null ? void 0 : q.commands) == null ? void 0 : X.length) != null ? tt : c) > c || x ? 1 : 0, a("done handled=".concat(h, " listeners=").concat(g, " painted=").concat(x)), { handled: h, listenerCount: g, painted: x, targetFound: 1 };
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
            var l;
            if (hn(this, n, s), !window.__TRUEOS_CAPTURE_ONLY__)
                return r.apply(this, s);
            try {
                window.__TRUEOS_PIXI_CAPTURE_STEP__ = "invoke:".concat(n, ":begin");
                var a = Uo(this, n, s);
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
        } var d = this; n === "text.text.set" ? d._text = String(a != null ? a : "") : n === "text.style.set" ? d._style = a != null ? a : {} : n === "text.resolution.set" ? d._resolution = Math.max(1, Number(a) || 1) : Object.defineProperty(d, i, { configurable: !0, enumerable: !1, writable: !0, value: a }); } }), t.constructor["__pixiCapturePatched_".concat(n)] = !0; }
    function yi(t, e) {
        if (e === void 0) { e = 0; }
        var s, l, a, d, f, h, g, x, M;
        if (!t || e > 64)
            return null;
        var n, r;
        try {
            var c = typeof t.getGlobalPosition == "function" ? t.getGlobalPosition() : null;
            c && Number.isFinite(Number(c.x)) && Number.isFinite(Number(c.y)) && (n = Number(c.x), r = Number(c.y));
        }
        catch (c) { }
        var i = { id: de(t), type: Oe(t), label: (s = t.label) != null ? s : void 0, x: (d = (a = (l = t.position) == null ? void 0 : l.x) != null ? a : t.x) != null ? d : 0, y: (g = (h = (f = t.position) == null ? void 0 : f.y) != null ? h : t.y) != null ? g : 0, globalX: n, globalY: r, scaleX: Number.isFinite(Number((x = t.scale) == null ? void 0 : x.x)) ? Number(t.scale.x) : 1, scaleY: Number.isFinite(Number((M = t.scale) == null ? void 0 : M.y)) ? Number(t.scale.y) : 1, visible: t.visible, alpha: Number.isFinite(Number(t.alpha)) ? Number(t.alpha) : 1, maskId: t.mask ? de(t.mask) : 0, zIndex: Number(t.zIndex) || 0, sortableChildren: t.sortableChildren === !0 }, o = Wo(t.hitArea);
        if (o && (i.hitArea = o), t.listeners && typeof t.listeners == "object") {
            var c = Object.keys(t.listeners).filter(function (y) { var P; var b = (P = t.listeners) == null ? void 0 : P[y]; return Array.isArray(b) && b.length > 0; });
            c.length > 0 && (i.listeners = c.slice(0, 16));
        }
        if (t instanceof yt && Array.isArray(t.commands) && t.commands.length > 0 && (i.commands = t.commands.slice(-256).map(function (c) { return Ce(c, 0); })), typeof t.text == "string" && (i.text = t.text.slice(0, 120), t instanceof jt && t.style && typeof t.style == "object")) {
            var c = {}, y = t.style;
            typeof y.fontSize != "undefined" && (c.fontSize = Ce(y.fontSize, 0)), typeof y.fontWeight != "undefined" && (c.fontWeight = Ce(y.fontWeight, 0)), typeof y.fill != "undefined" && (c.fill = Ce(y.fill, 0)), Object.keys(c).length > 0 && (i.textStyle = c);
        }
        return Array.isArray(t.children) && t.children.length && (i.children = t.children.map(function (c) { return yi(c, e + 1); })), i;
    }
    function _i() {
        var e_31, _a, e_32, _b;
        if (window.__pixiCapture)
            return window.__pixiCapture;
        var t = { enabled: !0, persist: !window.__TRUEOS_CAPTURE_ONLY__, commands: [], counts: Object.create(null), clear: function () { this.commands.length = 0, this.counts = Object.create(null); }, dump: function (e) {
                if (e === void 0) { e = 200; }
                return this.commands.slice(-e);
            }, flush: function () { ze(); }, summary: function () { return qt({}, this.counts); } };
        if (window.__pixiCapture = t, Xo(), window.addEventListener("beforeunload", function () { return ze(); }), !pi) {
            pi = !0, typeof yt.prototype.image != "function" && (yt.prototype.image = function () { return this; });
            try {
                for (var _c = __values(["clear", "rect", "roundRect", "circle", "ellipse", "moveTo", "lineTo", "closePath", "poly", "fill", "stroke", "image", "svg"]), _d = _c.next(); !_d.done; _d = _c.next()) {
                    var e = _d.value;
                    Gn(yt.prototype, e);
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
                    Gn(Tt.prototype, e);
                }
            }
            catch (e_32_1) { e_32 = { error: e_32_1 }; }
            finally {
                try {
                    if (_g && !_g.done && (_b = _f.return)) _b.call(_f);
                }
                finally { if (e_32) throw e_32.error; }
            }
            Re(jt.prototype, "text", "text.text.set"), Re(jt.prototype, "style", "text.style.set"), Re(jt.prototype, "resolution", "text.resolution.set"), Gn(jt.prototype, "setSize", "text.setSize"), Re(Tt.prototype, "visible", "visible"), Re(Tt.prototype, "alpha", "alpha"), Re(Tt.prototype, "mask", "mask");
        }
        return t;
    }
    function wi(t) { var e = t.renderer, n = e == null ? void 0 : e.render; if (typeof n != "function" || n.__pixiCapturePatched)
        return; var r = function (o) { var l; var s = o && typeof o == "object" && "container" in o ? o.container : o || t.stage; return bi++, window.__TRUEOS_CAPTURE_ONLY__ && ((l = window.__pixiCapture) == null || l.clear()), hn(s, "render", []), hn(s, "snapshot", [yi(s)]), window.__TRUEOS_CAPTURE_ONLY__ ? s : n.call(this, o); }; r.__pixiCapturePatched = !0, e.render = r; }
    _i();
    var st = null, gn = 6, Te = 10, Ht = 1, Ft = 3, Xt = 4, Ae = 512, Oi = new Map;
    var u = { focusedKeyByPointer: new Map, keyboardOwnerPointerId: 1, inputs: new Map, sliders: new Map, sliderDrags: new Map, sliderBounds: new Map, dialogs: new Map, dialogDrags: new Map, dialogSelectedBy: new Map, dialogZ: new Map, dialogZCounter: 1, numbers: new Map, numberHolds: new Map, selects: new Map, temporals: new Map, temporalYearOwners: new Map, color: { rgb: { r: 255, g: 0, b: 0 }, a: 255, pick: null, draggingPointerId: null, bounds: null }, cursorColors: new Map, primaryMousePointerId: 1, harness: { enabled: !0, activeUserPointerId: Ht, periodMs: 3e3 }, userCursorPos: new Map, lastMouse: { x: 0, y: 0, has: !1 }, scroll: { y: 0, contentHeight: 0, viewportHeight: 0, draggingPointerId: null, dragOffsetY: 0, track: { x: 0, y: 0, w: Te, h: 0 }, thumb: { x: 0, y: 0, w: Te, h: 0 } }, iframeScroll: new Map, iframeScrollRoots: new Map, iframeScrollbarGraphics: new Map, iframeRects: [], hoverRects: [], hoverHandlers: new Map, hoveredKeyByPointer: new Map, hoveredCursorByPointer: new Map, virtualCursor: { enabled: !1, x: 0, y: 0, t: 0, radius: 120, speed: .9 }, textDrags: new Map, fieldBounds: new Map, dialogDragBounds: new Map, detailsOpen: new Map, contextMenus: new Map, clipboards: new Map }, mn = null, Fn = 0;
    function Ko(t) { if (!mn) {
        var n = document.createElement("canvas").getContext("2d");
        if (!n)
            throw new Error("2D canvas not available");
        mn = n;
    } return mn.font = "".concat(t.fontSize, "px ").concat(t.fontFamily), function (e) { return (Fn += 1, mn.measureText(e).width); }; }
    function Wn(t, e) {
        if (e === void 0) { e = 16; }
        return Object.entries(t).sort(function (n, r) { return r[1] - n[1] || (n[0] < r[0] ? -1 : n[0] > r[0] ? 1 : 0); }).slice(0, e).map(function (_a) {
            var _b = __read(_a, 2), n = _b[0], r = _b[1];
            return "".concat(n, ":").concat(r);
        }).join(",");
    }
    function Bn(t) { var e = (2166136261 ^ t.length) >>> 0, n = function (o, s) { for (var l = o; l < s; l += 1) {
        var a = t.charCodeAt(l);
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
    function Di(t) { return t.kind === "text" ? { kind: "text", text: t.text } : { kind: "block", key: t.key, tagName: t.tagName, attrs: Ai(t.attrs), children: t.children.map(Di) }; }
    function Ni(t) { var e, n, r; return t.kind === "text" ? { kind: "text", text: (e = t.text) != null ? e : "", x: t.x, y: t.y, width: t.width, height: t.height, children: [] } : { kind: "block", key: (n = t.key) != null ? n : "", tagName: (r = t.tagName) != null ? r : "", attrs: Ai(t.attrs), x: t.x, y: t.y, width: t.width, height: t.height, children: t.children.map(Ni) }; }
    function jo(t, e, n, r, i) {
        Pt("[trueos pixi widgets] prepixi stage=canonical-render begin");
        var o = e.map(Di);
        Pt("[trueos pixi widgets] prepixi stage=canonical-render done"), Pt("[trueos pixi widgets] prepixi stage=canonical-layout begin");
        var s = Ni(n);
        Pt("[trueos pixi widgets] prepixi stage=canonical-layout done"), Pt("[trueos pixi widgets] prepixi stage=stringify begin");
        var l = JSON.stringify(o), a = JSON.stringify(s);
        Pt("[trueos pixi widgets] prepixi stage=stringify done render_bytes=".concat(l.length, " layout_bytes=").concat(a.length)), Pt("[trueos pixi widgets] prepixi stage=hash begin");
        var d = Bn(l), f = Bn(a), h = Bn("".concat(l, "\n").concat(a));
        Pt("[trueos pixi widgets] prepixi stage=hash done"), Pt("[trueos pixi widgets] prepixi stage=trace-stringify begin");
        var g = JSON.stringify({ version: 1, source: t, viewport: { width: r, height: i }, renderHash: d, layoutHash: f, hash: h, renderNodes: o, layout: s });
        return Pt("[trueos pixi widgets] prepixi stage=trace-stringify done bytes=".concat(g.length)), window.__TRUEOS_PIXI_PREPIX_TRACE__ = g, window.__TRUEOS_PIXI_PREPIX_HASH__ = h, window.__TRUEOS_PIXI_PREPIX_RENDER_HASH__ = d, window.__TRUEOS_PIXI_PREPIX_LAYOUT_HASH__ = f, Lt() && console.log("[trueos pixi widgets] prepixi source=".concat(t, " hash=").concat(h, " render_hash=").concat(d, " layout_hash=").concat(f, " bytes=").concat(g.length)), { hash: h, renderHash: d, layoutHash: f, bytes: g.length };
    }
    function De(t) { var e = typeof t == "string" ? t : ""; return e.indexOf("<truesurfer-") >= 0 && (e = e.replace(/<truesurfer-[A-Za-z0-9._-]+>/g, "")), e; }
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
    function Jo(t) { var e = De(t), n = e.indexOf("__trueos") >= 0 || e.indexOf("tsNu") >= 0 || e.indexOf("tsNum") >= 0; return e.indexOf("__TRUEOS_HOST_READY__") >= 0 && (e = e.replace(/__TRUEOS_HOST_READY__/g, "")), e.indexOf("__trueos") >= 0 && (e = Zo(e), e = e.replace(/__trueosNumberValue/g, "").replace(/__trueosHostNum/g, "").replace(/__trueosNum/g, "").replace(/__trueosNu/g, "").replace(/__trueos/g, "")), (e.indexOf("tsNu") >= 0 || e.indexOf("tsNum") >= 0) && (e = e.replace(/tsNum/g, "").replace(/tsNutsNutsNutsNu/g, "").replace(/tsNutsNutsNu/g, "").replace(/tsNutsNu/g, "").replace(/tsNu/g, "")), n && (e = Li(e.trimStart())), e; }
    function Zo(t) { var e = "__trueosN", n = t, r = 0; for (; r < n.length;) {
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
    function Qo(t) { var e = he(Un(t)); return !(e.length === 0 || e === "true" || e === "false" || e === "N" || e === "Nu" || e === "Num" || e.startsWith("<truesurfer-") || e.startsWith("__trueo")); }
    function Gi(t, e) { var r; var n = De(e) || "block"; t[n] = ((r = t[n]) != null ? r : 0) + 1; }
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
    function Mi(t, e) {
        if (e === void 0) { e = 12; }
        var n = [], r = function (i, o, s) { if (n.length >= e)
            return; if (i.kind === "text") {
            n.push("#".concat(n.length, "@").concat(o, ":").concat(s, " chars=").concat(i.text.length, " sample=\"").concat(wn(i.text), "\""));
            return;
        } var l = De(i.tagName || "block") || "block", a = i.key || ""; for (var d = 0; d < i.children.length; d += 1)
            r(i.children[d], l, a); };
        for (var i = 0; i < t.length; i += 1)
            r(t[i], "root", "");
        return n.join("|");
    }
    function Ii(t, e) {
        if (e === void 0) { e = 12; }
        var n = [], r = function (i, o, s) { var d; if (n.length >= e)
            return; if (i.kind === "text") {
            var f = (d = i.text) != null ? d : "";
            n.push("#".concat(n.length, "@").concat(o, ":").concat(s, " chars=").concat(f.length, " box=").concat(Math.round(i.x), ",").concat(Math.round(i.y), ",").concat(Math.round(i.width), ",").concat(Math.round(i.height), " sample=\"").concat(wn(f), "\""));
            return;
        } var l = De(i.tagName || "block") || "block", a = i.key || ""; for (var f = 0; f < i.children.length; f += 1)
            r(i.children[f], l, a); };
        return r(t, "root", ""), n.join("|");
    }
    function Pi(t, e) {
        if (e === void 0) { e = 24; }
        var n = [], r = new Set(["label", "input", "timeinput", "dateinput", "monthinput", "weekinput", "datetimelocalinput", "button", "select", "searchrow", "searchbutton"]), i = function (o, s, l, a) {
            var e_37, _a;
            var h;
            if (n.length >= e)
                return;
            var d = l + o.x, f = a + o.y;
            if (o.kind === "block") {
                var g = De(o.tagName || "block") || "block";
                if (r.has(g)) {
                    var x = wn(Tn(o), 36);
                    n.push("#".concat(n.length, "@").concat(s, ">").concat(g, ":").concat((h = o.key) != null ? h : "", " box=").concat(Math.round(d), ",").concat(Math.round(f), ",").concat(Math.round(o.width), ",").concat(Math.round(o.height), " text=\"").concat(x, "\""));
                }
                try {
                    for (var _b = __values(o.children), _c = _b.next(); !_c.done; _c = _b.next()) {
                        var x = _c.value;
                        i(x, g, d, f);
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
    function xn(t) { return (typeof t == "string" ? t : "").replace(/&quot;/g, '"').replace(/&#34;/g, '"').replace(/&#39;/g, "'").replace(/&apos;/g, "'").replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&amp;/g, "&"); }
    function Hn(t) { return he(xn((typeof t == "string" ? t : "").replace(/<[^>]*>/g, " "))); }
    function ns(t) { var e = 0, n = String(t != null ? t : ""); for (; e < n.length && n.charCodeAt(e) <= 32;)
        e += 1; for (n.charAt(e) === "/" && (e += 1); e < n.length && n.charCodeAt(e) <= 32;)
        e += 1; var r = e; for (; e < n.length;) {
        var i = n.charCodeAt(e);
        if (!(i >= 48 && i <= 57 || i >= 65 && i <= 90 || i >= 97 && i <= 122 || i === 45 || i === 58))
            break;
        e += 1;
    } return n.slice(r, e).toLowerCase(); }
    function rs(t) { return t === "h1" || t === "h2" || t === "h3" || t === "h4" || t === "h5" || t === "h6" || t === "summary" || t === "p" || t === "button" || t === "label" || t === "legend" || t === "option"; }
    function $i(t) { var e = typeof t == "string" ? t : "", n = [], r = function (f) { var h = Hn(f); h.length !== 0 && (h.startsWith("<truesurfer-") || h.startsWith("__trueo") || n.push(h)); }, i = [], o = e.toLowerCase(), s = o.indexOf("<body"); if (s >= 0) {
        var f = e.indexOf(">", s);
        s = f >= 0 ? f + 1 : s;
    }
    else
        s = 0; var l = o.indexOf("</body>", s), a = l >= 0 ? l : e.length, d = ""; for (; s < a && n.length < Ae;) {
        var f = e.charAt(s);
        if (f !== "<") {
            d += f, s += 1;
            continue;
        }
        var h = xn(d);
        if (h.length > 0) {
            for (var I = i.length - 1; I >= 0; I -= 1)
                if (i[I].wanted) {
                    i[I].text += " ".concat(h);
                    break;
                }
        }
        d = "";
        var g = e.indexOf(">", s + 1);
        if (g < 0)
            break;
        var x = e.slice(s, g + 1), M = e.slice(s + 1, g), c = ns(M);
        if (M.trimStart().charAt(0) === "/") {
            for (var I = i.length - 1; I >= 0; I -= 1) {
                var W = i.pop();
                if (W != null && W.wanted && r(W.text), (W == null ? void 0 : W.tag) === c)
                    break;
            }
            s = g + 1;
            continue;
        }
        if (c === "script" || c === "style" || c === "template") {
            var I = "</".concat(c, ">"), W = o.indexOf(I, g + 1);
            s = W >= 0 ? W + I.length : g + 1;
            continue;
        }
        if (c === "input") {
            var I = Si(x, "type").toLowerCase();
            (I === "button" || I === "submit" || I === "reset") && r(Si(x, "value"));
        }
        var b = x.length - 1;
        for (; b >= 0 && x.charCodeAt(b) <= 32;)
            b -= 1;
        b >= 1 && x.charAt(b) === ">" && x.charAt(b - 1) === "/" || c === "input" || c === "br" || c === "hr" || c === "img" || i.push({ tag: c, wanted: rs(c), text: "" }), s = g + 1;
    } if (d.length > 0) {
        var f = xn(d);
        for (var h = i.length - 1; h >= 0; h -= 1)
            if (i[h].wanted) {
                i[h].text += " ".concat(f);
                break;
            }
    } for (; i.length && n.length < Ae;) {
        var f = i.pop();
        f != null && f.wanted && r(f.text);
    } if (n.length === 0) {
        var f = o.indexOf("<body");
        if (f >= 0) {
            var c = e.indexOf(">", f);
            f = c >= 0 ? c + 1 : f;
        }
        else
            f = 0;
        var h = o.indexOf("</body>", f), g = h >= 0 ? h : e.length, x = !1, M = "";
        for (var c = f; c < g && n.length < Ae; c += 1) {
            var y = e.charAt(c);
            if (y === "<") {
                r(M), M = "", x = !0;
                continue;
            }
            if (y === ">") {
                x = !1;
                continue;
            }
            x || (M += y);
        }
        r(M);
    } return n; }
    function yn(t) { var e = window == null ? void 0 : window[t]; return e !== void 0 ? e : globalThis == null ? void 0 : globalThis[t]; }
    function is(t, e) {
        if (e === void 0) { e = 8; }
        var n = [];
        for (var r = 0; r < t.length && n.length < e; r += 1)
            n.push("#".concat(r, "=\"").concat(bn(t[r], 48), "\""));
        return n.join("|");
    }
    function Si(t, e) { var i, o, s; var r = new RegExp("".concat(e, "[ \\t\\r\\n\\f]*=[ \\t\\r\\n\\f]*(\"([^\"]*)\"|'([^']*)'|([^ \\t\\r\\n\\f>]+))"), "i").exec(t); return xn((s = (o = (i = r == null ? void 0 : r[2]) != null ? i : r == null ? void 0 : r[3]) != null ? o : r == null ? void 0 : r[4]) != null ? s : ""); }
    function je(t) { var e = []; for (var n = 0; n < t.length; n += 1) {
        var r = t[n];
        typeof r == "string" && _n(e, r);
    } return e; }
    function os(t) { var e = "", n = !1; for (var r = 0; r < t.length; r += 1) {
        var i = t.charCodeAt(r);
        if (i === 32 || i === 9 || i === 10 || i === 13 || i === 12) {
            n = !0;
            continue;
        }
        n && e.length > 0 && (e += " "), e += t.charAt(r), n = !1;
    } return e; }
    function _n(t, e) { var n = os(e); if (n.length !== 0 && !(n.indexOf("<truesurfer-") === 0 || n.indexOf("__trueo") === 0)) {
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
                _n(e, n), n = "", i === "\r" && t.charAt(r + 1) === "\n" && (r += 1);
                continue;
            }
            n += i;
        }
        return _n(e, n), e;
    }
    function as(t) { var e = []; if (!Array.isArray(t))
        return e; for (var n = 0; n < t.length; n += 1) {
        var r = t[n];
        typeof r == "string" && _n(e, r);
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
        return { source: "array", rows: s }; var l = $i(t); if (Lt()) {
        var a = Array.isArray(n) && typeof n[0] == "string" ? bn(n[0], 72) : "", d = typeof e == "string" ? bn(e, 72) : "";
        console.log("[trueos pixi widgets] text-fallback-globals text_type=".concat(typeof e, " text_len=").concat(typeof e == "string" ? e.length : 0, " text_rows=").concat(o.length, " text_sample=\"").concat(d, "\" array=").concat(Array.isArray(n) ? n.length : -1, " array_rows=").concat(s.length, " array0=\"").concat(a, "\" html_len=").concat(t.length, " html_rows=").concat(l.length));
    } return { source: "html", rows: l }; }
    function ds() { var e; var t = yn("__TRUEOS_WIDGET_RENDER_TREE_JSON__"); if (typeof t == "string" && t.length > 0)
        try {
            return { source: "json", tree: JSON.parse(t) };
        }
        catch (n) {
            Lt() && console.log("[trueos pixi widgets] render-tree-json parse failed err=".concat(String((e = n == null ? void 0 : n.message) != null ? e : n)));
        } return { source: "window", tree: yn("__TRUEOS_WIDGET_RENDER_TREE__") }; }
    function hs(t) { var o, s, l, a; var e = [], n = String(t != null ? t : "").replace(/<script[^]*?<\/script>/gi, " ").replace(/<style[^]*?<\/style>/gi, " "), r = /<(h[1-6]|p|label|button)\b[^>]*>([^]*?)<\/\1>|<input\b[^>]*>/gi, i; for (; (i = r.exec(n)) && e.length < Ae;) {
        var d = (o = i[0]) != null ? o : "", f = String((s = i[1]) != null ? s : "").toLowerCase();
        if (d.toLowerCase().startsWith("<input"))
            continue;
        var h = Hn(f === "p" || f === "label" ? (l = i[2]) != null ? l : "" : (a = i[2]) != null ? a : "");
        h.length > 0 && e.push(h);
    } return e; }
    function ms(t) { var e = hs(t), n = je(e); return je(n); }
    function fs(t, e, n, r) {
        var e_38, _a;
        var a, d, f, h, g, x;
        var i = je((d = Oi.get(String((a = t.key) != null ? a : ""))) != null ? d : []), o = je(String((h = (f = t.attrs) == null ? void 0 : f["data-trueos-srcdoc-text"]) != null ? h : "").split("\n").map(function (M) { return he(M); })), s = i.length > 0 ? i : o.length > 0 ? o : ms(String((x = (g = t.attrs) == null ? void 0 : g.srcdoc) != null ? x : "")), l = n + 48;
        try {
            for (var s_2 = __values(s), s_2_1 = s_2.next(); !s_2_1.done; s_2_1 = s_2.next()) {
                var M = s_2_1.value;
                if (r.length >= Ae)
                    return;
                r.push({ x: e + 16, y: l, text: M }), l += 32;
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
        var c, y, b;
        if (e.length >= Ae)
            return;
        var l = i + r.x, a = o + r.y, d = r.kind === "block" && r.tagName === "iframe" && String((y = (c = r.attrs) == null ? void 0 : c["data-root"]) != null ? y : "") !== "1", f = s + (d ? 1 : 0), h = r.kind === "block" && r.tagName === "button", g = r.kind === "text" ? (b = r.text) != null ? b : "" : h ? Tn(r) : "", x = he(Un(g)), M = e.length;
        if (Qo(x)) {
            var P = h ? l + 8 : l, I = h ? a + Math.max(0, Math.floor((r.height - ye.fontSize * 1.25) / 2)) : a;
            e.push({ x: P, y: I, text: x });
        }
        if (!h) {
            try {
                for (var _b = __values(r.children), _c = _b.next(); !_c.done; _c = _b.next()) {
                    var P = _c.value;
                    n(P, l, a, f);
                }
            }
            catch (e_39_1) { e_39 = { error: e_39_1 }; }
            finally {
                try {
                    if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
                }
                finally { if (e_39) throw e_39.error; }
            }
            d && e.length === M && fs(r, l, a, e);
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
        var i, o, s, l;
        var t = (o = (i = window.__pixiCapture) == null ? void 0 : i.commands) != null ? o : [], e = {}, n = {}, r = new Set(["addChild", "addChildAt", "setChildIndex", "removeChild", "removeChildren", "removeAllListeners", "on", "clear", "rect", "roundRect", "circle", "ellipse", "moveTo", "lineTo", "closePath", "poly", "fill", "stroke", "image", "visible", "alpha", "scale", "mask", "text.text.set", "text.style.set", "text.resolution.set", "text.setSize", "render", "snapshot"]);
        try {
            for (var t_2 = __values(t), t_2_1 = t_2.next(); !t_2_1.done; t_2_1 = t_2.next()) {
                var a = t_2_1.value;
                var d = De(a == null ? void 0 : a.op);
                d && (e[d] = ((s = e[d]) != null ? s : 0) + 1, r.has(d) || (n[d] = ((l = n[d]) != null ? l : 0) + 1));
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
        if (!Lt())
            return;
        var s = bs();
        window.__TRUEOS_PIXI_BRIDGE_STATS__ = { renderNodes: t.nodes, renderBlocks: t.blocks, renderText: t.text, renderTags: Wn(t.tags, 24), renderTextSamples: n, layoutBoxes: e.nodes, layoutBlocks: e.blocks, layoutText: e.text, layoutMaxDepth: e.maxDepth, layoutTextSamples: r, layoutWidgetSamples: i, prePixiHash: o.hash, prePixiRenderHash: o.renderHash, prePixiLayoutHash: o.layoutHash, prePixiTraceBytes: o.bytes, measureTextCalls: Fn, scrollbarVisible: u.scroll.track.h > 0 ? 1 : 0, scrollbarTrack: "".concat(Math.round(u.scroll.track.x), ",").concat(Math.round(u.scroll.track.y), ",").concat(Math.round(u.scroll.track.w), ",").concat(Math.round(u.scroll.track.h)), scrollbarThumb: "".concat(Math.round(u.scroll.thumb.x), ",").concat(Math.round(u.scroll.thumb.y), ",").concat(Math.round(u.scroll.thumb.w), ",").concat(Math.round(u.scroll.thumb.h)), pixiCommands: s.total, pixiOps: s.ops, pixiUnsupported: s.unsupported };
    }
    var Ri = new WeakMap;
    function Xn(t, e) { var n = t; for (; n;) {
        if (n === e)
            return !0;
        n = n.parent;
    } return !1; }
    function Bi(t) { return Array.isArray(t.children) || (t.children = []), t.children; }
    function Qt(t, e, n) { var r = Number(e) || 0, i = Number(n) || 0; (!t.position || typeof t.position != "object") && (t.position = { x: 0, y: 0 }), t.position.x = r, t.position.y = i; }
    function Ke(t, e, n) { if (e === t || Xn(t, e))
        return; var r = Bi(t); if (e.parent !== t) {
        var l = Math.max(0, Math.min(n, r.length));
        t.addChildAt(e, l);
        return;
    } var i = Math.max(0, r.length - 1), o = Math.max(0, Math.min(n, i)); t.getChildIndex(e) !== o && t.setChildIndex(e, o); }
    function Ci(t, e, n) { if (e === t || Xn(t, e))
        return; var r = Bi(t); if (e.parent !== t) {
        var l = Math.max(0, Math.min(n, r.length));
        t.addChildAt(e, l);
        return;
    } var i = Math.max(0, r.length - 1), o = Math.max(0, Math.min(n, i)); t.getChildIndex(e) !== o && t.setChildIndex(e, o); }
    var pn = null, at = null;
    function Vt(t) { var e = u.cursorColors.get(t); if (e != null)
        return e; var n = [1118481, 2450411, 1483594, 14427686, 8141549, 959977, 16096779], r = Math.abs(Number(t) || 0) % n.length, i = n[r]; return u.cursorColors.set(t, i), i; }
    function Wt(t) { var i, o, s, l, a, d; var e = Number((s = (o = t == null ? void 0 : t.pointerId) != null ? o : (i = t == null ? void 0 : t.data) == null ? void 0 : i.pointerId) != null ? s : 0), r = String((d = (a = t == null ? void 0 : t.pointerType) != null ? a : (l = t == null ? void 0 : t.data) == null ? void 0 : l.pointerType) != null ? d : "").toLowerCase() === "mouse" || e === 1 || e === u.primaryMousePointerId; return u.harness.enabled && r ? u.harness.activeUserPointerId : e; }
    function Lt() { return !!globalThis.__TRUEOS_CAPTURE_ONLY__; }
    function Et(t) { var i; if (!Lt() || (window.__TRUEOS_PIXI_APP_PHASE__ = t, !{ "main:start": !0, "main:yoga": !0, "main:create-app": !0, "main:attach-capture": !0, "main:append-canvas": !0, "main:capture-flags": !0, "main:canvas-listeners": !0, "main:stage:done": !0, "main:roots": !0, "main:text-measure": !0, "main:html": !0, "main:render-tree": !0, "main:first-rerender": !0, "main:layout-build": !0, "main:layout-commit": !0, "main:paint:clamp": !0, "main:paint:render-to-pixi": !0, "main:paint:scrollbar": !0, "main:paint:renderer-render": !0, "main:paint:done": !0, "main:cursor-setup": !0, "main:input-listeners": !0, "main:done": !0 }[t]))
        return; var n = window, r = (i = n.__TRUEOS_PIXI_PHASE_TRACE_SEEN__) != null ? i : n.__TRUEOS_PIXI_PHASE_TRACE_SEEN__ = {}; r[t] || (r[t] = 1, console.log("[Trace] [pixi] phase=".concat(t))); }
    function F(t) { Lt() && (window.__TRUEOS_PIXI_LAYOUT_STEP__ = t); }
    function Pt(t) { Lt() && console.log(t); }
    function ne(t, e, n) { var o; if (!Lt())
        return; var r = "__TRUEOS_".concat(t, "_LOG_COUNT__"), i = Number((o = window[r]) != null ? o : 0) || 0; i >= e || (window[r] = i + 1, console.log(n)); }
    function Wi(t) { var l, a, d, f, h; var e = (l = window.__TRUEOS_PIXI_APP_PHASE__) != null ? l : "unknown", n = (a = window.__TRUEOS_PIXI_LAYOUT_STEP__) != null ? a : "", r = t, i = String((d = r == null ? void 0 : r.name) != null ? d : "Error"), o = String((f = r == null ? void 0 : r.message) != null ? f : t), s = String((h = r == null ? void 0 : r.stack) != null ? h : ""); return "phase=".concat(e, " layout=").concat(n, " name=").concat(i, " message=").concat(o, " stack=").concat(s); }
    function xs() { var t = Math.max(1, Number(window.innerWidth || 1920) | 0), e = Math.max(1, Number(window.innerHeight || 1080) | 0), n = new bt(0, 0, t, e), r = document.createElement("canvas"), i = { width: t, height: e, screen: n, render: function (o) { return o; }, resize: function (o, s) { var l = Math.max(1, Number(o || t) | 0), a = Math.max(1, Number(s || e) | 0); this.width = l, this.height = a, n.width = l, n.height = a; } }; return { stage: new Tt, screen: n, canvas: r, renderer: i, ticker: { stop: function () { }, add: function () { }, remove: function () { } } }; }
    function ys() { var b = 0, P = 0, I = 2e4; return { Node: { create: function () { return ({ children: [], measureFunc: null, paddingLeft: 0, paddingTop: 0, paddingRight: 0, paddingBottom: 0, marginLeft: 0, marginTop: 0, marginRight: 0, marginBottom: 0, width: 0, height: 0, minWidth: 0, minHeight: 0, flexDirection: 0, alignItems: 0, justifyContent: 1, flexWrap: 0, positionType: 0, positionLeft: null, positionTop: null, positionRight: null, positionBottom: null, computed: { left: 0, top: 0, width: 0, height: 0 }, debugLabel: "node", setMeasureFunc: function (w) { this.measureFunc = w; }, setMargin: function (w, k) { var E = Number(k) || 0; w === 0 ? this.marginLeft = E : w === 1 ? this.marginTop = E : w === 2 ? this.marginRight = E : w === 3 && (this.marginBottom = E); }, setPadding: function (w, k) { var E = Number(k) || 0; w === 0 ? this.paddingLeft = E : w === 1 ? this.paddingTop = E : w === 2 ? this.paddingRight = E : w === 3 && (this.paddingBottom = E); }, setFlexDirection: function (w) { this.flexDirection = w; }, setAlignItems: function (w) { this.alignItems = Number(w) || 0; }, setJustifyContent: function (w) { this.justifyContent = Number(w) || 0; }, setFlexWrap: function (w) { this.flexWrap = Number(w) === 1 ? 1 : 0; }, setFlexGrow: function (w) { }, setFlexShrink: function (w) { }, setAlignSelf: function (w) { }, setPositionType: function (w) { this.positionType = Number(w) === 1 ? 1 : 0; }, setPosition: function (w, k) { var E = Number(k) || 0; w === 0 ? this.positionLeft = E : w === 1 ? this.positionTop = E : w === 2 ? this.positionRight = E : w === 3 && (this.positionBottom = E); }, setWidth: function (w) { this.width = Math.max(0, Number(w) || 0); }, setHeight: function (w) { this.height = Math.max(0, Number(w) || 0); }, setMinWidth: function (w) { this.minWidth = Math.max(0, Number(w) || 0); }, setMinHeight: function (w) { this.minHeight = Math.max(0, Number(w) || 0); }, insertChild: function (w, k) { this.children.splice(Math.max(0, Math.min(k, this.children.length)), 0, w); }, getChildCount: function () { return this.children.length; }, getComputedLeft: function () { return this.computed.left; }, getComputedTop: function () { return this.computed.top; }, getComputedWidth: function () { return this.computed.width; }, getComputedHeight: function () { return this.computed.height; }, freeRecursive: function () { }, calculateLayout: function (w, k) {
                    if (w === void 0) { w = this.width; }
                    if (k === void 0) { k = this.height; }
                    this.layout(0, 0, Math.max(1, Number(w) || this.width || 1), Math.max(1, Number(k) || this.height || 1));
                }, layout: function (w, k, E, _) {
                    var e_41, _a, e_42, _b, e_43, _c;
                    var C, U, rt, ot;
                    if (b += 1, (b <= 80 || b % 500 === 0) && (P += 1, P <= 140 && Pt("[trueos pixi widgets] yoga-layout-call #".concat(b, " label=\"").concat(this.debugLabel, "\" children=").concat(this.children.length, " flex=").concat(this.flexDirection, " pos=").concat(this.positionType, " xy=").concat(Math.round(w), ",").concat(Math.round(k), " avail=").concat(Math.round(E), "x").concat(Math.round(_), " own=").concat(Math.round(this.width), "x").concat(Math.round(this.height), " min=").concat(Math.round(this.minWidth), "x").concat(Math.round(this.minHeight)))), b > I)
                        throw new Error("capture yoga layout budget exceeded count=".concat(b, " label=\"").concat(this.debugLabel, "\" children=").concat(this.children.length, " flex=").concat(this.flexDirection, " pos=").concat(this.positionType, " avail=").concat(Math.round(E), "x").concat(Math.round(_)));
                    var B = this.paddingLeft + this.paddingRight, K = this.paddingTop + this.paddingBottom, V = Math.max(this.minWidth, this.width || E), m = Math.max(this.minHeight, this.height || 0);
                    if (this.computed.left = w, this.computed.top = k, this.computed.width = V, this.measureFunc) {
                        var q = this.measureFunc(Math.max(0, V - B), 0);
                        m = Math.max(m, Math.ceil(Number(q.height) || 0) + K), this.computed.height = m;
                        return;
                    }
                    if (this.flexDirection === 1) {
                        var q = this.paddingLeft, X = 0, tt = Math.max(1, this.children.length);
                        try {
                            for (var _d = __values(this.children), _f = _d.next(); !_f.done; _f = _d.next()) {
                                var T = _f.value;
                                if (T.positionType === 1)
                                    continue;
                                var L = T.width || T.minWidth || Math.max(24, (V - B) / tt);
                                T.layout(q + T.marginLeft, this.paddingTop + T.marginTop, L, _), q += T.computed.width + T.marginLeft + T.marginRight, X = Math.max(X, T.computed.height + T.marginTop + T.marginBottom);
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
                                var T = _h.value;
                                if (T.positionType === 1) {
                                    var L = T.width || T.minWidth || Math.max(0, V - B - T.marginLeft - T.marginRight), Z = T.height || T.minHeight || _, z = T.positionLeft != null ? this.paddingLeft + T.positionLeft : Math.max(0, V - this.paddingRight - ((C = T.positionRight) != null ? C : 0) - L), dt = T.positionTop != null ? this.paddingTop + T.positionTop : Math.max(0, m - this.paddingBottom - ((U = T.positionBottom) != null ? U : 0) - Z);
                                    T.layout(z + T.marginLeft, dt + T.marginTop, L, Z);
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
                        m = Math.max(m, X + K);
                    }
                    else {
                        var q = this.paddingTop;
                        try {
                            for (var _j = __values(this.children), _k = _j.next(); !_k.done; _k = _j.next()) {
                                var X = _k.value;
                                if (X.positionType === 1) {
                                    var T = X.width || X.minWidth || Math.max(0, V - B - X.marginLeft - X.marginRight), L = X.height || X.minHeight || _, Z = X.positionLeft != null ? this.paddingLeft + X.positionLeft : Math.max(0, V - this.paddingRight - ((rt = X.positionRight) != null ? rt : 0) - T), z = X.positionTop != null ? this.paddingTop + X.positionTop : Math.max(0, m - this.paddingBottom - ((ot = X.positionBottom) != null ? ot : 0) - L);
                                    X.layout(Z + X.marginLeft, z + X.marginTop, T, L);
                                    continue;
                                }
                                var tt = Math.max(0, V - B - X.marginLeft - X.marginRight);
                                X.layout(this.paddingLeft + X.marginLeft, q + X.marginTop, tt, _), q += X.computed.height + X.marginTop + X.marginBottom;
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
    function _s(t) {
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
    function fn(t, e) { var o, s, l, a; var n = u.inputs.get(t); if (n)
        return n; var r = {}, i = ((o = e == null ? void 0 : e.type) != null ? o : "text").toLowerCase(); if (i === "checkbox" || i === "radio") {
        if (r.checked = e ? Object.prototype.hasOwnProperty.call(e, "checked") : !1, i === "checkbox") {
            var d = ((s = e == null ? void 0 : e["aria-checked"]) != null ? s : "").toLowerCase(), f = ((l = e == null ? void 0 : e["data-indeterminate"]) != null ? l : "").toLowerCase();
            r.indeterminate = (e ? Object.prototype.hasOwnProperty.call(e, "indeterminate") : !1) || d === "mixed" || f === "true" || f === "1" || f === "yes";
        }
    }
    else
        r.value = (a = e == null ? void 0 : e.value) != null ? a : ""; return u.inputs.set(t, r), r; }
    function ws(t) { var e = new Map; function n(r) {
        var e_46, _a;
        var i, o, s, l, a;
        if (r.kind === "block" && r.tagName === "input" && ((o = (i = r.attrs) == null ? void 0 : i.type) != null ? o : "text").toLowerCase() === "radio") {
            var h = "radio:".concat((l = (s = r.attrs) == null ? void 0 : s.name) != null ? l : "__default__"), g = r.key;
            if (g) {
                var x = (a = e.get(h)) != null ? a : [];
                x.push(g), e.set(h, x);
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
    function Hi(t, e, n) { var d, f; if (!t || typeof t != "object")
        return null; var r = t, i = typeof r.kind == "string" ? r.kind : ""; if (i === "text") {
        var h = typeof r.text == "string" ? r.text : "", g = "", x = (d = n == null ? void 0 : n.rows[n.index]) != null ? d : "", M = !1;
        if (n && n.index < n.rows.length ? (n.index += 1, g = x, M = !0) : g = he(Un(h)), !M && (h.indexOf("<truesurfer-") >= 0 || h.indexOf("__trueo") >= 0) || g.startsWith("<truesurfer-") || g.startsWith("__trueo"))
            g = "";
        else if (g.length === 0) {
            var y = (f = n == null ? void 0 : n.rows[n.index]) != null ? f : "";
            n && y && (n.index += 1), y && (g = y);
        }
        return g.length > 0 ? { kind: "text", text: g } : null;
    } if (i !== "block")
        return null; var o = typeof r.tagName == "string" ? r.tagName.toLowerCase() : ""; if (o.length === 0)
        return null; var s = typeof r.key == "string" ? r.key : "".concat(e, ":").concat(o), l = [], a = Array.isArray(r.children) ? r.children : []; for (var h = 0; h < a.length; h += 1) {
        var g = Hi(a[h], "".concat(e, ".").concat(h), n);
        g && l.push(g);
    } return { kind: "block", key: s, tagName: o, attrs: Ts(r.attrs), children: l }; }
    function Es(t, e) { var n = Array.isArray(t) ? t : t && typeof t == "object" && Array.isArray(t.widgetRenderTree) ? t.widgetRenderTree : [], i = { rows: Array.isArray(e) ? je(e) : $i(e), index: 0 }, o = []; for (var s = 0; s < n.length; s += 1) {
        var l = Hi(n[s], "0.".concat(s), i);
        l && o.push(l);
    } return o; }
    function Ms(t, e) { if (!Array.isArray(e) || e.length === 0)
        return 0; var n = 0, r = 0, i = function (o) { if (o.kind === "text") {
        if (n < e.length) {
            var s = e[n];
            n += 1, typeof s == "string" && s.length > 0 && s.indexOf("<truesurfer-") !== 0 && s.indexOf("__trueo") !== 0 && (o.text = s, r += 1);
        }
        return;
    } for (var s = 0; s < o.children.length; s += 1)
        i(o.children[s]); }; for (var o = 0; o < t.length; o += 1)
        i(t[o]); return r; }
    function Is(t) { var n = document.createElement("canvas").getContext("2d"); if (!n)
        throw new Error("2D canvas not available"); n.font = t; var r = t.indexOf("px"), i = r; for (; i > 0;) {
        var l = t.charCodeAt(i - 1);
        if (l < 48 || l > 57)
            break;
        i -= 1;
    } var o = r > i ? Number(t.slice(i, r)) : 16, s = Math.ceil(o * 1.25); return { measure: function (l, a) {
            var e_48, _a;
            Fn += 1;
            var d = he(l).split(" ").filter(Boolean);
            if (d.length === 0)
                return { width: 0, height: s, lines: [""] };
            var f = [], h = "";
            try {
                for (var d_1 = __values(d), d_1_1 = d_1.next(); !d_1_1.done; d_1_1 = d_1.next()) {
                    var M = d_1_1.value;
                    var c = h ? "".concat(h, " ").concat(M) : M, y = n.measureText(c).width, b = a != null ? a : Number.POSITIVE_INFINITY;
                    y <= b || !h ? h = c : (f.push(h), h = M);
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
            var g = Math.min(Math.max.apply(Math, __spreadArray([], __read(f.map(function (M) { return n.measureText(M).width; })), false)), a != null ? a : Number.POSITIVE_INFINITY), x = f.length * s;
            return { width: Math.ceil(g), height: Math.ceil(x), lines: f };
        }, lineHeight: s, font: t }; }
    function Ps(t, e, n) { var M; F("build:start nodes=".concat(t.length, " viewport=").concat(e, "x").concat(n)), window.__TRUEOS_PIXI_LAYOUT_BUILD_COUNT__ = 0, Pt("[trueos pixi widgets] layout-build begin nodes=".concat(t.length, " viewport=").concat(e, "x").concat(n)); var r = 12, i = 8, o = ye; F("build:measurer"); var s = Is("".concat(o.fontSize, "px ").concat(o.fontFamily)); function l(c) { return c.kind !== "block" || c.tagName === "hr" || c.tagName === "tr" || c.tagName === "td" || c.tagName === "th" ? 0 : i; } var a = 0; function d(c) { if (a += 1, (a <= 140 || a % 250 === 0) && Pt("[trueos pixi widgets] layout-box-build #".concat(a, " label=\"").concat(c, "\"")), a > 5e3)
        throw new Error("layout box build budget exceeded count=".concat(a, " label=\"").concat(c, "\"")); } function f(c) { var G; var y = c.kind === "text" ? "text:".concat(c.text.slice(0, 24)) : "".concat(c.tagName, ":").concat(c.key); if (F("node:".concat(y, ":start")), c.kind === "text") {
        var w_1 = st.Node.create();
        return w_1.debugLabel = y, F("node:".concat(y, ":measure-func")), w_1.setMeasureFunc(function (k, E) { F("node:".concat(y, ":measure-call")); var _ = E === st.MEASURE_MODE_UNDEFINED ? void 0 : Math.max(0, k), B = s.measure(c.text, _); return { width: B.width, height: B.height }; }), w_1.setMargin(st.EDGE_RIGHT, 6), w_1.setMargin(st.EDGE_BOTTOM, 0), { yogaNode: w_1, buildBox: function () { return (d(y), { kind: "text", text: c.text, x: w_1.getComputedLeft(), y: w_1.getComputedTop(), width: w_1.getComputedWidth(), height: w_1.getComputedHeight(), children: [] }); } };
    } if (c.tagName === "sliderlabel")
        return F("node:".concat(c.tagName, ":").concat(c.key, ":sliderlabel")), cr({ node: c, Yoga: st, measurer: s }); F("node:".concat(c.tagName, ":").concat(c.key, ":create")); var b = st.Node.create(); if (b.debugLabel = y, F("node:".concat(c.tagName, ":").concat(c.key, ":base-defaults")), b.setFlexDirection(st.FLEX_DIRECTION_COLUMN), b.setAlignItems(st.ALIGN_STRETCH), b.setPadding(st.EDGE_LEFT, r), b.setPadding(st.EDGE_RIGHT, r), b.setPadding(st.EDGE_TOP, r), b.setPadding(st.EDGE_BOTTOM, r), b.setMargin(st.EDGE_BOTTOM, 0), An(c.tagName) && (F("node:".concat(c.tagName, ":").concat(c.key, ":heading-defaults")), Mr(b, st)), c.tagName === "hr" && (F("node:".concat(c.tagName, ":").concat(c.key, ":hr-defaults")), gr(b, st)), (c.tagName === "p" || c.tagName === "label") && (F("node:".concat(c.tagName, ":").concat(c.key, ":inline-scan")), c.children.some(function (k) { return k.kind === "block" && (k.tagName === "input" || k.tagName === "button" || k.tagName === "select" || k.tagName === "textarea" || k.tagName === "timeinput" || k.tagName === "dateinput" || k.tagName === "monthinput" || k.tagName === "weekinput" || k.tagName === "datetimelocalinput" || k.tagName === "progress" || k.tagName === "meter" || k.tagName === "slider" || k.tagName === "number" || k.tagName === "color"); }) && (b.setFlexDirection(st.FLEX_DIRECTION_ROW), b.setFlexWrap(st.WRAP_WRAP), b.setAlignItems(st.ALIGN_CENTER)), b.setPadding(st.EDGE_TOP, 4), b.setPadding(st.EDGE_BOTTOM, 4), b.setPadding(st.EDGE_LEFT, 4), b.setPadding(st.EDGE_RIGHT, 4)), c.tagName === "table" && (F("node:".concat(c.tagName, ":").concat(c.key, ":table-defaults")), wr(b, st)), c.tagName === "tr" && (F("node:".concat(c.tagName, ":").concat(c.key, ":tr-defaults")), Tr(b, st)), (c.tagName === "td" || c.tagName === "th") && (F("node:".concat(c.tagName, ":").concat(c.key, ":cell-defaults")), Er(b, st)), c.tagName === "input" && (F("node:".concat(c.tagName, ":").concat(c.key, ":input-defaults")), Yr(b, c, st)), c.tagName === "textarea" && (F("node:".concat(c.tagName, ":").concat(c.key, ":textarea-defaults")), Kr(b, st)), c.tagName === "select" && (F("node:".concat(c.tagName, ":").concat(c.key, ":select-defaults")), ai(b, st)), c.tagName === "timeinput" || c.tagName === "dateinput" || c.tagName === "monthinput" || c.tagName === "weekinput" || c.tagName === "datetimelocalinput") {
        var w = c.tagName === "timeinput" ? "time" : c.tagName === "monthinput" ? "month" : c.tagName === "weekinput" ? "week" : c.tagName === "dateinput" ? "date" : "datetime-local";
        F("node:".concat(c.tagName, ":").concat(c.key, ":temporal-defaults")), ui(b, st, w);
    } c.tagName === "img" && (F("node:".concat(c.tagName, ":").concat(c.key, ":img-defaults")), Dr(b, c, st)), c.tagName === "svg" && (F("node:".concat(c.tagName, ":").concat(c.key, ":svg-defaults")), Br(b, c, st)), c.tagName === "canvas" && (F("node:".concat(c.tagName, ":").concat(c.key, ":canvas-defaults")), Hr(b, c, st)), c.tagName === "iframe" && (F("node:".concat(c.tagName, ":").concat(c.key, ":iframe-defaults")), Ur(b, c, st)), c.tagName === "button" && (F("node:".concat(c.tagName, ":").concat(c.key, ":button-defaults")), xr(b, st)), c.tagName === "dialog" && (F("node:".concat(c.tagName, ":").concat(c.key, ":dialog-defaults")), qr(b, st)), c.tagName === "number" && (F("node:".concat(c.tagName, ":").concat(c.key, ":number-defaults")), ei(b, st)), c.tagName === "color" && (F("node:".concat(c.tagName, ":").concat(c.key, ":color-defaults")), ii(b, c, st)), c.tagName === "searchrow" && (F("node:".concat(c.tagName, ":").concat(c.key, ":searchrow-defaults")), Jr(b, st)), c.tagName === "searchbutton" && (F("node:".concat(c.tagName, ":").concat(c.key, ":searchbutton-defaults")), Zr(b, st)), c.tagName === "summary" && (F("node:".concat(c.tagName, ":").concat(c.key, ":summary-defaults")), hr(b, st)), c.tagName === "details" && (F("node:".concat(c.tagName, ":").concat(c.key, ":details-defaults")), mr(b, st)), c.tagName === "barrow" && (F("node:".concat(c.tagName, ":").concat(c.key, ":barrow-defaults")), Vr(b, st)), (c.tagName === "progress" || c.tagName === "meter") && (F("node:".concat(c.tagName, ":").concat(c.key, ":progress-defaults")), ar(b, st)), c.tagName === "slider" && (F("node:".concat(c.tagName, ":").concat(c.key, ":slider-defaults")), lr(b, st)), F("node:".concat(c.tagName, ":").concat(c.key, ":children-effective")); var P = fr(c, u.detailsOpen), I = Number((G = window.__TRUEOS_PIXI_LAYOUT_BUILD_COUNT__) != null ? G : 0) + 1; window.__TRUEOS_PIXI_LAYOUT_BUILD_COUNT__ = I, (I <= 120 || I % 50 === 0) && Pt("[trueos pixi widgets] layout-build-node #".concat(I, " label=\"").concat(y, "\" children=").concat(c.children.length, " effective=").concat(P.length)), F("node:".concat(c.tagName, ":").concat(c.key, ":children-map count=").concat(P.length)); var W = P.map(f); (I <= 120 || I % 50 === 0) && Pt("[trueos pixi widgets] layout-build-node-mapped #".concat(I, " label=\"").concat(y, "\" pairs=").concat(W.length)), F("node:".concat(c.tagName, ":").concat(c.key, ":children-insert")); for (var w = 0; w < W.length; w++) {
        var k = P[w], E = W[w];
        if (k && k.kind === "block") {
            var _ = w === W.length - 1 ? 0 : l(k);
            E.yogaNode.setMargin(st.EDGE_BOTTOM, _);
        }
        b.insertChild(E.yogaNode, b.getChildCount());
    } return { yogaNode: b, buildBox: function () { return (d(y), { kind: "block", key: c.key, tagName: c.tagName, attrs: c.attrs, x: b.getComputedLeft(), y: b.getComputedTop(), width: b.getComputedWidth(), height: b.getComputedHeight(), children: W.map(function (w) { return w.buildBox(); }) }); } }; } var h = st.Node.create(); h.debugLabel = "root", F("root:flex-direction"), h.setFlexDirection(st.FLEX_DIRECTION_COLUMN), F("root:align-items"), h.setAlignItems(st.ALIGN_STRETCH), F("root:width"), h.setWidth(e), F("root:height"), h.setHeight(n), F("root:padding-left"), h.setPadding(st.EDGE_LEFT, 16), F("root:padding-top"), h.setPadding(st.EDGE_TOP, 16), F("root:padding-right"), h.setPadding(st.EDGE_RIGHT, 16 + gn), F("root:padding-bottom"), h.setPadding(st.EDGE_BOTTOM, 16), F("root:children-map count=".concat(t.length)), Pt("[trueos pixi widgets] layout-root children-map count=".concat(t.length)); var g = t.map(f); F("root:children-insert"), Pt("[trueos pixi widgets] layout-root children-insert pairs=".concat(g.length)); for (var c = 0; c < g.length; c++) {
        var y = t[c], b = g[c];
        if (y && y.kind === "block") {
            var P = c === g.length - 1 ? 0 : l(y);
            b.yogaNode.setMargin(st.EDGE_BOTTOM, P);
        }
        h.insertChild(b.yogaNode, h.getChildCount());
    } F("root:calculate"), Pt("[trueos pixi widgets] layout-root calculate begin"), h.calculateLayout(e, n, st.DIRECTION_LTR), Pt("[trueos pixi widgets] layout-root calculate done"), F("root:build-box"), Pt("[trueos pixi widgets] layout-root build-box begin"), d("root"); var x = { kind: "block", tagName: "root", x: 0, y: 0, width: h.getComputedWidth(), height: h.getComputedHeight(), children: g.map(function (c) { return c.buildBox(); }) }; return Pt("[trueos pixi widgets] layout-root build-box done boxes=".concat(a)), F("root:free"), (M = h.freeRecursive) == null || M.call(h), F("build:done"), x; }
    function Ss(t, e, n) {
        var e_49, _a, e_50, _b, e_51, _c, e_52, _d, e_53, _f;
        var K, V;
        F("render:start");
        var r = ye, i = n != null ? n : t.stage;
        F("render:get-background");
        var o = Ot(i, "__background");
        F("render:get-content-root");
        var s = ae(i, "__contentRoot");
        F("render:get-dialog-root");
        var l = ae(i, "__dialogRoot");
        F("render:get-overlay-root");
        var a = ae(i, "__overlayRoot");
        F("render:ensure-background"), Ci(i, o, 0), F("render:ensure-content-root"), Ke(i, s, 1), F("render:ensure-dialog-root"), Ke(i, l, 2), F("render:ensure-overlay-root"), Ke(i, a, 3), F("render:overlay-remove-children"), a.removeChildren(), F("render:overlay-removed");
        var d = [], f = [], h = ws(e);
        F("render:clear-ui-state"), u.fieldBounds.clear(), u.sliderBounds.clear(), u.dialogDragBounds.clear(), u.hoverRects.length = 0, u.hoverHandlers.clear(), u.iframeRects.length = 0, u.iframeScrollRoots.clear(), u.iframeScrollbarGraphics.clear(), F("render:node-cache");
        var g = (K = Ri.get(i)) != null ? K : new Map;
        Ri.set(i, g);
        var x = new Set, M = function (m) {
            var e_54, _a;
            var rt;
            var C = 0, U = function (ot, q, X) {
                var e_55, _a;
                var L;
                if (ot.kind === "block" && ot.tagName === "dialog")
                    return;
                var tt = q + ot.x, T = X + ot.y;
                C = Math.max(C, T + ot.height);
                try {
                    for (var _b = __values((L = ot.children) != null ? L : []), _c = _b.next(); !_c.done; _c = _b.next()) {
                        var Z = _c.value;
                        U(Z, tt, T);
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
                    U(ot, 0, 0);
                }
            }
            catch (e_54_1) { e_54 = { error: e_54_1 }; }
            finally {
                try {
                    if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
                }
                finally { if (e_54) throw e_54.error; }
            }
            return C;
        }, c = new Set;
        try {
            for (var _g = __values(u.textDrags.values()), _h = _g.next(); !_h.done; _h = _g.next()) {
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
        F("render:measure");
        var y = Ko(r);
        function b(m, C, U) { return Math.max(C, Math.min(U, m)); }
        var P = function (m) {
            var e_56, _a;
            try {
                for (var _b = __values(u.textDrags.entries()), _c = _b.next(); !_c.done; _c = _b.next()) {
                    var _d = __read(_c.value, 2), C = _d[0], U = _d[1];
                    if (U.key === m)
                        return C;
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
        }, I = function (m) {
            var e_57, _a;
            var C = u.keyboardOwnerPointerId;
            if (u.focusedKeyByPointer.get(C) === m)
                return C;
            try {
                for (var _b = __values(u.focusedKeyByPointer.entries()), _c = _b.next(); !_c.done; _c = _b.next()) {
                    var _d = __read(_c.value, 2), U = _d[0], rt = _d[1];
                    if (rt === m)
                        return U;
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
        F("render:background-clear"), St(o), F("render:background-rect"), o.rect(0, 0, t.renderer.width, t.renderer.height), F("render:background-fill"), o.fill(r.background), F("render:content-position");
        {
            var m = u.scroll, C = m && Number(m.y || 0) || 0, U = s.position;
            U && (U.x = 0, U.y = -C);
        }
        F("render:content-position-done");
        function W(m, C, U, rt, ot, q, X, tt, T) {
            var e_58, _a;
            if (rt === void 0) { rt = 0; }
            if (ot === void 0) { ot = 0; }
            var R, D, S, v, N, O, $, J, Y, H, Q, nt, ht, wt, Mt, Rt, kt, xt, Ct, Nt;
            F("render:draw:".concat(tt, ":").concat(m.kind, ":").concat(m.kind === "block" ? m.tagName : "text", ":start"));
            var L = m.kind === "block" ? m.key && m.key.length > 0 ? m.key : "".concat(tt, ":").concat((R = m.tagName) != null ? R : "block") : "", Z = m.kind === "block" ? "b:".concat(L) : "t:".concat(tt);
            F("render:draw:".concat(tt, ":cache"));
            var z = g.get(Z);
            (!z || Xn(C, z)) && (F("render:draw:".concat(tt, ":new-container")), z = new Tt, z.label = Z, g.set(Z, z)), F("render:draw:".concat(tt, ":ensure-child")), x.add(Z), Ke(C, z, T), F("render:draw:".concat(tt, ":children-root"));
            var dt = ae(z, "__children");
            if (F("render:draw:".concat(tt, ":ensure-children-root")), Ke(z, dt, 1), F("render:draw:".concat(tt, ":position")), Qt(z, m.x, m.y), m.kind === "block" && m.tagName === "hr" && Qt(z, Math.round(m.x), Math.round(m.y)), m.kind === "block" && m.tagName === "dialog" && m.key) {
                var et = nn(u.dialogs, m.key), lt = Math.max(0, m.width), ut = Math.max(0, m.height), ct = X.x, Yt = X.y, Gt = Math.max(ct, X.x + X.w - lt), Bt = Math.max(Yt, X.y + X.h - ut);
                if (u.dialogDragBounds.set(m.key, { minX: ct, minY: Yt, maxX: Gt, maxY: Bt }), Lt() && !et.__trueosInitialPositionSeeded) {
                    var re = X.w <= 760 && X.h <= 800, te = ct + Math.max(12, Math.floor((X.w - lt) / 2)), me = Yt + Math.max(re ? 190 : 40, Math.floor((X.h - ut) / 2));
                    et.x = Math.max(ct, Math.min(Gt, te)), et.y = Math.max(Yt, Math.min(Bt, me)), et.__trueosInitialPositionSeeded = !0;
                }
                et.x = Math.max(ct, Math.min(Gt, et.x)), et.y = Math.max(Yt, Math.min(Bt, et.y)), Qt(z, et.x, et.y);
            }
            var A = rt + z.position.x, p = ot + z.position.y;
            if (m.kind === "block") {
                F("render:draw:".concat(tt, ":block:").concat(m.tagName, ":begin"));
                var et = U;
                (m.tagName === "h1" || m.tagName === "h2" || m.tagName === "h3" || m.tagName === "summary" || m.tagName === "th") && (et = { bold: !0 }), F("render:draw:".concat(tt, ":graphics"));
                var lt = Ot(z, "__g");
                F("render:draw:".concat(tt, ":graphics-clear")), St(lt), F("render:draw:".concat(tt, ":graphics-ensure")), Ci(z, lt, 0), lt.zIndex = -10;
                var ut = Math.max(0, m.width), ct = Math.max(0, m.height), Yt = null;
                if ((m.tagName === "h1" || m.tagName === "h2" || m.tagName === "h3") && (Qt(z, Math.round(m.x), Math.round(m.y)), ut = Math.round(ut), ct = Math.round(ct)), F("render:draw:".concat(tt, ":widget:").concat(m.tagName)), m.tagName === "hr")
                    pr({ graphics: lt, w: ut, theme: r });
                else if (m.tagName !== "barrow") {
                    if (m.tagName !== "searchrow") {
                        if (m.tagName === "searchbutton")
                            Qr({ node: m, container: z, graphics: lt, w: ut, h: ct, theme: r, uiState: u, getPointerId: Wt, focusInputKey: (D = m.attrs) == null ? void 0 : D["data-focus-key"], requestPaint: at });
                        else if (m.tagName === "progress" || m.tagName === "meter")
                            sr({ node: m, graphics: lt, w: ut, h: ct, theme: r });
                        else if (m.tagName === "sliderlabel")
                            ur({ node: m, container: z, theme: r, sliderStates: u.sliders });
                        else if (m.tagName === "slider")
                            en({ node: m, container: z, graphics: lt, w: ut, h: ct, absX: A, absY: p, theme: r, sliderStates: u.sliders, sliderBounds: u.sliderBounds, sliderDrags: u.sliderDrags, requestPaint: at, getPointerId: Wt });
                        else if (m.tagName === "timeinput" || m.tagName === "dateinput" || m.tagName === "monthinput" || m.tagName === "weekinput" || m.tagName === "datetimelocalinput")
                            di({ node: m, container: z, graphics: lt, w: ut, h: ct, absX: A, absY: p, theme: r, uiState: u, getPointerId: Wt, getCursorColor: Vt, temporalStates: u.temporals, yearSliderOwners: u.temporalYearOwners, getOrInitInputValue: function (j, it) { return fn(j, it); }, requestPaint: at, popupSink: f });
                        else if (m.tagName === "input") {
                            var j = m.key, it = j != null ? I(j) : null, It = j != null && u.focusedKeyByPointer.get(u.keyboardOwnerPointerId) === j, pt = j == null ? null : It ? u.keyboardOwnerPointerId : c.has(j) ? P(j) : null, vt = pt != null, $t = it != null ? Vt(it) : null;
                            zr({ node: m, container: z, graphics: lt, w: ut, h: ct, absX: A, absY: p, theme: r, textMeasure: y, uiState: u, getOrInitInputState: fn, clamp: b, radioGroups: h, textDrags: u.textDrags, requestPaint: at, showCaret: vt, caretPointerId: pt, focusColor: $t != null ? $t : void 0, getCursorColor: Vt, getPointerId: Wt });
                        }
                        else if (m.tagName === "textarea") {
                            var j = m.key, it = j != null ? I(j) : null, It = j != null && u.focusedKeyByPointer.get(u.keyboardOwnerPointerId) === j, pt = j == null ? null : It ? u.keyboardOwnerPointerId : c.has(j) ? P(j) : null, vt = pt != null, $t = it != null ? Vt(it) : null;
                            jr({ node: m, container: z, graphics: lt, w: ut, h: ct, absX: A, absY: p, theme: r, textMeasure: y, uiState: u, getOrInitInputState: fn, clamp: b, textDrags: u.textDrags, requestPaint: at, showCaret: vt, caretPointerId: pt, focusColor: $t != null ? $t : void 0, getCursorColor: Vt, getPointerId: Wt });
                        }
                        else if (m.tagName === "select") {
                            if (m.key) {
                                var j = Number((v = (S = m.attrs) == null ? void 0 : S["data-selected-index"]) != null ? v : "0");
                                ue(u.selects, m.key, Number.isFinite(j) ? j : 0);
                            }
                            an({ node: m, container: z, graphics: lt, w: ut, h: ct, absX: A, absY: p, theme: r, selectStates: u.selects, uiState: u, getPointerId: Wt, getCursorColor: Vt, requestPaint: at, popupSink: d });
                        }
                        else if (m.tagName === "summary")
                            m.key && u.hoverRects.push({ key: m.key, kind: "summary", cursor: "pointer", x: A, y: p, w: ut, h: ct }), dr({ node: m, container: z, w: ut, h: ct, theme: r, detailsOpen: u.detailsOpen, requestRerender: pn });
                        else if (m.tagName === "dialog")
                            ti({ node: m, container: z, w: ut, h: ct, theme: r, selectedBy: u.dialogSelectedBy, getCursorColor: Vt, dialogStates: u.dialogs, dialogDrags: u.dialogDrags, bringToFront: function (j) { u.dialogZ.set(j, u.dialogZCounter++); }, requestPaint: at, getPointerId: Wt });
                        else if (m.tagName === "img")
                            Ar({ node: m, container: z, graphics: lt, w: ut, h: ct, theme: r, requestRerender: pn });
                        else if (m.tagName === "svg") {
                            var j = (O = (N = m.attrs) == null ? void 0 : N["data-svg"]) != null ? O : "";
                            Wr({ svgMarkup: j, container: z, w: ut, h: ct, requestRerender: pn });
                        }
                        else if (m.tagName === "canvas")
                            Fr({ node: m, container: z, graphics: lt, w: ut, h: ct, theme: r });
                        else if (m.tagName === "iframe")
                            Xr({ node: m, container: z, graphics: lt, w: ut, h: ct, theme: r });
                        else if (m.tagName === "color")
                            u.color.bounds = { x: A, y: p, w: Math.max(0, ut), h: Math.max(0, ct) }, si({ node: m, container: z, graphics: lt, w: ut, h: ct, theme: r, rgb: u.color.rgb, setRgb: function (j) { u.color.rgb = j; }, alpha: u.color.a, setAlpha: function (j) { u.color.a = Math.max(0, Math.min(255, Math.round(j))); }, pick: u.color.pick, setPick: function (j) { u.color.pick = j; }, requestPaint: at, getPointerId: Wt, setDraggingPointerId: function (j) { u.color.draggingPointerId = j; } });
                        else if (m.tagName === "number") {
                            var j_1 = m.key, it_1 = String((J = ($ = m.attrs) == null ? void 0 : $.channel) != null ? J : "").toLowerCase(), It_1 = it_1 === "r" || it_1 === "g" || it_1 === "b" || it_1 === "a";
                            j_1 && ni({ node: m, container: z, graphics: lt, w: ut, h: ct, theme: r, getValue: function () { var pt, vt; return It_1 ? it_1 === "a" ? (pt = u.color.a) != null ? pt : 255 : (vt = u.color.rgb[it_1]) != null ? vt : 0 : Nn(u.numbers, j_1, m.attrs).value; }, setValue: function (pt) { It_1 ? it_1 === "a" ? u.color.a = Math.max(0, Math.min(255, Math.round(pt))) : u.color.rgb[it_1] = Math.max(0, Math.min(255, Math.round(pt))) : Nn(u.numbers, j_1, m.attrs).value = pt; }, requestPaint: at, numberHolds: u.numberHolds, getPointerId: Wt });
                        }
                        else if (m.tagName === "button") {
                            var j = he(Tn(m));
                            m.key && u.hoverRects.push({ key: m.key, kind: "button", cursor: "pointer", x: A, y: p, w: ut, h: ct }), br({ container: z, graphics: lt, w: ut, h: ct, label: Lt() ? "" : j, theme: r, registerHoverHandlers: m.key ? function (it) { u.hoverHandlers.set(m.key, it); } : void 0 });
                        }
                        else if (!An(m.tagName))
                            if (m.tagName === "table")
                                yr({ graphics: lt, w: ut, h: ct, boxBorder: r.boxBorder });
                            else if (m.tagName === "td" || m.tagName === "th")
                                _r({ nodeTag: m.tagName, graphics: lt, w: ut, h: ct, theme: r });
                            else {
                                var j = Math.max(0, Math.round(ut)), it = Math.max(0, Math.round(ct));
                                lt.rect(0, 0, j, it), lt.stroke({ width: 1, color: r.boxBorder, alignment: 0 });
                            }
                    }
                }
                F("render:draw:".concat(tt, ":overlay-label")), Yt && z.addChild(Yt);
                var Gt = null, Bt = null, re = m.tagName === "iframe" && String((H = (Y = m.attrs) == null ? void 0 : Y["data-root"]) != null ? H : "") === "1";
                if (m.tagName === "iframe" && !re) {
                    m.key && u.iframeRects.push({ key: m.key, x: A, y: p, w: Math.max(0, ut), h: Math.max(0, ct) }), Gt = ae(z, "__iframeContentRoot"), Qt(Gt, 0, 0);
                    var pt = Ot(z, "__iframeContentMask");
                    St(pt);
                    var vt = 0, $t = 34, pe = Math.max(0, ut), En = Math.max(0, ct - 34);
                    pt.rect(vt, $t, pe, En), pt.fill(16777215), pt.alpha = 0, Gt.mask = pt;
                    var se_1 = (Q = m.key) != null ? Q : "", mt_1 = (nt = u.iframeScroll.get(se_1)) != null ? nt : { y: 0, contentHeight: 0, viewportHeight: 0, draggingPointerId: null, dragOffsetY: 0, track: { x: 0, y: 0, w: Te, h: 0 }, thumb: { x: 0, y: 0, w: Te, h: 0 }, rect: { x: A, y: p, w: Math.max(0, ut), h: Math.max(0, ct) } };
                    mt_1.rect = { x: A, y: p, w: Math.max(0, ut), h: Math.max(0, ct) }, mt_1.contentHeight = M(m), mt_1.viewportHeight = Math.max(0, ct - 34 - 8);
                    var Ee_1 = Math.max(0, mt_1.contentHeight - mt_1.viewportHeight);
                    mt_1.y = Math.max(0, Math.min(mt_1.y, Ee_1)), Bt = ae(Gt, "__iframeScrollRoot"), Qt(Bt, 0, -mt_1.y), se_1 && u.iframeScrollRoots.set(se_1, Bt);
                    var ie = Ot(z, "__iframeScrollbar");
                    se_1 && u.iframeScrollbarGraphics.set(se_1, ie), St(ie), ie.eventMode = "static";
                    var Mn = gn, ge = Te, Ve = Math.max(0, ut - ge - Mn), In = 34 + Mn, Ge = Math.max(0, ct - 34 - Mn * 2), Yn = Ee_1 > .5 && Ge > 1;
                    if (ie.visible = Yn, Yn) {
                        var Pn = Math.max(24, (mt_1.viewportHeight || 1) / Math.max(1, mt_1.contentHeight) * Ge), Fi = Math.max(1, Ge - Pn), Ui = Ee_1 <= 0 ? 0 : mt_1.y / Ee_1, zn = In + Fi * Ui;
                        mt_1.track = { x: A + Ve, y: p + In, w: ge, h: Ge }, mt_1.thumb = { x: A + Ve, y: p + zn, w: ge, h: Pn }, ie.rect(Ve, In, ge, Ge), ie.fill({ color: 0, alpha: .06 }), ie.rect(Ve, zn, ge, Pn), ie.fill({ color: 0, alpha: .25 }), ie.on("pointerdown", function (Jt) { var Vn, Jn, Zn, Qn, qn, tr; if ((Jt == null ? void 0 : Jt.button) === 2)
                            return; var Sn = Wt(Jt); if (Sn <= 0)
                            return; var Je = (Jn = (Vn = Jt.global) == null ? void 0 : Vn.x) != null ? Jn : 0, be = (Qn = (Zn = Jt.global) == null ? void 0 : Zn.y) != null ? Qn : 0; if (!(Je >= mt_1.track.x && Je <= mt_1.track.x + mt_1.track.w && be >= mt_1.track.y && be <= mt_1.track.y + mt_1.track.h))
                            return; if (Je >= mt_1.thumb.x && Je <= mt_1.thumb.x + mt_1.thumb.w && be >= mt_1.thumb.y && be <= mt_1.thumb.y + mt_1.thumb.h) {
                            mt_1.draggingPointerId = Sn, mt_1.dragOffsetY = be - mt_1.thumb.y, u.iframeScroll.set(se_1, mt_1), (qn = Jt.stopPropagation) == null || qn.call(Jt);
                            return;
                        } var Kn = Math.max(1, mt_1.track.h - mt_1.thumb.h), jn = Math.max(mt_1.track.y, Math.min(mt_1.track.y + Kn, be - mt_1.thumb.h / 2)), Xi = (jn - mt_1.track.y) / Kn; mt_1.y = Math.max(0, Math.min(Ee_1, Xi * Ee_1)), mt_1.draggingPointerId = Sn, mt_1.dragOffsetY = be - jn, u.iframeScroll.set(se_1, mt_1), at == null || at(), (tr = Jt.stopPropagation) == null || tr.call(Jt); });
                    }
                    else
                        mt_1.track = { x: 0, y: 0, w: ge, h: 0 }, mt_1.thumb = { x: 0, y: 0, w: ge, h: 0 };
                    u.iframeScroll.set(se_1, mt_1);
                }
                var te = [], me = m.tagName === "dialog" || m.tagName === "iframe" && !re ? te : q, fe = X;
                if (m.tagName === "dialog")
                    fe = { x: 0, y: 0, w: Math.max(0, ut), h: Math.max(0, ct) };
                else if (m.tagName === "iframe" && !re) {
                    var j = (ht = m.key) != null ? ht : "", it = u.iframeScroll.get(j), It = it ? it.y : 0, pt = 34;
                    fe = { x: 0, y: pt + It, w: Math.max(0, ut), h: Math.max(0, ct - pt) };
                }
                var Ne = (wt = Bt != null ? Bt : Gt) != null ? wt : dt, Le = A + ((Mt = Gt == null ? void 0 : Gt.position.x) != null ? Mt : 0), ve = p + ((Rt = Gt == null ? void 0 : Gt.position.y) != null ? Rt : 0) + ((kt = Bt == null ? void 0 : Bt.position.y) != null ? kt : 0);
                F("render:draw:".concat(tt, ":children"));
                var ft = 0;
                for (var j = 0; j < ((xt = m.children) != null ? xt : []).length; j++) {
                    var it = ((Ct = m.children) != null ? Ct : [])[j];
                    if (it.kind === "block" && it.tagName === "dialog")
                        me.push(it);
                    else {
                        if (m.tagName === "button" && it.kind === "text" && !Lt())
                            continue;
                        W(it, Ne, et, Le, ve, me, fe, "".concat(tt, ".").concat(j), ft++);
                    }
                }
                if ((m.tagName === "dialog" || m.tagName === "iframe" && !re) && te.length > 0) {
                    te.sort(function (j, it) { var vt, $t; var It = j.key && (vt = u.dialogZ.get(j.key)) != null ? vt : 0, pt = it.key && ($t = u.dialogZ.get(it.key)) != null ? $t : 0; return It - pt; });
                    try {
                        for (var te_1 = __values(te), te_1_1 = te_1.next(); !te_1_1.done; te_1_1 = te_1.next()) {
                            var j = te_1_1.value;
                            var it = j.key && j.key.length > 0 ? j.key : "".concat(tt, ".dlg.").concat(ft);
                            W(j, Ne, et, Le, ve, te, fe, "".concat(tt, ".dlg.").concat(it), ft++);
                        }
                    }
                    catch (e_58_1) { e_58 = { error: e_58_1 }; }
                    finally {
                        try {
                            if (te_1_1 && !te_1_1.done && (_a = te_1.return)) _a.call(te_1);
                        }
                        finally { if (e_58) throw e_58.error; }
                    }
                }
            }
            else {
                F("render:draw:".concat(tt, ":text:begin"));
                var et = At(z, "__text", function (lt) { lt.style = { fontFamily: r.fontFamily, fontSize: r.fontSize, fill: r.text, fontWeight: U.bold ? "700" : "400", wordWrap: !0, wordWrapWidth: 0 }; });
                et.text = (Nt = m.text) != null ? Nt : "", et.style.fontFamily = r.fontFamily, et.style.fontSize = r.fontSize, et.style.fill = r.text, et.style.fontWeight = U.bold ? "700" : "400", et.style.wordWrap = !0, et.style.wordWrapWidth = Math.max(0, Math.ceil(m.width) + Ie), Qt(et, 0, _t), F("render:draw:".concat(tt, ":text:done"));
            }
        }
        F("render:root-loop");
        var G = { bold: !1 }, w = { x: 0, y: 0, w: t.renderer.width, h: t.renderer.height }, k = [], E = s.position, _ = E && Number(E.y || 0) || 0, B = 0;
        for (var m = 0; m < e.children.length; m++) {
            F("render:root-loop:".concat(m));
            var C = e.children[m];
            C && (C.kind === "block" && C.tagName === "dialog" ? k.push(C) : (F("render:root-loop:".concat(m, ":dispatch")), W(C, s, G, 0, _, k, w, "root.".concat(m), B++)));
        }
        if (F("render:root-dialogs"), k.length > 0) {
            k.sort(function (C, U) { var q, X; var rt = C.key && (q = u.dialogZ.get(C.key)) != null ? q : 0, ot = U.key && (X = u.dialogZ.get(U.key)) != null ? X : 0; return rt - ot; });
            var m = 0;
            try {
                for (var k_1 = __values(k), k_1_1 = k_1.next(); !k_1_1.done; k_1_1 = k_1.next()) {
                    var C = k_1_1.value;
                    var U = C.key && C.key.length > 0 ? C.key : "rootdlg.".concat(m);
                    W(C, l, G, 0, 0, k, w, "dlg.".concat(U), m++);
                }
            }
            catch (e_50_1) { e_50 = { error: e_50_1 }; }
            finally {
                try {
                    if (k_1_1 && !k_1_1.done && (_b = k_1.return)) _b.call(k_1);
                }
                finally { if (e_50) throw e_50.error; }
            }
        }
        if (F("render:temporal-popups"), f.length > 0 && hi({ popups: f, stage: a, theme: r, viewportW: t.renderer.width, viewportH: t.renderer.height, temporalStates: u.temporals, getOrInitInputValue: function (m, C) { return fn(m, C); }, sliders: u.sliders, sliderBounds: u.sliderBounds, sliderDrags: u.sliderDrags, selects: u.selects, selectPopups: d, uiFocus: u, getPointerId: Wt, getCursorColor: Vt, requestPaint: at }), F("render:select-popups"), d.length > 0)
            try {
                for (var d_2 = __values(d), d_2_1 = d_2.next(); !d_2_1.done; d_2_1 = d_2.next()) {
                    var m = d_2_1.value;
                    li({ popup: m, stage: a, theme: r, selectStates: u.selects, uiState: u, getPointerId: Wt, requestPaint: at, viewportW: t.renderer.width, viewportH: t.renderer.height });
                }
            }
            catch (e_51_1) { e_51 = { error: e_51_1 }; }
            finally {
                try {
                    if (d_2_1 && !d_2_1.done && (_c = d_2.return)) _c.call(d_2);
                }
                finally { if (e_51) throw e_51.error; }
            }
        F("render:context-menus");
        var _loop_3 = function (m, C) {
            if (!(C != null && C.open))
                return "continue";
            var U = new Tt;
            U.eventMode = "static", U.cursor = "default", Qt(U, C.x, C.y);
            var rt = 140, ot = 28, q = 6, X = ["Copy", "Paste", "Close"], tt = new yt;
            tt.rect(0, 0, rt + q * 2, X.length * ot + q * 2), tt.fill(16777215);
            var T = 1;
            tt.rect(T, T, rt + q * 2 - T * 2, X.length * ot + q * 2 - T * 2), tt.stroke({ width: 2, color: Vt(m), alignment: 0 }), U.addChild(tt), X.forEach(function (L, Z) { var z = q + Z * ot, dt = new Tt; dt.eventMode = "static", dt.cursor = "pointer", Qt(dt, q, z); var A = new yt; A.rect(0, 0, rt, ot), A.fill(16777215), dt.addChild(A); var p = Kt({ text: L, fontFamily: r.fontFamily, fontSize: r.fontSize, fill: r.text }); Qt(p, 8, Math.max(0, (ot - p.height) / 2) + _t), dt.addChild(p); var R = function (D) { return Wt(D) === m; }; dt.on("pointerover", function (D) { R(D) && (A.clear(), A.rect(0, 0, rt, ot), A.fill(15921906)); }), dt.on("pointerout", function (D) { R(D) && (A.clear(), A.rect(0, 0, rt, ot), A.fill(16777215)); }), dt.on("pointerdown", function (D) { var $, J, Y, H, Q, nt, ht, wt, Mt, Rt, kt; if (!R(D))
                return; ($ = D.stopPropagation) == null || $.call(D); var S = (J = u.focusedKeyByPointer.get(m)) != null ? J : null, v = S ? u.inputs.get(S) : null, N = S != null && u.fieldBounds.has(S) && v != null && typeof v.value == "string"; if (L === "Copy" && N) {
                var xt = v, Ct = (Y = xt.value) != null ? Y : "", Nt = (Q = (H = xt.selections) == null ? void 0 : H.get(m)) != null ? Q : null, et = Nt ? Math.max(0, Math.min(Ct.length, (nt = Nt.start) != null ? nt : 0)) : 0, lt = Nt ? Math.max(0, Math.min(Ct.length, (ht = Nt.end) != null ? ht : et)) : et, ut = Math.min(et, lt), ct = Math.max(et, lt), Yt = ut !== ct ? Ct.slice(ut, ct) : Ct;
                u.clipboards.set(m, Yt);
            }
            else if (L === "Paste" && N) {
                var xt = (wt = u.clipboards.get(m)) != null ? wt : "";
                if (xt.length > 0) {
                    var Ct = v, Nt = (Mt = Ct.value) != null ? Mt : "";
                    if (Ct.selections || (Ct.selections = new Map), !Ct.selections.has(m)) {
                        var Bt = Nt.length;
                        Ct.selections.set(m, { start: Bt, end: Bt });
                    }
                    var et = Ct.selections.get(m), lt = Math.max(0, Math.min(Nt.length, (Rt = et.start) != null ? Rt : Nt.length)), ut = Math.max(0, Math.min(Nt.length, (kt = et.end) != null ? kt : lt)), ct = Math.min(lt, ut), Yt = Math.max(lt, ut);
                    Ct.value = Nt.slice(0, ct) + xt + Nt.slice(Yt);
                    var Gt = ct + xt.length;
                    et.start = Gt, et.end = Gt;
                }
            } var O = u.contextMenus.get(m); O && (O.open = !1, u.contextMenus.set(m, O)), at == null || at(); }), U.addChild(dt); }), a.addChild(U);
        };
        try {
            for (var _j = __values(u.contextMenus.entries()), _k = _j.next(); !_k.done; _k = _j.next()) {
                var _l = __read(_k.value, 2), m = _l[0], C = _l[1];
                _loop_3(m, C);
            }
        }
        catch (e_52_1) { e_52 = { error: e_52_1 }; }
        finally {
            try {
                if (_k && !_k.done && (_d = _j.return)) _d.call(_j);
            }
            finally { if (e_52) throw e_52.error; }
        }
        F("render:prune-cache");
        try {
            for (var _m = __values(g.entries()), _p = _m.next(); !_p.done; _p = _m.next()) {
                var _q = __read(_p.value, 2), m = _q[0], C = _q[1];
                if (!x.has(m)) {
                    try {
                        C.removeFromParent(), (V = C.destroy) == null || V.call(C, { children: !0 });
                    }
                    catch (U) { }
                    g.delete(m);
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
        F("render:done");
    }
    function ks() {
        return Ze(this, null, function () {
            var t, e, n, r, _a, i_1, _b, o_3, s, l, a_2, d, f_1, h_1, g_1, x_1, c_1, y_1, b_2, P, I, _c, W, G_1, w_2, k, E_1, _1, B_1, K_1, V_1, m_2, C_2, U_3, rt_1, ot_2, q_3, X_1, tt_1, T_4, L, Z, z_2, dt_2, p, R, D, A_2, n_3, r;
            return __generator(this, function (_d) {
                switch (_d.label) {
                    case 0:
                        _d.trys.push([0, 9, , 10]);
                        Et("main:start");
                        n = (t = document.getElementById("app")) != null ? t : document.body, r = !0;
                        Et("main:yoga");
                        if (!r) return [3 /*break*/, 1];
                        _a = ys();
                        return [3 /*break*/, 3];
                    case 1: return [4 /*yield*/, Promise.resolve().then(function () { return (Ei(), Ti); })];
                    case 2:
                        _a = (_d.sent()).default;
                        _d.label = 3;
                    case 3:
                        st = _a, Et("main:create-app");
                        i_1 = r ? xs() : new tn;
                        _b = r;
                        if (_b) return [3 /*break*/, 5];
                        return [4 /*yield*/, i_1.init({ background: "#ffffff", resizeTo: window, antialias: !1, preference: "webgl" })];
                    case 4:
                        _b = (_d.sent());
                        _d.label = 5;
                    case 5:
                        _b, Et("main:attach-capture"), wi(i_1), window.__TRUEOS_PIXI_APP = i_1, Et("main:append-canvas"), n.appendChild(i_1.canvas), i_1.ticker.stop(), Et("main:capture-flags"), r && (u.harness.enabled = !1, u.virtualCursor.enabled = !1, window.__pixiCapture && (window.__pixiCapture.persist = !1)), Et("main:canvas-listeners"), i_1.canvas.addEventListener("contextmenu", function (p) { return p.preventDefault(); }), i_1.canvas.addEventListener("wheel", function (p) { var J, Y; var R = (J = p.offsetX) != null ? J : 0, D = (Y = p.offsetY) != null ? Y : 0, S = function (H) { var ht; if (!Lt())
                            return; var Q = window, nt = Number((ht = Q.__TRUEOS_WHEEL_ROUTE_LOG_COUNT__) != null ? ht : 0) || 0; nt >= 32 || (Q.__TRUEOS_WHEEL_ROUTE_LOG_COUNT__ = nt + 1, console.log("[trueos pixi widgets] wheel-route ".concat(H))); }, v = null; for (var H = u.iframeRects.length - 1; H >= 0; H--) {
                            var Q = u.iframeRects[H];
                            if (R >= Q.x && R <= Q.x + Q.w && D >= Q.y && D <= Q.y + Q.h) {
                                v = Q.key;
                                break;
                            }
                        } var N = !1; if (v) {
                            var H = u.iframeScroll.get(v);
                            if (H) {
                                var Q = Math.max(0, H.contentHeight - H.viewportHeight);
                                if (S("hit=iframe x=".concat(Math.round(R), " y=").concat(Math.round(D), " delta=").concat(Math.round(p.deltaY), " y0=").concat(Math.round(H.y), " max=").concat(Math.round(Q))), Q > 0) {
                                    var nt = Math.max(0, Math.min(Q, H.y + p.deltaY));
                                    nt !== H.y && (H.y = nt, u.iframeScroll.set(v, H), Lt() && (window.__TRUEOS_PIXI_SCROLL_REPAINT_REQUIRED__ = !0), at == null || at(), p.preventDefault(), N = !0, S("owner=iframe y1=".concat(Math.round(nt), " repaint=1")));
                                }
                            }
                        } if (N)
                            return; var O = Math.max(0, u.scroll.contentHeight - u.scroll.viewportHeight); if (O <= 0) {
                            S("owner=none x=".concat(Math.round(R), " y=").concat(Math.round(D), " delta=").concat(Math.round(p.deltaY), " root_y=").concat(Math.round(u.scroll.y), " root_max=0"));
                            return;
                        } var $ = Math.max(0, Math.min(O, u.scroll.y + p.deltaY)); if ($ !== u.scroll.y) {
                            var H = u.scroll.y;
                            u.scroll.y = $, Lt() && (window.__TRUEOS_PIXI_SCROLL_REPAINT_REQUIRED__ = !0), at == null || at(), p.preventDefault(), S("owner=root x=".concat(Math.round(R), " y=").concat(Math.round(D), " delta=").concat(Math.round(p.deltaY), " y0=").concat(Math.round(H), " y1=").concat(Math.round($), " max=").concat(Math.round(O), " repaint=1"));
                        }
                        else
                            S("owner=root-boundary x=".concat(Math.round(R), " y=").concat(Math.round(D), " delta=").concat(Math.round(p.deltaY), " y0=").concat(Math.round(u.scroll.y), " max=").concat(Math.round(O))); }, { passive: !1 }), Et("main:stage:eventMode"), i_1.stage.eventMode = "static", Et("main:stage:hitArea"), i_1.stage.hitArea = i_1.screen, Et("main:stage:on:pointerdown"), i_1.stage.on("pointerdown", function (p) {
                            var e_59, _a;
                            var R, D, S, v, N, O;
                            if ((p == null ? void 0 : p.button) === 2) {
                                var $ = Wt(p);
                                if ($ > 0) {
                                    var J = (R = u.contextMenus.get($)) != null ? R : { open: !1, x: 0, y: 0 };
                                    J.open = !0, J.x = (S = (D = p.global) == null ? void 0 : D.x) != null ? S : 0, J.y = (N = (v = p.global) == null ? void 0 : v.y) != null ? N : 0, u.contextMenus.set($, J);
                                }
                                at == null || at(), (O = p.preventDefault) == null || O.call(p);
                                return;
                            }
                            if ((p == null ? void 0 : p.button) !== 2) {
                                var $ = Wt(p), J = $ > 0 ? u.contextMenus.get($) : null;
                                J && J.open && (J.open = !1, u.contextMenus.set($, J), at == null || at());
                            }
                            if ((p == null ? void 0 : p.button) !== 2) {
                                var $ = !1;
                                try {
                                    for (var _b = __values(u.selects.values()), _c = _b.next(); !_c.done; _c = _b.next()) {
                                        var J = _c.value;
                                        J.open && (J.open = !1, $ = !0);
                                    }
                                }
                                catch (e_59_1) { e_59 = { error: e_59_1 }; }
                                finally {
                                    try {
                                        if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
                                    }
                                    finally { if (e_59) throw e_59.error; }
                                }
                                $ && (at == null || at());
                            }
                            (p == null ? void 0 : p.button) !== 2 && mi(u.temporals) && (at == null || at()), T_4();
                        }), Et("main:stage:done"), Et("main:roots");
                        o_3 = new Tt, s = new Tt;
                        s.eventMode = "static";
                        l = new Tt;
                        l.eventMode = "none", i_1.stage.addChild(o_3), i_1.stage.addChild(s), i_1.stage.addChild(l);
                        a_2 = new yt;
                        a_2.label = "__trueosGlobalScrollbar", a_2.eventMode = "static", s.addChild(a_2);
                        d = function (p, R) { p.clear(); var D = R.half, S = R.strokeWidth, v = R.color; p.moveTo(-D, 0), p.lineTo(D, 0), p.stroke({ width: S, color: v }), p.moveTo(0, -D), p.lineTo(0, D), p.stroke({ width: S, color: v }); }, f_1 = new yt;
                        f_1.eventMode = "none", f_1.visible = !1, l.addChild(f_1);
                        h_1 = new yt;
                        h_1.eventMode = "none", h_1.visible = !1, l.addChild(h_1);
                        g_1 = new yt;
                        g_1.eventMode = "none", g_1.visible = !1, l.addChild(g_1);
                        x_1 = new yt;
                        x_1.eventMode = "none", l.addChild(x_1), Et("main:text-measure");
                        c_1 = document.createElement("canvas").getContext("2d");
                        if (!c_1)
                            throw new Error("2D canvas not available");
                        c_1.font = "".concat(ye.fontSize, "px ").concat(ye.fontFamily);
                        y_1 = function (p) { return c_1.measureText(p).width; }, b_2 = ye.fontSize * 1.25;
                        Et("main:html");
                        if (!(typeof window.__TRUEOS_INPUT_HTML__ == "string")) return [3 /*break*/, 6];
                        _c = window.__TRUEOS_INPUT_HTML__;
                        return [3 /*break*/, 8];
                    case 6: return [4 /*yield*/, fetch("/input.html").then(function (p) { return p.text(); })];
                    case 7:
                        _c = _d.sent();
                        _d.label = 8;
                    case 8:
                        P = _c, I = es(P) ? P : "";
                        Lt() && console.log("[trueos pixi widgets] input-html chars=".concat(P.length, " usable=").concat(I ? 1 : 0, " sample=\"").concat(bn(P), "\"")), Et("main:render-tree"), Oi.clear();
                        W = us(I), G_1 = ds(), w_2 = Es(G_1.tree, W.rows), k = Ms(w_2, W.rows);
                        if (Lt() && (console.log("[trueos pixi widgets] text-fallback source=".concat(W.source, " rows=").concat(W.rows.length, " samples=").concat(is(W.rows))), console.log("[trueos pixi widgets] render-tree source=".concat(G_1.source, " nodes=").concat(w_2.length, " trusted_text_applied=").concat(k))), w_2.length === 0)
                            throw new Error("TrueSurfer widget render tree is missing");
                        E_1 = qo(w_2), _1 = null, B_1 = { nodes: 0, blocks: 0, text: 0, maxDepth: 0, tags: {} }, K_1 = { hash: "", renderHash: "", layoutHash: "", bytes: 0 }, V_1 = 0, m_2 = function () { var p = Math.max(0, u.scroll.contentHeight - u.scroll.viewportHeight); u.scroll.y = Math.max(0, Math.min(u.scroll.y, p)); }, C_2 = function () { var p = i_1.renderer.width, R = i_1.renderer.height; u.scroll.viewportHeight = R; var D = u.scroll.contentHeight, S = Math.max(0, D - R), v = S > .5; if (a_2.clear(), a_2.visible = v, !v) {
                            u.scroll.track = { x: 0, y: 0, w: u.scroll.track.w, h: 0 }, u.scroll.thumb = { x: 0, y: 0, w: u.scroll.thumb.w, h: 0 };
                            return;
                        } var N = gn, O = Te, $ = Math.max(0, p - O - N), J = N, Y = Math.max(0, R - N * 2), Q = Math.max(24, R / Math.max(R, D) * Y), nt = Math.max(1, Y - Q), ht = S <= 0 ? 0 : u.scroll.y / S, wt = J + nt * ht; u.scroll.track = { x: $, y: J, w: O, h: Y }, u.scroll.thumb = { x: $, y: wt, w: O, h: Q }, a_2.rect($, J, O, Y), a_2.fill({ color: 0, alpha: .06 }), a_2.rect($, wt, O, Q), a_2.fill({ color: 0, alpha: .25 }); }, U_3 = function () {
                            var e_60, _a;
                            var R = gn, D = Te;
                            try {
                                for (var _b = __values(u.iframeScrollRoots.entries()), _c = _b.next(); !_c.done; _c = _b.next()) {
                                    var _d = __read(_c.value, 2), S = _d[0], v = _d[1];
                                    var N = u.iframeScroll.get(S);
                                    if (!N)
                                        continue;
                                    var O = Math.max(0, N.contentHeight - N.viewportHeight);
                                    N.y = Math.max(0, Math.min(N.y, O)), Qt(v, 0, -N.y);
                                    var $ = u.iframeScrollbarGraphics.get(S);
                                    if (!$) {
                                        u.iframeScroll.set(S, N);
                                        continue;
                                    }
                                    var J = Math.max(0, N.rect.w), Y = Math.max(0, N.rect.h), H = Math.max(0, J - D - R), Q = 34 + R, nt = Math.max(0, Y - 34 - R * 2), ht = O > .5 && nt > 1;
                                    if ($.clear(), $.visible = ht, !ht) {
                                        N.track = { x: 0, y: 0, w: D, h: 0 }, N.thumb = { x: 0, y: 0, w: D, h: 0 }, u.iframeScroll.set(S, N);
                                        continue;
                                    }
                                    var Mt = Math.max(24, (N.viewportHeight || 1) / Math.max(1, N.contentHeight) * nt), Rt = Math.max(1, nt - Mt), kt = O <= 0 ? 0 : N.y / O, xt = Q + Rt * kt;
                                    N.track = { x: N.rect.x + H, y: N.rect.y + Q, w: D, h: nt }, N.thumb = { x: N.rect.x + H, y: N.rect.y + xt, w: D, h: Mt }, $.rect(H, Q, D, nt), $.fill({ color: 0, alpha: .06 }), $.rect(H, xt, D, Mt), $.fill({ color: 0, alpha: .25 }), u.iframeScroll.set(S, N);
                                }
                            }
                            catch (e_60_1) { e_60 = { error: e_60_1 }; }
                            finally {
                                try {
                                    if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
                                }
                                finally { if (e_60) throw e_60.error; }
                            }
                        }, rt_1 = function () { if (_1) {
                            if (ne("PIXI_PAINT_STEP", 96, "[trueos pixi widgets] paint-step clamp begin"), Et("main:paint:clamp"), m_2(), ne("PIXI_PAINT_STEP", 96, "[trueos pixi widgets] paint-step render-to-pixi begin"), Et("main:paint:render-to-pixi"), Ss(i_1, _1, o_3), ne("PIXI_PAINT_STEP", 96, "[trueos pixi widgets] paint-step render-to-pixi done"), Et("main:paint:scrollbar"), ne("PIXI_PAINT_STEP", 96, "[trueos pixi widgets] paint-step scrollbar begin"), C_2(), Et("main:paint:renderer-render"), ne("PIXI_PAINT_STEP", 96, "[trueos pixi widgets] paint-step renderer-render begin"), i_1.renderer.render(i_1.stage), ne("PIXI_PAINT_STEP", 96, "[trueos pixi widgets] paint-step publish-stats begin"), ki(E_1, B_1, Mi(w_2), Ii(_1), Pi(_1), K_1), ne("PIXI_PAINT_STEP", 96, "[trueos pixi widgets] paint-step publish-stats done"), Lt()) {
                                ne("PIXI_PAINT_STEP", 96, "[trueos pixi widgets] paint-step overlays begin");
                                var p = ps(_1);
                                window.__TRUEOS_PIXI_LAYOUT_TEXT_OVERLAYS__ = p, V_1 < 4 && (V_1 += 1, console.log("[trueos pixi widgets] layout-text-overlays count=".concat(p.length, " samples=").concat(gs(p)))), ne("PIXI_PAINT_STEP", 96, "[trueos pixi widgets] paint-step overlays done");
                            }
                            Et("main:paint:done"), ne("PIXI_PAINT_STEP", 96, "[trueos pixi widgets] paint-step done");
                        } }, ot_2 = function () { Et("main:scroll-paint:clamp"), m_2(), Et("main:scroll-paint:content-position"); var p = ae(o_3, "__contentRoot"); Qt(p, 0, -u.scroll.y), Et("main:scroll-paint:scrollbar"), C_2(), Et("main:scroll-paint:iframe-scrollbars"), U_3(), Et("main:scroll-paint:renderer-render"), i_1.renderer.render(i_1.stage), ki(E_1, B_1, Mi(w_2), _1 ? Ii(_1) : "", _1 ? Pi(_1) : ""), Et("main:scroll-paint:done"); };
                        Lt() && (window.__TRUEOS_REPAINT_NOW__ = function () { var D; var p = window.__TRUEOS_PIXI_SCROLL_REPAINT_REQUIRED__ === !0; window.__TRUEOS_PIXI_DIRTY__ = !1, window.__TRUEOS_PIXI_REPAINT_REQUIRED__ = !1, window.__TRUEOS_PIXI_SCROLL_REPAINT_REQUIRED__ = !1; var R = Number((D = window.__TRUEOS_REPAINT_NOW_LOG_COUNT__) != null ? D : 0) || 0; R < 24 && (window.__TRUEOS_REPAINT_NOW_LOG_COUNT__ = R + 1, console.log("[trueos pixi widgets] repaint-now scrollOnly=".concat(p ? 1 : 0, " begin"))), p ? ot_2() : rt_1(), R < 24 && console.log("[trueos pixi widgets] repaint-now scrollOnly=".concat(p ? 1 : 0, " done")); });
                        q_3 = function () { Et("main:layout-build"), Pt("[trueos pixi widgets] rerender layout-build begin"); var p = Ps(w_2, window.innerWidth, window.innerHeight); Pt("[trueos pixi widgets] rerender layout-build done"), Pt("[trueos pixi widgets] rerender prepixi begin"), K_1 = jo(G_1.source, w_2, p, window.innerWidth, window.innerHeight), Pt("[trueos pixi widgets] rerender prepixi done"), Et("main:layout-commit"), _1 = p, Lt() && (window.__TRUEOS_PIXI_LAST_LAYOUT__ = p, window.__TRUEOS_PIXI_LAYOUT_TEXT_OVERLAYS__ = []), Pt("[trueos pixi widgets] rerender stats begin"), B_1 = ts(p), Pt("[trueos pixi widgets] rerender stats done"), Pt("[trueos pixi widgets] rerender scroll-height begin"), u.scroll.contentHeight = _s(p), u.scroll.viewportHeight = window.innerHeight, Pt("[trueos pixi widgets] rerender paint begin"), rt_1(), Pt("[trueos pixi widgets] rerender paint done"); };
                        pn = function () { q_3(); };
                        X_1 = !1, tt_1 = !1, T_4 = function () { if (Lt()) {
                            window.__TRUEOS_PIXI_DIRTY__ = !0;
                            return;
                        } tt_1 || X_1 || (tt_1 = !0, requestAnimationFrame(function () { tt_1 = !1, i_1.renderer.render(i_1.stage); })); };
                        at = function () { if (!X_1) {
                            if (Lt()) {
                                window.__TRUEOS_PIXI_DIRTY__ = !0, window.__TRUEOS_PIXI_REPAINT_REQUIRED__ = !0;
                                return;
                            }
                            X_1 = !0, requestAnimationFrame(function () { X_1 = !1, rt_1(); });
                        } }, Et("main:first-rerender"), q_3(), Et("main:cursor-setup");
                        L = 2, Z = 10, z_2 = Lt();
                        d(f_1, { half: Z, strokeWidth: L, color: Vt(Ht) }), d(h_1, { half: Z, strokeWidth: L, color: Vt(Ft) }), d(g_1, { half: Z, strokeWidth: L, color: Vt(Xt) });
                        dt_2 = 2;
                        if (d(x_1, { half: Z, strokeWidth: L, color: Vt(dt_2) }), u.userCursorPos.set(Ht, { x: i_1.renderer.width * .25, y: i_1.renderer.height * .5 }), u.userCursorPos.set(Ft, { x: i_1.renderer.width * .25 + 40, y: i_1.renderer.height * .5 + 20 }), u.userCursorPos.set(Xt, { x: i_1.renderer.width * .25 + 80, y: i_1.renderer.height * .5 + 40 }), f_1.visible = !z_2, h_1.visible = !z_2, g_1.visible = !z_2, !z_2) {
                            p = u.userCursorPos.get(Ht), R = u.userCursorPos.get(Ft), D = u.userCursorPos.get(Xt);
                            f_1.position.set(p.x, p.y), h_1.position.set(R.x, R.y), g_1.position.set(D.x, D.y);
                        }
                        x_1.visible = !z_2 && u.virtualCursor.enabled;
                        A_2 = function () { if (z_2) {
                            f_1.visible = !1, h_1.visible = !1, g_1.visible = !1, x_1.visible = !1;
                            return;
                        } var p = u.userCursorPos.get(Ht), R = u.userCursorPos.get(Ft), D = u.userCursorPos.get(Xt); p && (f_1.visible = !0, f_1.position.set(p.x, p.y)), R && (h_1.visible = !0, h_1.position.set(R.x, R.y)), D && (g_1.visible = !0, g_1.position.set(D.x, D.y)); var S = function (v, N) { var O = null, $ = null; for (var J = u.hoverRects.length - 1; J >= 0; J--) {
                            var Y = u.hoverRects[J];
                            if (v >= Y.x && v <= Y.x + Y.w && N >= Y.y && N <= Y.y + Y.h) {
                                O = Y.key, $ = Y.cursor;
                                break;
                            }
                        } return { hitKey: O, hitCursor: $ }; }; if (p) {
                            var _a = S(p.x, p.y), v = _a.hitKey, N = _a.hitCursor;
                            u.hoveredKeyByPointer.set(Ht, v), u.hoveredCursorByPointer.set(Ht, N);
                            var O = u.textDrags.has(Ht) || u.sliderDrags.has(Ht) || u.dialogDrags.has(Ht);
                            f_1.rotation = N != null || O ? Math.PI / 4 : 0;
                        } if (R) {
                            var _b = S(R.x, R.y), v = _b.hitKey, N = _b.hitCursor;
                            u.hoveredKeyByPointer.set(Ft, v), u.hoveredCursorByPointer.set(Ft, N);
                            var O = u.textDrags.has(Ft) || u.sliderDrags.has(Ft) || u.dialogDrags.has(Ft);
                            h_1.rotation = N != null || O ? Math.PI / 4 : 0;
                        } if (D) {
                            var _c = S(D.x, D.y), v = _c.hitKey, N = _c.hitCursor;
                            u.hoveredKeyByPointer.set(Xt, v), u.hoveredCursorByPointer.set(Xt, N);
                            var O = u.textDrags.has(Xt) || u.sliderDrags.has(Xt) || u.dialogDrags.has(Xt);
                            g_1.rotation = N != null || O ? Math.PI / 4 : 0;
                        } T_4(); };
                        u.harness.enabled && setInterval(function () {
                            var e_61, _a, e_62, _b;
                            var p = u.harness.activeUserPointerId, R = p === Ht ? Ft : p === Ft ? Xt : Ht;
                            if (u.harness.activeUserPointerId = R, u.lastMouse.has) {
                                var Y = u.userCursorPos.get(p), H = u.userCursorPos.get(R);
                                u.userCursorPos.set(R, { x: u.lastMouse.x, y: u.lastMouse.y }), H ? u.userCursorPos.set(p, { x: H.x, y: H.y }) : Y && u.userCursorPos.set(p, { x: Y.x, y: Y.y });
                            }
                            var D = u.textDrags.size > 0, S = u.sliderDrags.size > 0, v = u.dialogDrags.size > 0, N = u.scroll.draggingPointerId != null, O = u.color.draggingPointerId != null, $ = !1;
                            try {
                                for (var _c = __values(u.iframeScroll.values()), _d = _c.next(); !_d.done; _d = _c.next()) {
                                    var Y = _d.value;
                                    if (Y.draggingPointerId != null) {
                                        $ = !0;
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
                            var J = D || S || v || N || O || $;
                            u.textDrags.delete(Ht), u.textDrags.delete(Ft), u.textDrags.delete(Xt), u.sliderDrags.delete(Ht), u.sliderDrags.delete(Ft), u.sliderDrags.delete(Xt), u.dialogDrags.delete(Ht), u.dialogDrags.delete(Ft), u.dialogDrags.delete(Xt);
                            try {
                                for (var _f = __values([Ht, Ft, Xt]), _g = _f.next(); !_g.done; _g = _f.next()) {
                                    var Y = _g.value;
                                    var H = u.numberHolds.get(Y);
                                    H && (H.timeoutId != null && window.clearTimeout(H.timeoutId), H.intervalId != null && window.clearInterval(H.intervalId), u.numberHolds.delete(Y));
                                }
                            }
                            catch (e_62_1) { e_62 = { error: e_62_1 }; }
                            finally {
                                try {
                                    if (_g && !_g.done && (_b = _f.return)) _b.call(_f);
                                }
                                finally { if (e_62) throw e_62.error; }
                            }
                            (u.scroll.draggingPointerId === Ht || u.scroll.draggingPointerId === Ft || u.scroll.draggingPointerId === Xt) && (u.scroll.draggingPointerId = null), (u.color.draggingPointerId === Ht || u.color.draggingPointerId === Ft || u.color.draggingPointerId === Xt) && (u.color.draggingPointerId = null), A_2(), J && (at == null || at());
                        }, u.harness.periodMs), !z_2 && u.virtualCursor.enabled && i_1.ticker.add(function () { var N, O, $, J, Y; var p = Math.max(0, i_1.ticker.deltaMS) / 1e3; x_1.visible = !0, u.virtualCursor.t += p; var R = i_1.renderer.width * .75, D = i_1.renderer.height * .25, S = u.virtualCursor.t * u.virtualCursor.speed, v = u.virtualCursor.radius; u.virtualCursor.x = R + Math.cos(S) * v, u.virtualCursor.y = D + Math.sin(S) * v, x_1.position.set(u.virtualCursor.x, u.virtualCursor.y); {
                            var H = dt_2, Q = u.virtualCursor.x, nt = u.virtualCursor.y, ht = null, wt = null;
                            for (var kt = u.hoverRects.length - 1; kt >= 0; kt--) {
                                var xt = u.hoverRects[kt];
                                if (Q >= xt.x && Q <= xt.x + xt.w && nt >= xt.y && nt <= xt.y + xt.h) {
                                    ht = xt.key, wt = xt.cursor;
                                    break;
                                }
                            }
                            var Mt = (N = u.hoveredKeyByPointer.get(H)) != null ? N : null;
                            Mt !== ht && (Mt && (($ = (O = u.hoverHandlers.get(Mt)) == null ? void 0 : O.out) == null || $.call(O)), ht && ((Y = (J = u.hoverHandlers.get(ht)) == null ? void 0 : J.over) == null || Y.call(J)), u.hoveredKeyByPointer.set(H, ht)), u.hoveredCursorByPointer.set(H, wt);
                            var Rt = u.textDrags.has(H) || u.sliderDrags.has(H) || u.dialogDrags.has(H);
                            x_1.rotation = wt != null || Rt ? Math.PI / 4 : 0;
                        } }), u.virtualCursor.x = i_1.renderer.width * .75 + u.virtualCursor.radius, u.virtualCursor.y = i_1.renderer.height * .25, x_1.position.set(u.virtualCursor.x, u.virtualCursor.y), Lt() && rt_1(), i_1.stage.on("pointerup", function (p) {
                            var e_63, _a;
                            var S, v, N;
                            var R = Wt(p), D = (v = (S = u.sliderDrags.get(R)) == null ? void 0 : S.key) != null ? v : null;
                            u.textDrags.delete(R), u.sliderDrags.delete(R), u.dialogDrags.delete(R), u.scroll.draggingPointerId === R && (u.scroll.draggingPointerId = null), u.color.draggingPointerId === R && (u.color.draggingPointerId = null);
                            try {
                                for (var _b = __values(u.iframeScroll.values()), _c = _b.next(); !_c.done; _c = _b.next()) {
                                    var O = _c.value;
                                    O.draggingPointerId === R && (O.draggingPointerId = null);
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
                                var O = u.numberHolds.get(R);
                                O && (O.timeoutId != null && window.clearTimeout(O.timeoutId), O.intervalId != null && window.clearInterval(O.intervalId), u.numberHolds.delete(R));
                            }
                            if (D) {
                                var O = (N = u.temporalYearOwners.get(D)) != null ? N : null;
                                if (O) {
                                    var $ = u.temporals.get(O);
                                    $ && $.openYear && ($.openYear = !1, u.temporals.set(O, $), at == null || at());
                                }
                            }
                            T_4();
                        }), i_1.stage.on("pointerupoutside", function (p) {
                            var e_64, _a;
                            var S, v, N;
                            var R = Wt(p), D = (v = (S = u.sliderDrags.get(R)) == null ? void 0 : S.key) != null ? v : null;
                            u.textDrags.delete(R), u.sliderDrags.delete(R), u.dialogDrags.delete(R), u.scroll.draggingPointerId === R && (u.scroll.draggingPointerId = null), u.color.draggingPointerId === R && (u.color.draggingPointerId = null);
                            try {
                                for (var _b = __values(u.iframeScroll.values()), _c = _b.next(); !_c.done; _c = _b.next()) {
                                    var O = _c.value;
                                    O.draggingPointerId === R && (O.draggingPointerId = null);
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
                                var O = u.numberHolds.get(R);
                                O && (O.timeoutId != null && window.clearTimeout(O.timeoutId), O.intervalId != null && window.clearInterval(O.intervalId), u.numberHolds.delete(R));
                            }
                            if (D) {
                                var O = (N = u.temporalYearOwners.get(D)) != null ? N : null;
                                if (O) {
                                    var $ = u.temporals.get(O);
                                    $ && $.openYear && ($.openYear = !1, u.temporals.set(O, $), at == null || at());
                                }
                            }
                            T_4();
                        }), a_2.on("pointerdown", function (p) { var nt, ht, wt, Mt, Rt, kt; if ((p == null ? void 0 : p.button) === 2)
                            return; var R = Wt(p); if (R <= 0)
                            return; var D = (ht = (nt = p.global) == null ? void 0 : nt.x) != null ? ht : 0, S = (Mt = (wt = p.global) == null ? void 0 : wt.y) != null ? Mt : 0, v = u.scroll.track, N = u.scroll.thumb; if (!(D >= v.x && D <= v.x + v.w && S >= v.y && S <= v.y + v.h))
                            return; var $ = Math.max(0, u.scroll.contentHeight - u.scroll.viewportHeight); if ($ <= .5)
                            return; if (D >= N.x && D <= N.x + N.w && S >= N.y && S <= N.y + N.h) {
                            u.scroll.draggingPointerId = R, u.scroll.dragOffsetY = S - N.y, (Rt = p.stopPropagation) == null || Rt.call(p);
                            return;
                        } var Y = Math.max(1, v.h - N.h), H = Math.max(v.y, Math.min(v.y + Y, S - N.h / 2)), Q = (H - v.y) / Y; u.scroll.y = Math.max(0, Math.min($, Q * $)), u.scroll.draggingPointerId = R, u.scroll.dragOffsetY = S - H, at == null || at(), (kt = p.stopPropagation) == null || kt.call(p); }), i_1.stage.on("pointermove", function (p) {
                            var e_65, _a;
                            var $, J, Y, H, Q, nt, ht, wt, Mt, Rt, kt, xt, Ct, Nt, et, lt, ut, ct, Yt, Gt, Bt, re, te, me, fe, Ne, Le, ve;
                            var R = Number((Y = (J = p == null ? void 0 : p.pointerId) != null ? J : ($ = p == null ? void 0 : p.data) == null ? void 0 : $.pointerId) != null ? Y : 1);
                            if (String((nt = (Q = p == null ? void 0 : p.pointerType) != null ? Q : (H = p == null ? void 0 : p.data) == null ? void 0 : H.pointerType) != null ? nt : "").toLowerCase() === "mouse" || R === 1) {
                                var ft = (wt = (ht = p.global) == null ? void 0 : ht.x) != null ? wt : 0, j = (Rt = (Mt = p.global) == null ? void 0 : Mt.y) != null ? Rt : 0;
                                u.lastMouse.x = ft, u.lastMouse.y = j, u.lastMouse.has = !0, u.primaryMousePointerId = R;
                                var it = u.harness.enabled ? u.harness.activeUserPointerId : R;
                                u.userCursorPos.set(it, { x: ft, y: j }), A_2();
                            }
                            var v = Wt(p);
                            if (v <= 0)
                                return;
                            var N = !1, O = !1;
                            {
                                var ft = u.textDrags.get(v);
                                if (ft) {
                                    var j = ft.key, it = u.fieldBounds.get(j), It = u.inputs.get(j);
                                    if (it && It && typeof It.value == "string") {
                                        var pt = it.isPassword ? "\u2022".repeat(It.value.length) : It.value, vt = ce(le(pt, Math.max(0, it.innerWidth), y_1), it.maxLines), $t = ((xt = (kt = p.global) == null ? void 0 : kt.x) != null ? xt : 0) - it.x - it.innerLeft, pe = ((Nt = (Ct = p.global) == null ? void 0 : Ct.y) != null ? Nt : 0) - it.y - it.innerTop, En = Pe({ fullText: pt, lines: vt, localX: $t, localY: pe, lineHeight: b_2, measure: y_1 });
                                        It.selections || (It.selections = new Map), It.selections.set(v, { start: ft.anchor, end: En }), N = !0;
                                    }
                                }
                            }
                            {
                                var ft = u.sliderDrags.get(v);
                                if (ft) {
                                    var j = ft.key, it = u.sliderBounds.get(j);
                                    if (it) {
                                        var pt = ((lt = (et = p.global) == null ? void 0 : et.x) != null ? lt : 0) - it.x, vt = Math.max(1, it.w - it.innerPad * 2), $t = (pt - it.innerPad) / vt, pe = _e(u.sliders, j, void 0);
                                        pe.value = Math.max(0, Math.min(1, $t)), N = !0;
                                    }
                                }
                            }
                            {
                                var ft = u.color.draggingPointerId;
                                if (ft != null && ft === v) {
                                    var j = u.color.bounds;
                                    if (j) {
                                        var it = (ct = (ut = p.global) == null ? void 0 : ut.x) != null ? ct : 0, It = (Gt = (Yt = p.global) == null ? void 0 : Yt.y) != null ? Gt : 0, pt = it - j.x, vt = It - j.y, $t = Ln({ lx: pt, ly: vt, w: j.w, h: j.h });
                                        $t && (u.color.rgb = $t, u.color.pick = { x: pt, y: vt }, N = !0);
                                    }
                                }
                            }
                            {
                                var ft = u.scroll.draggingPointerId;
                                if (ft != null && ft === v) {
                                    var j = u.scroll.track, it = u.scroll.thumb, It = Math.max(0, u.scroll.contentHeight - u.scroll.viewportHeight);
                                    if (It > .5 && j.h > 0 && it.h > 0) {
                                        var pt = (re = (Bt = p.global) == null ? void 0 : Bt.y) != null ? re : 0, vt = Math.max(1, j.h - it.h), pe = (Math.max(j.y, Math.min(j.y + vt, pt - u.scroll.dragOffsetY)) - j.y) / vt;
                                        u.scroll.y = Math.max(0, Math.min(It, pe * It)), N = !0, O = !0;
                                    }
                                }
                            }
                            try {
                                for (var _b = __values(u.iframeScroll.values()), _c = _b.next(); !_c.done; _c = _b.next()) {
                                    var ft = _c.value;
                                    if (ft.draggingPointerId == null || ft.draggingPointerId !== v)
                                        continue;
                                    var j = Math.max(0, ft.contentHeight - ft.viewportHeight);
                                    if (j <= .5 || ft.track.h <= 0 || ft.thumb.h <= 0)
                                        continue;
                                    var it = (me = (te = p.global) == null ? void 0 : te.y) != null ? me : 0, It = Math.max(1, ft.track.h - ft.thumb.h), vt = (Math.max(ft.track.y, Math.min(ft.track.y + It, it - ft.dragOffsetY)) - ft.track.y) / It;
                                    ft.y = Math.max(0, Math.min(j, vt * j)), N = !0, O = !0;
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
                                var ft = u.dialogDrags.get(v);
                                if (ft) {
                                    var j = nn(u.dialogs, ft.key), it = (Ne = (fe = p.global) == null ? void 0 : fe.x) != null ? Ne : 0, It = (ve = (Le = p.global) == null ? void 0 : Le.y) != null ? ve : 0;
                                    j.x = ft.originX + (it - ft.startGX), j.y = ft.originY + (It - ft.startGY);
                                    var pt = u.dialogDragBounds.get(ft.key);
                                    pt && (j.x = Math.max(pt.minX, Math.min(pt.maxX, j.x)), j.y = Math.max(pt.minY, Math.min(pt.maxY, j.y))), N = !0;
                                }
                            }
                            N && (O && Lt() && (window.__TRUEOS_PIXI_SCROLL_REPAINT_REQUIRED__ = !0), at == null || at());
                        }), Et("main:input-listeners"), window.addEventListener("keydown", function (p) {
                            var wt, Mt, Rt, kt, xt, Ct, Nt;
                            var R = u.keyboardOwnerPointerId, D = (wt = u.focusedKeyByPointer.get(R)) != null ? wt : null;
                            if (!D)
                                return;
                            var S = u.inputs.get(D);
                            if (!S || typeof S.value != "string")
                                return;
                            if (S.selections || (S.selections = new Map), !S.selections.has(R)) {
                                var et = S.value.length;
                                S.selections.set(R, { start: et, end: et });
                            }
                            var v = S.selections.get(R), N = S.value.length, O = function (et) { return Math.max(0, Math.min(N, et)); }, $ = O((Mt = v.start) != null ? Mt : N), J = O((Rt = v.end) != null ? Rt : $);
                            v.start = $, v.end = J;
                            var Y = Math.min($, J), H = Math.max($, J), Q = Y !== H, nt = function (et) { var lt = Math.max(0, Math.min(S.value.length, et)); v.start = lt, v.end = lt; }, ht = function (et, lt) { v.start = Math.max(0, Math.min(S.value.length, et)), v.end = Math.max(0, Math.min(S.value.length, lt)); };
                            if (p.key.toLowerCase() === "a" && (p.ctrlKey || p.metaKey)) {
                                ht(0, S.value.length), p.preventDefault(), rt_1();
                                return;
                            }
                            if (p.key === "ArrowLeft" || p.key === "ArrowRight") {
                                var et = p.key === "ArrowLeft" ? -1 : 1;
                                if (p.shiftKey) {
                                    var lt = (kt = v.start) != null ? kt : N, ut = ((xt = v.end) != null ? xt : lt) + et;
                                    ht(lt, ut);
                                }
                                else
                                    nt((Q ? Y : H) + et);
                                p.preventDefault(), q_3();
                                return;
                            }
                            if (p.key === "Home") {
                                p.shiftKey ? ht((Ct = v.start) != null ? Ct : N, 0) : nt(0), p.preventDefault(), q_3();
                                return;
                            }
                            if (p.key === "End") {
                                p.shiftKey ? ht((Nt = v.start) != null ? Nt : 0, S.value.length) : nt(S.value.length), p.preventDefault(), q_3();
                                return;
                            }
                            if (p.key === "Backspace") {
                                if (Q)
                                    S.value = S.value.slice(0, Y) + S.value.slice(H), nt(Y);
                                else {
                                    var et = H;
                                    et > 0 && (S.value = S.value.slice(0, et - 1) + S.value.slice(et), nt(et - 1));
                                }
                                p.preventDefault(), q_3();
                                return;
                            }
                            if (p.key === "Enter") {
                                var et = "\n";
                                if (Q)
                                    S.value = S.value.slice(0, Y) + et + S.value.slice(H), nt(Y + et.length);
                                else {
                                    var lt = H;
                                    S.value = S.value.slice(0, lt) + et + S.value.slice(lt), nt(lt + et.length);
                                }
                                p.preventDefault(), q_3();
                                return;
                            }
                            if (p.key === "Delete") {
                                if (Q)
                                    S.value = S.value.slice(0, Y) + S.value.slice(H), nt(Y);
                                else {
                                    var et = H;
                                    et < S.value.length && (S.value = S.value.slice(0, et) + S.value.slice(et + 1), nt(et));
                                }
                                p.preventDefault(), q_3();
                                return;
                            }
                            if (p.key === "Escape") {
                                u.focusedKeyByPointer.set(R, null), q_3();
                                return;
                            }
                            if (p.key.length === 1 && !p.ctrlKey && !p.metaKey && !p.altKey) {
                                if (Q)
                                    S.value = S.value.slice(0, Y) + p.key + S.value.slice(H), nt(Y + 1);
                                else {
                                    var et = H;
                                    S.value = S.value.slice(0, et) + p.key + S.value.slice(et), nt(et + 1);
                                }
                                p.preventDefault(), q_3();
                            }
                        }), window.addEventListener("resize", function () { q_3(), x_1.visible = u.virtualCursor.enabled; }), Et("main:done"), r && (window.__TRUEOS_PIXI_APP_READY__ = !0, window.__TRUEOS_PIXI_APP_ERROR__ = "", window.__TRUEOS_PIXI_APP_PHASE__ = "ready");
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
