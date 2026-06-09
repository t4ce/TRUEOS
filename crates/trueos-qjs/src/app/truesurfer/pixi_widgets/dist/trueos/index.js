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
    var Gi = Object.getOwnPropertyDescriptors;
    var Vn = Object.getOwnPropertySymbols;
    var vi = Object.prototype.hasOwnProperty, Li = Object.prototype.propertyIsEnumerable;
    var _n = function (t, e, n) { return e in t ? Jn(t, e, { enumerable: !0, configurable: !0, writable: !0, value: n }) : t[e] = n; }, ne = function (t, e) {
        var e_1, _a;
        for (var n in e || (e = {}))
            vi.call(e, n) && _n(t, n, e[n]);
        if (Vn)
            try {
                for (var _b = __values(Vn(e)), _c = _b.next(); !_c.done; _c = _b.next()) {
                    var n = _c.value;
                    Li.call(e, n) && _n(t, n, e[n]);
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
    }, ge = function (t, e) { return Ni(t, Gi(e)); };
    var Bi = function (t, e) { return function () { return (t && (e = t(t = 0)), e); }; };
    var Fi = function (t, e) { for (var n in e)
        Jn(t, n, { get: e[n], enumerable: !0 }); };
    var tt = function (t, e, n) { return _n(t, typeof e != "symbol" ? e + "" : e, n); };
    var ze = function (t, e, n) { return new Promise(function (r, i) { var o = function (a) { try {
        l(n.next(a));
    }
    catch (h) {
        i(h);
    } }, s = function (a) { try {
        l(n.throw(a));
    }
    catch (h) {
        i(h);
    } }, l = function (a) { return a.done ? r(a.value) : Promise.resolve(a.value).then(o, s); }; l((n = n.apply(t, e)).next()); }); };
    var gi = {};
    Fi(gi, { default: function () { return Go; } });
    var Go, bi = Bi(function () { Go = {}; });
    var Re = /** @class */ (function () {
        function Re(e, n) {
            if (e === void 0) { e = 0; }
            if (n === void 0) { n = 0; }
            tt(this, "x");
            tt(this, "y");
            this.x = Number(e) || 0, this.y = Number(n) || 0;
        }
        Re.prototype.set = function (e, n) {
            if (e === void 0) { e = 0; }
            if (n === void 0) { n = e; }
            this.x = Number(e) || 0, this.y = Number(n) || 0;
        };
        return Re;
    }()), bt = /** @class */ (function () {
        function bt(e, n, r, i) {
            if (e === void 0) { e = 0; }
            if (n === void 0) { n = 0; }
            if (r === void 0) { r = 0; }
            if (i === void 0) { i = 0; }
            tt(this, "x");
            tt(this, "y");
            tt(this, "width");
            tt(this, "height");
            this.x = Number(e) || 0, this.y = Number(n) || 0, this.width = Number(r) || 0, this.height = Number(i) || 0;
        }
        return bt;
    }()), wn = /** @class */ (function () {
        function wn() {
            tt(this, "parent");
            tt(this, "children");
            tt(this, "label");
            tt(this, "name");
            tt(this, "position");
            tt(this, "scale");
            tt(this, "pivot");
            tt(this, "visible");
            tt(this, "alpha");
            tt(this, "mask");
            tt(this, "rotation");
            tt(this, "zIndex");
            tt(this, "eventMode");
            tt(this, "cursor");
            tt(this, "hitArea");
            tt(this, "listeners");
            this.parent = null, this.position = new Re, this.scale = new Re(1, 1), this.pivot = new Re, this.visible = !0, this.alpha = 1, this.mask = null, this.rotation = 0, this.zIndex = 0, this.eventMode = null, this.cursor = null, this.hitArea = null, this.listeners = {};
        }
        Object.defineProperty(wn.prototype, "x", {
            get: function () { return this.position.x; },
            set: function (e) { this.position.x = Number(e) || 0; },
            enumerable: false,
            configurable: true
        });
        Object.defineProperty(wn.prototype, "y", {
            get: function () { return this.position.y; },
            set: function (e) { this.position.y = Number(e) || 0; },
            enumerable: false,
            configurable: true
        });
        wn.prototype.on = function (e, n) { return this; };
        wn.prototype.removeAllListeners = function (e) { return e == null ? this.listeners = {} : delete this.listeners[String(e)], this; };
        wn.prototype.removeFromParent = function () { var e; return (e = this.parent) == null || e.removeChild(this), this; };
        wn.prototype.destroy = function (e) { this.removeFromParent(), this.removeAllListeners(); };
        wn.prototype.toLocal = function (e) { var n = e || {}; return { x: (Number(n.x) || 0) - this.getGlobalX(), y: (Number(n.y) || 0) - this.getGlobalY() }; };
        wn.prototype.getGlobalPosition = function () { return { x: this.getGlobalX(), y: this.getGlobalY() }; };
        wn.prototype.getGlobalX = function () { return (this.parent ? this.parent.getGlobalX() : 0) + this.x; };
        wn.prototype.getGlobalY = function () { return (this.parent ? this.parent.getGlobalY() : 0) + this.y; };
        return wn;
    }()), wt = /** @class */ (function (_super) {
        __extends(wt, _super);
        function wt() {
            var _this = _super.call(this) || this;
            tt(_this, "children");
            tt(_this, "sortableChildren");
            _this.children = [], _this.sortableChildren = !1;
            return _this;
        }
        wt.prototype.addChild = function () {
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
        wt.prototype.addChildAt = function (n, r) { var o; (o = n.parent) == null || o.removeChild(n), n.parent = this; var i = Math.max(0, Math.min(Number(r) | 0, this.children.length)); return this.children.splice(i, 0, n), n; };
        wt.prototype.removeChild = function () {
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
        wt.prototype.removeChildren = function (n, r) {
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
        wt.prototype.setChildIndex = function (n, r) { var i = this.children.indexOf(n); if (i < 0)
            return; this.children.splice(i, 1); var o = Math.max(0, Math.min(Number(r) | 0, this.children.length)); this.children.splice(o, 0, n); };
        wt.prototype.getChildIndex = function (n) { return this.children.indexOf(n); };
        wt.prototype.getChildByLabel = function (n) { for (var r = 0; r < this.children.length; r += 1) {
            var i = this.children[r];
            if (i && i.label === n)
                return i;
        } return null; };
        return wt;
    }(wn)), _t = /** @class */ (function (_super) {
        __extends(_t, _super);
        function _t() {
            var _this = _super.call(this) || this;
            tt(_this, "commands");
            _this.commands = [];
            return _this;
        }
        _t.prototype.clear = function () { return this.commands.length = 0, this; };
        _t.prototype.rect = function (n, r, i, o) { return this.commands.push(["rect", n, r, i, o]), this; };
        _t.prototype.roundRect = function (n, r, i, o, s) {
            if (s === void 0) { s = 0; }
            return this.commands.push(["roundRect", n, r, i, o, s]), this;
        };
        _t.prototype.circle = function (n, r, i) { return this.commands.push(["circle", n, r, i]), this; };
        _t.prototype.ellipse = function (n, r, i, o) { return this.commands.push(["ellipse", n, r, i, o]), this; };
        _t.prototype.moveTo = function (n, r) { return this.commands.push(["moveTo", n, r]), this; };
        _t.prototype.lineTo = function (n, r) { return this.commands.push(["lineTo", n, r]), this; };
        _t.prototype.closePath = function () { return this.commands.push(["closePath"]), this; };
        _t.prototype.poly = function (n) { return this.commands.push(["poly", n]), this; };
        _t.prototype.fill = function (n) { return this.commands.push(["fill", n]), this; };
        _t.prototype.stroke = function (n) { return this.commands.push(["stroke", n]), this; };
        _t.prototype.image = function (n, r, i, o, s) { return this.commands.push(["image", n, r, i, o, s]), this; };
        _t.prototype.svg = function (n) { return this.commands.push(["svg", n]), this; };
        return _t;
    }(wt)), qt = /** @class */ (function (_super) {
        __extends(qt, _super);
        function qt(n) {
            if (n === void 0) { n = ""; }
            var r, i;
            var _this = _super.call(this) || this;
            tt(_this, "_text");
            tt(_this, "_style");
            tt(_this, "_resolution");
            _this._text = "", _this._style = {}, _this._resolution = 1, typeof n == "string" ? _this._text = n : (_this._text = String((r = n.text) != null ? r : ""), _this._style = ne({}, (i = n.style) != null ? i : {}));
            return _this;
        }
        Object.defineProperty(qt.prototype, "text", {
            get: function () { return this._text; },
            set: function (n) { this._text = String(n != null ? n : ""); },
            enumerable: false,
            configurable: true
        });
        Object.defineProperty(qt.prototype, "style", {
            get: function () { return this._style; },
            set: function (n) { this._style = n != null ? n : {}; },
            enumerable: false,
            configurable: true
        });
        Object.defineProperty(qt.prototype, "resolution", {
            get: function () { return this._resolution; },
            set: function (n) { this._resolution = Math.max(1, Number(n) || 1); },
            enumerable: false,
            configurable: true
        });
        Object.defineProperty(qt.prototype, "width", {
            get: function () { var n = Number(this._style.fontSize) || 16; return this._text.length * n * .58; },
            enumerable: false,
            configurable: true
        });
        Object.defineProperty(qt.prototype, "height", {
            get: function () { var n = Number(this._style.fontSize) || 16; return Number(this._style.lineHeight) || n * 1.25; },
            enumerable: false,
            configurable: true
        });
        qt.prototype.setSize = function (n, r) { return this; };
        return qt;
    }(wt)), Te = /** @class */ (function () {
        function Te(e) {
            if (e === void 0) { e = {}; }
            tt(this, "options");
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
            tt(_this, "geometry");
            tt(_this, "shader");
            _this.geometry = (r = n.geometry) != null ? r : new Te, _this.shader = (i = n.shader) != null ? i : new De;
            return _this;
        }
        return je;
    }(wt)), Ve = /** @class */ (function () {
        function Ve(e) {
            if (e === void 0) { e = {}; }
            tt(this, "options");
            this.options = e;
        }
        return Ve;
    }()), Tn = { VERTEX: 1, COPY_DST: 2 }, De = /** @class */ (function () {
        function De(e) {
            if (e === void 0) { e = {}; }
            tt(this, "options");
            this.options = e;
        }
        return De;
    }());
    var Zn = "", Qn = "", qn = "", Je = /** @class */ (function () {
        function Je() {
            var _this = this;
            tt(this, "stage");
            tt(this, "screen");
            tt(this, "canvas");
            tt(this, "renderer");
            tt(this, "ticker");
            var e = Math.max(1, Number(globalThis.innerWidth || 1920) | 0), n = Math.max(1, Number(globalThis.innerHeight || 1080) | 0);
            this.stage = new wt, this.screen = new bt(0, 0, e, n), this.canvas = document.createElement("canvas"), this.ticker = { stop: function () { }, add: function () { }, remove: function () { } }, this.renderer = { width: e, height: n, screen: this.screen, render: function (r) { return r; }, resize: function (r, i) { var o = Math.max(1, Number(r || e) | 0), s = Math.max(1, Number(i || n) | 0); _this.renderer.width = o, _this.renderer.height = s, _this.screen.width = o, _this.screen.height = s; } };
        }
        Je.prototype.init = function (e) { return ze(this, null, function () { return __generator(this, function (_a) {
            return [2 /*return*/];
        }); }); };
        return Je;
    }());
    var be = { fontFamily: "system-ui, -apple-system, Segoe UI, Arial", fontSize: 16, background: 16777215, text: 1118481, mutedText: 6710886, boxBorder: 14540253, hr: 13421772, control: { border: 0, focusBorder: 3900150, background: 16777215, accent: 3900150, radius: 0, button: { fill: 15921906, hoverFill: 15395562, activeFill: 14737632, border: 6710886, text: 1118481, radius: 0 }, progress: { border: 10066329, background: 16777215, fill: 6990335 }, table: { border: 10066329, cellBorder: 11579568, headerFill: 16250871 } } };
    var Me = 24, xt = 1;
    function jt(t) { var i, o; var e = t.wrapWidth, n = (i = t.wordWrap) != null ? i : e != null, r = (o = t.wordWrapWidth) != null ? o : e == null ? void 0 : Math.max(0, Math.ceil(e) + Me); return new qt({ text: t.text, style: { fontFamily: t.fontFamily, fontSize: t.fontSize, fill: t.fill, fontWeight: t.bold ? "700" : "400", wordWrap: n, wordWrapWidth: r } }); }
    function Mn(t, e) { var n = t.children; if (!Array.isArray(n))
        return null; for (var r = 0; r < n.length; r += 1) {
        var i = n[r];
        if (i && i.label === e)
            return i;
    } return null; }
    function xe(t, e) { var n = Mn(t, e); if (n)
        return n; var r = new wt; return r.label = e, t.addChild(r), r; }
    function Pt(t, e) { var n = Mn(t, e); if (n)
        return n; var r = new _t; return r.label = e, t.addChild(r), r; }
    function Ct(t, e, n) { var r = Mn(t, e); if (r)
        return r; var i = new qt({ text: "" }); return i.label = e, n == null || n(i), t.addChild(i), i; }
    function kt(t) { t.clear(), t.removeAllListeners(), t.hitArea = null; }
    function Ut(t) { t.removeAllListeners(); }
    function ce(t, e, n) {
        var r = String(t != null ? t : ""), i = [], o = 0;
        for (var s = 0; s <= r.length; s++) {
            if (!(s === r.length || r[s] === "\n"))
                continue;
            var a = o, h = s;
            if (a === h)
                i.push({ start: a, end: h, text: "" });
            else {
                var f = a, d = -1;
                for (var b = f; b < h; b++) {
                    r[b] === " " && (d = b);
                    var E = r.slice(f, b + 1);
                    if (n(E) <= e || b === f)
                        continue;
                    var p = d >= f ? d + 1 : b;
                    p <= f && (p = Math.min(h, f + 1)), i.push({ start: f, end: p, text: r.slice(f, p) }), f = p, b = f - 1, d = -1;
                }
                f <= h && i.push({ start: f, end: h, text: r.slice(f, h) });
            }
            o = s + 1;
        }
        return i;
    }
    function ue(t, e) { return e <= 0 ? [] : t.length <= e ? t : t.slice(0, e); }
    function Ee(t) { var e = t.fullText, n = t.lines, r = t.localX, i = t.localY, o = t.lineHeight, s = t.measure; if (n.length === 0)
        return 0; var l = Math.max(0, r), a = Math.max(0, i), h = Math.max(1, o), f = Math.max(0, Math.min(n.length - 1, Math.floor(a / h))), d = n[f], b = d.start, c = Number.POSITIVE_INFINITY; for (var E = d.start; E <= d.end; E++) {
        var p = s(e.slice(d.start, E)), y = Math.abs(p - l);
        y < c && (c = y, b = E);
    } return b; }
    function tr(t) { var E, p, y, k; var e = t.node, n = t.graphics, r = t.w, i = t.h, o = t.theme, s = Math.max(0, Math.round(r)), l = Math.max(0, Math.round(i)); n.rect(.5, .5, Math.max(0, s - 1), Math.max(0, l - 1)), n.fill(o.control.progress.background), n.stroke({ width: 1, color: o.control.progress.border }); var a = Number((p = (E = e.attrs) == null ? void 0 : E.value) != null ? p : "0"), h = Number((k = (y = e.attrs) == null ? void 0 : y.max) != null ? k : "1"), f = h > 0 ? Math.max(0, Math.min(1, a / h)) : 0, d = 3, b = Math.max(0, s - d * 2), c = Math.max(0, l - d * 2); n.rect(d, d, Math.max(0, b * f), c), n.fill(o.control.progress.fill); }
    function er(t, e) { t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0), t.setHeight(14), t.setMinWidth(240); }
    function ye(t, e, n) { var l; var r = t.get(e); if (r)
        return r; var i = Number((l = n == null ? void 0 : n.value) != null ? l : "0"), o = Number.isFinite(i) ? i : 0, s = { value: Math.max(0, Math.min(1, o)) }; return t.set(e, s), s; }
    function nr(t, e) { t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0), t.setHeight(14), t.setMinWidth(240); }
    function rr(t) { var e = t.node, n = t.Yoga, r = t.measurer, i = n.Node.create(); return i.setPadding(n.EDGE_LEFT, 0), i.setPadding(n.EDGE_RIGHT, 0), i.setPadding(n.EDGE_TOP, 0), i.setPadding(n.EDGE_BOTTOM, 0), i.setMargin(n.EDGE_RIGHT, 6), i.setMeasureFunc(function () { var o = r.measure("100"); return { width: o.width, height: o.height }; }), { yogaNode: i, buildBox: function () { return ({ kind: "block", key: e.key, tagName: e.tagName, attrs: e.attrs, x: i.getComputedLeft(), y: i.getComputedTop(), width: i.getComputedWidth(), height: i.getComputedHeight(), children: [] }); } }; }
    function ir(t) { var h, f; var e = t.node, n = t.container, r = t.theme, i = t.sliderStates, o = (h = e.attrs) == null ? void 0 : h["data-slider-key"], s = null; if (o) {
        var d = i.get(o);
        if (d)
            s = d;
        else {
            var b = (f = e.attrs) == null ? void 0 : f["data-slider-init"];
            s = ye(i, o, b != null ? { value: String(b) } : void 0);
        }
    } var l = s ? Math.round(s.value * 100) : 0, a = Ct(n, "__pct", function (d) { d.style = { fontFamily: r.fontFamily, fontSize: r.fontSize, fill: r.text, fontWeight: "400", wordWrap: !1 }; }); a.text = String(l), a.position.set(0, xt); }
    function Ze(t) { var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.absX, l = t.absY, a = t.theme, h = t.sliderStates, f = t.sliderBounds, d = t.sliderDrags, b = t.requestPaint, c = e.key, E = c ? ye(h, c, e.attrs) : null, p = Math.max(0, Math.round(i)), y = Math.max(0, Math.round(o)), k = 3; c && f.set(c, { x: s, y: l, w: p, h: y, innerPad: k }), r.rect(.5, .5, Math.max(0, p - 1), Math.max(0, y - 1)), r.fill(a.control.progress.background), r.stroke({ width: 1, color: a.control.progress.border }); var M = E ? Math.max(0, Math.min(1, E.value)) : 0, _ = Math.max(0, p - k * 2), B = Math.max(0, y - k * 2); r.rect(k, k, Math.max(0, _ * M), B), r.fill(a.control.progress.fill); var R = k + _ * M, H = B / 2; r.moveTo(R, k - H), r.lineTo(R, k + B + H), r.stroke({ width: 2, color: a.text }), c && (Ut(n), n.eventMode = "static", n.cursor = "pointer", n.hitArea = new bt(0, 0, Math.max(0, p), Math.max(0, y)), n.on("pointerdown", function (D) {
        var e_5, _a;
        var v, et, q, nt, Q, j;
        if ((D == null ? void 0 : D.button) === 2)
            return;
        var w = t.getPointerId ? t.getPointerId(D) : Number((q = (et = D == null ? void 0 : D.pointerId) != null ? et : (v = D == null ? void 0 : D.data) == null ? void 0 : v.pointerId) != null ? q : 0);
        if (w <= 0)
            return;
        try {
            for (var _b = __values(d.entries()), _c = _b.next(); !_c.done; _c = _b.next()) {
                var _d = __read(_c.value, 2), G = _d[0], U = _d[1];
                U.key === c && G !== w && d.delete(G);
            }
        }
        catch (e_5_1) { e_5 = { error: e_5_1 }; }
        finally {
            try {
                if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
            }
            finally { if (e_5) throw e_5.error; }
        }
        d.set(w, { key: c });
        var T = f.get(c), $ = (Q = (nt = D.global) == null ? void 0 : nt.x) != null ? Q : 0, C = T ? $ - T.x : 0, F = T ? Math.max(1, T.w - T.innerPad * 2) : 1, m = (C - ((j = T == null ? void 0 : T.innerPad) != null ? j : 0)) / F, P = ye(h, c, e.attrs);
        P.value = Math.max(0, Math.min(1, m)), b == null || b();
    })); }
    function or(t) { var B; var e = t.node, n = t.container, r = t.w, i = t.h, o = t.theme, s = t.detailsOpen, l = t.requestRerender, a = (B = e.attrs) == null ? void 0 : B["data-details-key"], h = e.attrs ? Object.prototype.hasOwnProperty.call(e.attrs, "data-details-open") : !1, f = a && s.has(a) ? s.get(a) === !0 : h, d = function (R) { var w; if (!a || (R == null ? void 0 : R.button) === 2)
        return; var D = !(s.has(a) ? s.get(a) === !0 : h); s.set(a, D), l == null || l(), (w = R == null ? void 0 : R.stopPropagation) == null || w.call(R); }, b = 16, c = Pt(n, "__arrow"); kt(c); var E = 2, p = 3, y = p, k = p, M = b - p, _ = b - p; f ? (c.moveTo(y, k), c.lineTo((y + M) / 2, _), c.lineTo(M, k)) : (c.moveTo(y, k), c.lineTo(M, (k + _) / 2), c.lineTo(y, _)), c.stroke({ width: E, color: o.text }), c.position.set(4, Math.max(0, (i - b) / 2)), c.eventMode = "static", c.cursor = "pointer", c.hitArea = new bt(0, 0, b + 8, b + 8), c.on("pointerdown", d), a && (Ut(n), n.eventMode = "static", n.cursor = "pointer", n.hitArea = new bt(0, 0, Math.max(0, r), Math.max(0, i)), n.on("pointerdown", d)); }
    function sr(t, e) { t.setFlexDirection(e.FLEX_DIRECTION_ROW), t.setAlignItems(e.ALIGN_CENTER), t.setJustifyContent(e.JUSTIFY_SPACE_BETWEEN), t.setPadding(e.EDGE_TOP, 6), t.setPadding(e.EDGE_BOTTOM, 6), t.setPadding(e.EDGE_LEFT, 26), t.setPadding(e.EDGE_RIGHT, 12), t.setMinHeight(36); }
    function ar(t, e) { t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0); }
    function lr(t, e) { var i, o, s; if (!t || t.tagName !== "details" || !t.key)
        return (i = t == null ? void 0 : t.children) != null ? i : []; var n = t.attrs ? Object.prototype.hasOwnProperty.call(t.attrs, "open") : !1; return (e.has(t.key) ? e.get(t.key) === !0 : n) ? (o = t.children) != null ? o : [] : ((s = t.children) != null ? s : []).filter(function (l) { return l && l.kind === "block" && l.tagName === "summary"; }); }
    function cr(t) { var e = t.graphics, n = t.w, r = t.theme; e.rect(0, 0, Math.round(n), 1), e.fill(r.hr); }
    function ur(t, e) { t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0), t.setMargin(e.EDGE_TOP, 2), t.setMargin(e.EDGE_BOTTOM, 2), t.setHeight(1); }
    function dr(t) { var y, k; var e = t.container, n = t.graphics, r = t.w, i = t.h, o = t.label, s = t.theme, l = t.registerHoverHandlers, a = function (M) { n.clear(); var _ = 1, B = _ / 2; s.control.button.radius > 0 ? n.roundRect(B, B, Math.max(0, r - _), Math.max(0, i - _), s.control.button.radius) : n.rect(B, B, Math.max(0, r - _), Math.max(0, i - _)), n.fill(M), n.stroke({ width: _, color: s.control.button.border }); }; a(s.control.button.fill); var h = Ct(e, "__label", function (M) { M.style = { fontFamily: s.fontFamily, fontSize: s.fontSize, fill: s.control.button.text, fontWeight: "400", wordWrap: !1, wordWrapWidth: 0 }; }), f = String(o != null ? o : "").trim(); h.text = f, h.visible = f.length > 0, h.style = ge(ne({}, h.style), { fontFamily: s.fontFamily, fontSize: s.fontSize, fill: s.control.button.text, wordWrap: !1, wordWrapWidth: Math.max(0, Math.ceil(r - 16)) }); var d = Number((y = h.width) != null ? y : 0), b = Number((k = h.height) != null ? k : 0), c = s.fontSize * 1.25; h.position.set(d > 0 ? Math.max(8, Math.floor((r - d) / 2)) : 8, Math.max(0, Math.floor((i - (b > 0 ? b : c)) / 2)) + xt); var E = function () { return a(s.control.button.hoverFill); }, p = function () { return a(s.control.button.fill); }; l == null || l({ over: E, out: p }), Ut(e), e.eventMode = "static", e.cursor = "pointer", e.on("pointerover", E), e.on("pointerout", p), e.on("pointerdown", function () { return a(s.control.button.activeFill); }), e.on("pointerup", function () { return a(s.control.button.hoverFill); }); }
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
    function _r(_a) {
        var _b = _a === void 0 ? {} : _a, _c = _b.ring, t = _c === void 0 ? 34 : _c, _d = _b.core, e = _d === void 0 ? 14 : _d, _f = _b.hueA, n = _f === void 0 ? "#00e5ff" : _f, _g = _b.hueB, r = _g === void 0 ? "#ff2bd6" : _g;
        var i = Math.max(0, t - 10), o = Math.max(0, e * .35);
        return "\n<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 100 100\">\n  <rect width=\"100\" height=\"100\" fill=\"#ffffff\"/>\n  <rect width=\"100\" height=\"100\" fill=\"".concat(n, "\" opacity=\"0.08\"/>\n\n  <circle cx=\"50\" cy=\"50\" r=\"").concat(t, "\" fill=\"none\" stroke=\"").concat(r, "\" stroke-width=\"4\" opacity=\"0.95\"/>\n  <circle cx=\"50\" cy=\"50\" r=\"").concat(i, "\" fill=\"none\" stroke=\"").concat(n, "\" stroke-width=\"1\" opacity=\"0.35\"/>\n\n  <circle cx=\"50\" cy=\"50\" r=\"").concat(e, "\" fill=\"").concat(n, "\" opacity=\"0.9\"/>\n  <circle cx=\"43\" cy=\"43\" r=\"").concat(o, "\" fill=\"#ffffff\" opacity=\"0.55\"/>\n\n  <path d=\"M50 16 L52 22 L58 24 L52 26 L50 32 L48 26 L42 24 L48 22 Z\" fill=\"#ffffff\" opacity=\"0.85\"/>\n  <path d=\"M82 52 L85 56 L90 57 L85 58 L82 62 L79 58 L74 57 L79 56 Z\" fill=\"#ffffff\" opacity=\"0.70\"/>\n  <path d=\"M20 70 L22 74 L27 75 L22 76 L20 80 L18 76 L13 75 L18 74 Z\" fill=\"#ffffff\" opacity=\"0.65\"/>\n</svg>\n");
    }
    var wr = new Map;
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
    function $i(t, e) { var n = globalThis, r = String(t != null ? t : "").trim(); if (!r || !e)
        return; var i = n.__TRUEOS_READY_IMAGE_TEXTURE_CACHE__; (!i || typeof i.get != "function" || typeof i.set != "function") && (i = new Map, n.__TRUEOS_READY_IMAGE_TEXTURE_CACHE__ = i), i.set(r, e); }
    function kr(t, e) { var r, i, o, s, l; var n = Math.max(0, Number((r = e == null ? void 0 : e.texId) != null ? r : 0) | 0); if (n <= 0)
        throw new Error("image texture missing"); t.state = "ready", t.texId = n, t.width = Math.max(0, Number((o = (i = e == null ? void 0 : e.pixelWidth) != null ? i : e == null ? void 0 : e.width) != null ? o : 0) | 0), t.height = Math.max(0, Number((l = (s = e == null ? void 0 : e.pixelHeight) != null ? s : e == null ? void 0 : e.height) != null ? l : 0) | 0); }
    function Hi(t, e) { var n = Wi(t) || Er(t); return !n || typeof n.then == "function" ? !1 : (kr(e, n), $i(t, n), !0); }
    function Tr(t, e) { var n = String(t != null ? t : "").trim(); if (!n)
        return null; var r = wr.get(n); if (r) {
        if (Ae() && r.state === "loading")
            try {
                Hi(n, r);
            }
            catch (l) {
                r.state = "error";
            }
        return r;
    } if (Ae())
        return null; var i = { state: "loading", texId: 0, width: 0, height: 0 }; wr.set(n, i); var o = function (l) { kr(i, l), Ae() || e == null || e(); }, s = function () { i.state = "error", Ae() || e == null || e(); }; try {
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
        var h = t.indexOf(">", a + s.length);
        r = h < 0 ? t.length : h + 1;
    } return n; }
    function Sr(t) { var B, R, H, D; var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.theme, l = t.requestRerender, a = (R = (B = e.attrs) == null ? void 0 : B.alt) != null ? R : "", h = (D = (H = e.attrs) == null ? void 0 : H.src) != null ? D : "", f = h.trim().length > 0, d = a.trim().length > 0 ? a : h.trim().length > 0 ? h : "img", b = r.image, c = f ? Tr(h, l) : null; if ((c == null ? void 0 : c.state) === "ready" && c.texId > 0 && typeof b == "function") {
        b.call(r, c.texId, 0, 0, Math.max(0, i), Math.max(0, o));
        var w = n.getChildByLabel ? n.getChildByLabel("__label") : n.children.find(function (T) { return (T == null ? void 0 : T.label) === "__label"; });
        w && (w.visible = !1);
        return;
    } var E = f ? Ui(h) : null, p = Xi(E != null ? E : f ? yr(i, o) : _r({ ring: 34, core: 14 })), y = Pt(n, "__svg"), k = Tr(Yi(p), l); if ((k == null ? void 0 : k.state) === "ready" && k.texId > 0 && typeof y.image == "function") {
        var w = "texture:".concat(k.texId, ":").concat(Math.round(i), "x").concat(Math.round(o));
        if (y.__key !== w && (kt(y), y.image(k.texId, 0, 0, Math.max(0, i), Math.max(0, o)), y.__key = w), y.scale.set(1), y.position.set(0, 0), !f) {
            var T = n.getChildByLabel ? n.getChildByLabel("__label") : n.children.find(function ($) { return ($ == null ? void 0 : $.label) === "__label"; });
            T && (T.visible = !1);
            return;
        }
        if (d.trim().length > 0) {
            var T = Ct(n, "__label", function ($) { $.style = { fontFamily: s.fontFamily, fontSize: s.fontSize, fill: s.mutedText, fontWeight: "400", wordWrap: !0, wordWrapWidth: 0 }; });
            T.text = d, T.style.wordWrapWidth = Math.max(0, Math.ceil(i - 16) + Me), T.position.set(8, 8 + xt), T.visible = !0;
        }
        return;
    }
    else
        kt(y); var M = y.svg; if (0 && y.__key !== w)
        try { }
        catch ($) { } r.rect(.5, .5, Math.max(0, i - 1), Math.max(0, o - 1)), r.fill(16185078), r.stroke({ width: 1, color: s.control.border }); var _ = Ct(n, "__label", function (w) { w.style = { fontFamily: s.fontFamily, fontSize: s.fontSize, fill: s.mutedText, fontWeight: "400", wordWrap: !0, wordWrapWidth: 0 }; }); _.text = d, _.style.wordWrapWidth = Math.max(0, Math.ceil(i - 16) + Me), _.position.set(8, 8 + xt); }
    function Ir(t, e, n) { var h, f, d, b; t.setPadding(n.EDGE_LEFT, 0), t.setPadding(n.EDGE_RIGHT, 0), t.setPadding(n.EDGE_TOP, 0), t.setPadding(n.EDGE_BOTTOM, 0); var r = Number((f = (h = e.attrs) == null ? void 0 : h.width) != null ? f : "0"), i = Number((b = (d = e.attrs) == null ? void 0 : d.height) != null ? b : "0"), o = Number.isFinite(r) && r > 0, s = Number.isFinite(i) && i > 0, l = o ? r : 240, a = s ? i : 140; (o || s) && (t.setAlignSelf(n.ALIGN_FLEX_START), t.setFlexGrow(0), t.setFlexShrink(0)), t.setWidth(l), t.setHeight(a), t.setMinWidth(120), t.setMinHeight(80); }
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
        var h = t.indexOf(">", a + s.length);
        r = h < 0 ? t.length : h + 1;
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
        return null; var h = Number(a[0]), f = Number(a[1]), d = Number(a[2]), b = Number(a[3]); return ![h, f, d, b].every(function (c) { return Number.isFinite(c); }) || d <= 0 || b <= 0 ? null : { minX: h, minY: f, w: d, h: b }; }
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
        return { x: 0, y: 0, w: r, h: i }; var s = r / o.w, l = i / o.h, a = Math.min(s, l), h = Math.max(0, o.w * a), f = Math.max(0, o.h * a); return { x: Math.max(0, (r - h) / 2), y: Math.max(0, (i - f) / 2), w: h, h: f }; }
    function Ar(t, e, n) { var h, f, d, b; t.setPadding(n.EDGE_LEFT, 0), t.setPadding(n.EDGE_RIGHT, 0), t.setPadding(n.EDGE_TOP, 0), t.setPadding(n.EDGE_BOTTOM, 0); var r = Number((f = (h = e.attrs) == null ? void 0 : h.width) != null ? f : "0"), i = Number((b = (d = e.attrs) == null ? void 0 : d.height) != null ? b : "0"), o = Number.isFinite(r) && r > 0, s = Number.isFinite(i) && i > 0, l = o ? r : 300, a = s ? i : 150; (o || s) && (t.setAlignSelf(n.ALIGN_FLEX_START), t.setFlexGrow(0), t.setFlexShrink(0)), t.setWidth(l), t.setHeight(a), t.setMinWidth(Math.min(120, l)), t.setMinHeight(Math.min(80, a)); }
    function Nr(t) { var e = t.svgMarkup, n = t.container, r = t.w, i = t.h, o = t.requestRerender, s = Vi(e), l = Pt(n, "__svg"), a = l.__svgString, h = l.__w, f = l.__h, d = a !== s, b = Zi(s, o); if (l.scale.set(1), l.position.set(0, 0), (b == null ? void 0 : b.state) === "ready" && b.texId > 0 && typeof l.image == "function") {
        if (d || h !== r || f !== i || l.__texId !== b.texId) {
            var E = Qi(s, r, i);
            kt(l), l.image(b.texId, E.x, E.y, E.w, E.h), l.__svgString = s, l.__w = r, l.__h = i, l.__texId = b.texId;
        }
        return;
    } kt(l); return; if (typeof c == "function") {
        if (d || h !== r || f !== i) {
            kt(l);
            var p = void 0;
            try {
                p = c.call(l, s);
            }
            catch (y) {
                p = null;
            }
            p && typeof p.then == "function" && p.then(function () { return o == null ? void 0 : o(); }).catch(function () { }), l.__svgString = s, l.__w = r, l.__h = i;
        }
        var E = Dr(s);
        if (E) {
            var p = r / E.w, y = i / E.h, k = Math.min(p, y), M = E.w * k, _ = E.h * k;
            l.scale.set(k), l.position.set(-E.minX * k + (r - M) / 2, -E.minY * k + (i - _) / 2);
        }
        return;
    } }
    function Gr(t, e, n) { var h, f, d, b; t.setPadding(n.EDGE_LEFT, 0), t.setPadding(n.EDGE_RIGHT, 0), t.setPadding(n.EDGE_TOP, 0), t.setPadding(n.EDGE_BOTTOM, 0); var r = Number((f = (h = e.attrs) == null ? void 0 : h.width) != null ? f : "0"), i = Number((b = (d = e.attrs) == null ? void 0 : d.height) != null ? b : "0"), o = Number.isFinite(r) && r > 0, s = Number.isFinite(i) && i > 0, l = o ? r : 300, a = s ? i : 150; (o || s) && (t.setAlignSelf(n.ALIGN_FLEX_START), t.setFlexGrow(0), t.setFlexShrink(0)), t.setWidth(l), t.setHeight(a), t.setMinWidth(Math.min(120, l)), t.setMinHeight(Math.min(80, a)); }
    function vr(t) { var e = t.graphics, n = t.container, r = t.w, i = t.h, o = t.theme, s = 1, l = s / 2; e.rect(l, l, Math.max(0, r - s), Math.max(0, i - s)), e.fill(16777215), e.stroke({ width: s, color: o.control.border, alignment: 0 }), e.moveTo(6, i - 6), e.lineTo(r - 6, 6), e.stroke({ width: 1, color: 0, alpha: .1 }); var a = jt({ text: "canvas", fontFamily: o.fontFamily, fontSize: Math.max(10, Math.floor(o.fontSize * .85)), fill: o.mutedText, wordWrap: !1 }); a.position.set(8, 8 + xt), n.addChild(a); }
    function Lr(t, e, n) { var f, d, b, c, E, p; var r = String((d = (f = e.attrs) == null ? void 0 : f["data-root"]) != null ? d : "") === "1"; if (t.setFlexDirection(n.FLEX_DIRECTION_COLUMN), t.setAlignItems(n.ALIGN_STRETCH), r) {
        t.setPadding(n.EDGE_LEFT, 0), t.setPadding(n.EDGE_RIGHT, 0), t.setPadding(n.EDGE_TOP, 0), t.setPadding(n.EDGE_BOTTOM, 0), t.setAlignSelf(n.ALIGN_STRETCH), t.setFlexGrow(1), t.setFlexShrink(1), t.setMinWidth(0), t.setMinHeight(0);
        return;
    } t.setPadding(n.EDGE_LEFT, 8), t.setPadding(n.EDGE_RIGHT, 8), t.setPadding(n.EDGE_BOTTOM, 8), t.setPadding(n.EDGE_TOP, 34); var i = Number((c = (b = e.attrs) == null ? void 0 : b.width) != null ? c : "0"), o = Number((p = (E = e.attrs) == null ? void 0 : E.height) != null ? p : "0"), s = Number.isFinite(i) && i > 0, l = Number.isFinite(o) && o > 0, a = s ? i : 420, h = l ? o : 240; (s || l) && (t.setAlignSelf(n.ALIGN_FLEX_START), t.setFlexGrow(0), t.setFlexShrink(0)), t.setWidth(a), t.setHeight(h), t.setMinWidth(Math.min(200, a)), t.setMinHeight(Math.min(160, h)); }
    function Br(t) { var c, E, p, y; var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.theme; if (String((E = (c = e.attrs) == null ? void 0 : c["data-root"]) != null ? E : "") === "1")
        return; var a = 1, h = a / 2; r.rect(h, h, Math.max(0, i - a), Math.max(0, o - a)), r.fill(16777215), r.stroke({ width: a, color: s.control.border, alignment: 0 }), r.rect(h, h, Math.max(0, i - a), 26), r.fill({ color: 0, alpha: .04 }); var d = String((y = (p = e.attrs) == null ? void 0 : p.srcdoc) != null ? y : "").trim().length > 0 ? "srcdoc" : "empty", b = Ct(n, "__title", function (k) { k.style = { fontFamily: s.fontFamily, fontSize: Math.max(10, Math.floor(s.fontSize * .85)), fill: s.mutedText, fontWeight: "400", wordWrap: !1 }; }); b.text = "iframe (".concat(d, ")"), b.position.set(8, 6 + xt), n.eventMode = "static", n.cursor = "default", n.hitArea = new bt(0, 0, Math.max(0, i), Math.max(0, o)); }
    function Fr(t, e, n) { var i, o; var r = ((o = (i = e.attrs) == null ? void 0 : i.type) != null ? o : "text").toLowerCase(); r === "checkbox" || r === "radio" ? (t.setPadding(n.EDGE_LEFT, 0), t.setPadding(n.EDGE_RIGHT, 0), t.setPadding(n.EDGE_TOP, 0), t.setPadding(n.EDGE_BOTTOM, 0), t.setWidth(16), t.setHeight(16), t.setMinWidth(16), t.setMargin(n.EDGE_RIGHT, 6)) : (t.setPadding(n.EDGE_TOP, 6), t.setPadding(n.EDGE_BOTTOM, 6), t.setHeight(36), t.setMinHeight(36), t.setMinWidth(220)); }
    function Wr(t) {
        var e_6, _a, e_7, _b;
        var C, F, m, P, v, et, q, nt, Q, j;
        var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.absX, l = t.absY, a = t.theme, h = t.textMeasure, f = t.uiState, d = t.getOrInitInputState, b = t.clamp, c = t.radioGroups, E = t.textDrags, p = t.requestPaint, y = ((F = (C = e.attrs) == null ? void 0 : C.type) != null ? F : "text").toLowerCase(), k = e.key, M = k ? d(k, e.attrs) : void 0, _ = (m = t.showCaret) != null ? m : !1, B = (P = t.caretPointerId) != null ? P : null, R = t.focusColor, H = t.getCursorColor;
        n.sortableChildren = !0;
        try {
            for (var _c = __values(n.children), _d = _c.next(); !_d.done; _d = _c.next()) {
                var G = _d.value;
                var U = G.label;
                U && (U.startsWith("__sel:") || U === "__caret") && (G.visible = !1);
            }
        }
        catch (e_6_1) { e_6 = { error: e_6_1 }; }
        finally {
            try {
                if (_d && !_d.done && (_a = _c.return)) _a.call(_c);
            }
            finally { if (e_6) throw e_6.error; }
        }
        var D = 8, w = 6 + xt, T = 5, $ = a.fontSize * 1.25;
        if (y === "checkbox")
            r.rect(.5, .5, Math.max(0, i - 1), Math.max(0, o - 1)), r.fill(a.control.background), r.stroke({ width: 1, color: a.control.border }), M != null && M.indeterminate ? (r.moveTo(4, 4), r.lineTo(Math.max(4, i - 4), Math.max(4, o - 4)), r.stroke({ width: 2, color: a.control.accent }), r.moveTo(Math.max(4, i - 4), 4), r.lineTo(4, Math.max(4, o - 4)), r.stroke({ width: 2, color: a.control.accent })) : M != null && M.checked && (r.rect(3, 3, Math.max(0, i - 3 * 2), Math.max(0, o - 3 * 2)), r.fill(a.control.accent));
        else if (y === "radio") {
            {
                var g = Math.max(0, Math.min(i, o) / 2 - .5);
                r.circle(i / 2, o / 2, g), r.fill(a.control.background), r.stroke({ width: 1, color: a.control.border });
            }
            if (M != null && M.checked) {
                var G = Math.max(0, Math.min(i, o) / 2 - 4.5);
                r.circle(i / 2, o / 2, G), r.fill(a.control.accent);
            }
        }
        else {
            var G = R != null ? 2 : 1, U = G / 2;
            a.control.radius > 0 ? r.roundRect(U, U, Math.max(0, i - G), Math.max(0, o - G), a.control.radius) : r.rect(U, U, Math.max(0, i - G), Math.max(0, o - G)), r.fill(a.control.background), r.stroke({ width: G, color: R != null ? R : a.control.border });
            var g = y === "password" ? "\u2022".repeat(((v = M == null ? void 0 : M.value) != null ? v : "").length) : (et = M == null ? void 0 : M.value) != null ? et : "", S = Math.max(0, i - D * 2);
            k && f.fieldBounds.set(k, { x: s, y: l, w: i, h: o, innerLeft: D, innerTop: w, innerWidth: S, maxLines: T, isPassword: y === "password" });
            var X = ce(g, S, h), x = ue(X, T), I = x.length > 0 ? x[x.length - 1].end : 0;
            if (k && M && typeof M.value == "string") {
                var L = M.selections;
                if (L && L.size > 0)
                    try {
                        for (var _f = __values(L.entries()), _g = _f.next(); !_g.done; _g = _f.next()) {
                            var _h = __read(_g.value, 2), K = _h[0], Y = _h[1];
                            var N = b((q = Y.start) != null ? q : 0, 0, g.length), z = b((nt = Y.end) != null ? nt : N, 0, g.length), V = b(Math.min(N, z), 0, I), J = b(Math.max(N, z), 0, I);
                            if (V === J)
                                continue;
                            var rt = Pt(n, "__sel:".concat(K));
                            kt(rt), rt.zIndex = 0, rt.visible = !0;
                            for (var st = 0; st < x.length; st++) {
                                var mt = x[st], Tt = Math.max(V, mt.start), Mt = Math.min(J, mt.end);
                                if (Tt >= Mt)
                                    continue;
                                var Gt = D + h(g.slice(mt.start, Tt)), vt = h(g.slice(Tt, Mt));
                                rt.rect(Gt, w + st * $, vt, $);
                            }
                            rt.fill({ color: H(K), alpha: .22 });
                        }
                    }
                    catch (e_7_1) { e_7 = { error: e_7_1 }; }
                    finally {
                        try {
                            if (_g && !_g.done && (_b = _f.return)) _b.call(_f);
                        }
                        finally { if (e_7) throw e_7.error; }
                    }
                if (_ && B != null) {
                    var K = (Q = M.selections) == null ? void 0 : Q.get(B), Y = K ? K.end : 0, N = b(Y, 0, I), z = Math.max(0, x.length - 1);
                    for (var st = 0; st < x.length; st++) {
                        var mt = x[st];
                        if (N >= mt.start && N <= mt.end) {
                            z = st;
                            break;
                        }
                    }
                    var V = (j = x[z]) != null ? j : { start: 0, end: 0, text: "" }, J = D + h(g.slice(V.start, N)), rt = Pt(n, "__caret");
                    kt(rt), rt.zIndex = 2, rt.visible = !0, rt.moveTo(J, w + z * $), rt.lineTo(J, w + z * $ + $), rt.stroke({ width: 1, color: R != null ? R : a.control.focusBorder });
                }
            }
            var A = x.map(function (L) { return L.text; }).join("\n"), O = Ct(n, "__valueText", function (L) { L.style = { fontFamily: a.fontFamily, fontSize: a.fontSize, fill: a.text, fontWeight: "400", wordWrap: !1 }, L.zIndex = 1; });
            O.text = A, O.position.set(D, w);
        }
        k && (Ut(n), n.eventMode = "static", n.cursor = "text", n.on("pointerdown", function (G) {
            var e_8, _a, e_9, _b, e_10, _c;
            var g, S, X, x, I, A, O, L, K, Y, N, z, V;
            if ((G == null ? void 0 : G.button) === 2)
                return;
            var U = t.getPointerId ? t.getPointerId(G) : Number((X = (S = G == null ? void 0 : G.pointerId) != null ? S : (g = G == null ? void 0 : G.data) == null ? void 0 : g.pointerId) != null ? X : 0);
            if (!(U <= 0)) {
                if (f.focusedKeyByPointer.set(U, k), f.keyboardOwnerPointerId = U, y === "checkbox") {
                    var J = d(k, e.attrs), rt = J.indeterminate === !0, st = J.checked === !0;
                    !st && !rt ? (J.checked = !0, J.indeterminate = !1) : st && !rt ? (J.checked = !1, J.indeterminate = !0) : (J.checked = !1, J.indeterminate = !1);
                }
                else if (y === "radio") {
                    var rt = "radio:".concat((I = (x = e.attrs) == null ? void 0 : x.name) != null ? I : "__default__"), st = (A = c.get(rt)) != null ? A : [];
                    try {
                        for (var st_1 = __values(st), st_1_1 = st_1.next(); !st_1_1.done; st_1_1 = st_1.next()) {
                            var mt = st_1_1.value;
                            var Tt = d(mt, void 0);
                            Tt.checked = mt === k;
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
                    var J = d(k, e.attrs);
                    if (typeof J.value == "string") {
                        try {
                            for (var _d = __values(f.inputs.entries()), _f = _d.next(); !_f.done; _f = _d.next()) {
                                var _g = __read(_f.value, 2), lt = _g[0], yt = _g[1];
                                lt !== k && ((O = yt.selections) == null || O.delete(U));
                            }
                        }
                        catch (e_9_1) { e_9 = { error: e_9_1 }; }
                        finally {
                            try {
                                if (_f && !_f.done && (_b = _d.return)) _b.call(_d);
                            }
                            finally { if (e_9) throw e_9.error; }
                        }
                        var rt = y === "password" ? "\u2022".repeat(J.value.length) : J.value, st = f.fieldBounds.get(k), mt = (L = st == null ? void 0 : st.innerWidth) != null ? L : Math.max(0, i - D * 2), Tt = ue(ce(rt, mt, h), T), Mt = ((Y = (K = G.global) == null ? void 0 : K.x) != null ? Y : 0) - s - D, Gt = ((z = (N = G.global) == null ? void 0 : N.y) != null ? z : 0) - l - w, vt = Ee({ fullText: rt, lines: Tt, localX: Mt, localY: Gt, lineHeight: $, measure: h });
                        J.selections || (J.selections = new Map), J.selections.set(U, { start: vt, end: vt });
                        try {
                            for (var _h = __values(E.entries()), _j = _h.next(); !_j.done; _j = _h.next()) {
                                var _k = __read(_j.value, 2), lt = _k[0], yt = _k[1];
                                yt.key === k && lt !== U && E.delete(lt);
                            }
                        }
                        catch (e_10_1) { e_10 = { error: e_10_1 }; }
                        finally {
                            try {
                                if (_j && !_j.done && (_c = _h.return)) _c.call(_h);
                            }
                            finally { if (e_10) throw e_10.error; }
                        }
                        E.set(U, { key: k, anchor: vt });
                    }
                }
                (y === "checkbox" || y === "radio") && ((V = G.stopPropagation) == null || V.call(G)), p == null || p();
            }
        }), (y === "checkbox" || y === "radio") && (n.cursor = "pointer"), n.hitArea = new bt(0, 0, Math.max(0, i), Math.max(0, o)));
    }
    function $r(t, e) { t.setPadding(e.EDGE_TOP, 6), t.setPadding(e.EDGE_BOTTOM, 6), t.setHeight(108), t.setMinHeight(108), t.setMinWidth(220); }
    function Hr(t) {
        var e_11, _a, e_12, _b;
        var nt, Q, j, G, U, g, S, X;
        var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.absX, l = t.absY, a = t.theme, h = t.textMeasure, f = t.uiState, d = t.getOrInitInputState, b = t.clamp, c = t.textDrags, E = t.requestPaint, p = e.key, y = p ? d(p, ge(ne({}, (nt = e.attrs) != null ? nt : {}), { type: "text" })) : void 0, k = (Q = t.showCaret) != null ? Q : !1, M = (j = t.caretPointerId) != null ? j : null, _ = t.focusColor, B = t.getCursorColor;
        n.sortableChildren = !0;
        try {
            for (var _c = __values(n.children), _d = _c.next(); !_d.done; _d = _c.next()) {
                var x = _d.value;
                var I = x.label;
                I && (I.startsWith("__sel:") || I === "__caret") && (x.visible = !1);
            }
        }
        catch (e_11_1) { e_11 = { error: e_11_1 }; }
        finally {
            try {
                if (_d && !_d.done && (_a = _c.return)) _a.call(_c);
            }
            finally { if (e_11) throw e_11.error; }
        }
        var R = 8, H = 6 + xt, D = 5, w = a.fontSize * 1.25, T = _ != null ? 2 : 1, $ = T / 2;
        a.control.radius > 0 ? r.roundRect($, $, Math.max(0, i - T), Math.max(0, o - T), a.control.radius) : r.rect($, $, Math.max(0, i - T), Math.max(0, o - T)), r.fill(a.control.background), r.stroke({ width: T, color: _ != null ? _ : a.control.border });
        var C = (G = y == null ? void 0 : y.value) != null ? G : "", F = Math.max(0, i - R * 2);
        p && f.fieldBounds.set(p, { x: s, y: l, w: i, h: o, innerLeft: R, innerTop: H, innerWidth: F, maxLines: D, isPassword: !1 });
        var m = ce(C, F, h), P = ue(m, D), v = P.length > 0 ? P[P.length - 1].end : 0;
        if (p && y && typeof y.value == "string") {
            var x = y.selections;
            if (x && x.size > 0)
                try {
                    for (var _f = __values(x.entries()), _g = _f.next(); !_g.done; _g = _f.next()) {
                        var _h = __read(_g.value, 2), I = _h[0], A = _h[1];
                        var O = b((U = A.start) != null ? U : 0, 0, C.length), L = b((g = A.end) != null ? g : O, 0, C.length), K = b(Math.min(O, L), 0, v), Y = b(Math.max(O, L), 0, v);
                        if (K === Y)
                            continue;
                        var N = Pt(n, "__sel:".concat(I));
                        kt(N), N.zIndex = 0, N.visible = !0;
                        for (var z = 0; z < P.length; z++) {
                            var V = P[z], J = Math.max(K, V.start), rt = Math.min(Y, V.end);
                            if (J >= rt)
                                continue;
                            var st = R + h(C.slice(V.start, J)), mt = h(C.slice(J, rt));
                            N.rect(st, H + z * w, mt, w);
                        }
                        N.fill({ color: B(I), alpha: .22 });
                    }
                }
                catch (e_12_1) { e_12 = { error: e_12_1 }; }
                finally {
                    try {
                        if (_g && !_g.done && (_b = _f.return)) _b.call(_f);
                    }
                    finally { if (e_12) throw e_12.error; }
                }
            if (k && M != null) {
                var I = (S = y.selections) == null ? void 0 : S.get(M), A = I ? I.end : 0, O = b(A, 0, v), L = Math.max(0, P.length - 1);
                for (var z = 0; z < P.length; z++) {
                    var V = P[z];
                    if (O >= V.start && O <= V.end) {
                        L = z;
                        break;
                    }
                }
                var K = (X = P[L]) != null ? X : { start: 0, end: 0, text: "" }, Y = R + h(C.slice(K.start, O)), N = Pt(n, "__caret");
                kt(N), N.zIndex = 2, N.visible = !0, N.moveTo(Y, H + L * w), N.lineTo(Y, H + L * w + w), N.stroke({ width: 1, color: _ != null ? _ : a.control.focusBorder });
            }
        }
        var et = P.map(function (x) { return x.text; }).join("\n"), q = Ct(n, "__valueText", function (x) { x.style = { fontFamily: a.fontFamily, fontSize: a.fontSize, fill: a.text, fontWeight: "400", wordWrap: !1 }, x.zIndex = 1; });
        q.text = et, q.position.set(R, H), p && (Ut(n), n.eventMode = "static", n.cursor = "text", n.hitArea = new bt(0, 0, Math.max(0, i), Math.max(0, o)), n.on("pointerdown", function (x) {
            var e_13, _a, e_14, _b;
            var O, L, K, Y, N, z, V, J, rt, st;
            if ((x == null ? void 0 : x.button) === 2)
                return;
            var I = t.getPointerId ? t.getPointerId(x) : Number((K = (L = x == null ? void 0 : x.pointerId) != null ? L : (O = x == null ? void 0 : x.data) == null ? void 0 : O.pointerId) != null ? K : 0);
            if (I <= 0)
                return;
            f.focusedKeyByPointer.set(I, p), f.keyboardOwnerPointerId = I;
            var A = d(p, ge(ne({}, (Y = e.attrs) != null ? Y : {}), { type: "text" }));
            if (typeof A.value == "string") {
                try {
                    for (var _c = __values(f.inputs.entries()), _d = _c.next(); !_d.done; _d = _c.next()) {
                        var _f = __read(_d.value, 2), It = _f[0], At = _f[1];
                        It !== p && ((N = At.selections) == null || N.delete(I));
                    }
                }
                catch (e_13_1) { e_13 = { error: e_13_1 }; }
                finally {
                    try {
                        if (_d && !_d.done && (_a = _c.return)) _a.call(_c);
                    }
                    finally { if (e_13) throw e_13.error; }
                }
                var mt = f.fieldBounds.get(p), Tt = (z = mt == null ? void 0 : mt.innerWidth) != null ? z : Math.max(0, i - R * 2), Mt = A.value, Gt = ue(ce(Mt, Tt, h), D), vt = ((J = (V = x.global) == null ? void 0 : V.x) != null ? J : 0) - s - R, lt = ((st = (rt = x.global) == null ? void 0 : rt.y) != null ? st : 0) - l - H, yt = Ee({ fullText: Mt, lines: Gt, localX: vt, localY: lt, lineHeight: w, measure: h });
                A.selections || (A.selections = new Map), A.selections.set(I, { start: yt, end: yt });
                try {
                    for (var _g = __values(c.entries()), _h = _g.next(); !_h.done; _h = _g.next()) {
                        var _j = __read(_h.value, 2), It = _j[0], At = _j[1];
                        At.key === p && It !== I && c.delete(It);
                    }
                }
                catch (e_14_1) { e_14 = { error: e_14_1 }; }
                finally {
                    try {
                        if (_h && !_h.done && (_b = _g.return)) _b.call(_g);
                    }
                    finally { if (e_14) throw e_14.error; }
                }
                c.set(I, { key: p, anchor: yt });
            }
            E == null || E();
        }));
    }
    function Ur(t, e) { t.setFlexDirection(e.FLEX_DIRECTION_ROW), t.setAlignItems(e.ALIGN_CENTER), t.setJustifyContent(e.JUSTIFY_FLEX_START), t.setPadding(e.EDGE_LEFT, 8), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0); }
    function qi(t, e, n, r, i) { t.circle(e, n, r), t.stroke({ width: 2, color: i }); var o = e + r * .65, s = n + r * .65, l = e + r * 1.55, a = n + r * 1.55; t.moveTo(o, s), t.lineTo(l, a), t.stroke({ width: 2, color: i }); }
    function Xr(t, e) { t.setFlexDirection(e.FLEX_DIRECTION_ROW), t.setFlexWrap(e.WRAP_NO_WRAP), t.setAlignItems(e.ALIGN_CENTER), t.setJustifyContent(e.JUSTIFY_FLEX_START), t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0); }
    function Yr(t, e) { t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0), t.setWidth(36), t.setHeight(36), t.setMinWidth(36), t.setMinHeight(36), t.setFlexGrow(0), t.setFlexShrink(0), t.setMargin(e.EDGE_RIGHT, 6); }
    function Kr(t) { var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.theme, l = t.uiState, a = t.getPointerId, h = t.focusInputKey, f = t.requestPaint, d = function (c) { r.clear(); var E = 1, p = E / 2; s.control.button.radius > 0 ? r.roundRect(p, p, Math.max(0, i - E), Math.max(0, o - E), s.control.button.radius) : r.rect(p, p, Math.max(0, i - E), Math.max(0, o - E)), r.fill(c), r.stroke({ width: E, color: s.control.button.border }); var y = i / 2 - 2, k = o / 2 - 2, M = Math.max(5, Math.min(7, Math.min(i, o) * .22)); qi(r, y, k, M, s.text); }; d(s.control.button.fill), Ut(n), n.eventMode = "static", n.cursor = "pointer", n.hitArea = new bt(0, 0, Math.max(0, i), Math.max(0, o)), n.on("pointerover", function () { return d(s.control.button.hoverFill); }), n.on("pointerout", function () { return d(s.control.button.fill); }), n.on("pointerdown", function (c) { var E; if ((c == null ? void 0 : c.button) !== 2) {
        if (d(s.control.button.activeFill), h) {
            var p = a(c);
            p > 0 && (l.focusedKeyByPointer.set(p, h), l.keyboardOwnerPointerId = p);
        }
        f == null || f(), (E = c.stopPropagation) == null || E.call(c);
    } }), n.on("pointerup", function () { return d(s.control.button.hoverFill); }); var b = e.attrs; }
    function Qe(t, e) { var n = t.get(e); if (n)
        return n; var r = { x: 0, y: 0 }; return t.set(e, r), r; }
    function zr(t, e) { t.setPositionType(e.POSITION_TYPE_ABSOLUTE), t.setPosition(e.EDGE_LEFT, 0), t.setPosition(e.EDGE_TOP, 0), t.setAlignSelf(e.ALIGN_FLEX_START), t.setFlexGrow(0), t.setFlexShrink(0), t.setPadding(e.EDGE_LEFT, 12), t.setPadding(e.EDGE_RIGHT, 12), t.setPadding(e.EDGE_TOP, 12), t.setPadding(e.EDGE_BOTTOM, 12), t.setWidth(540), t.setMinWidth(360), t.setMinHeight(148); }
    function jr(t) { var B, R; var e = t.node, n = t.container, r = t.w, i = t.h, o = t.theme, s = t.selectedBy, l = t.getCursorColor, a = t.dialogStates, h = t.dialogDrags, f = t.bringToFront, d = t.requestPaint, b = e.key; if (!b)
        return; var c = s.get(b), E = c == null ? o.boxBorder : l(c), p = Math.max(0, Math.round(r)), y = Math.max(0, Math.round(i)), k = Pt(n, "__dialogBorder"); kt(k), k.rect(0, 0, p, y), k.fill({ color: 16777215, alpha: .8 }); var M = c == null ? 1 : 2, _ = M / 2; k.rect(_, _, Math.max(0, p - M), Math.max(0, y - M)), k.stroke({ width: M, color: E, alignment: 0 }), k.eventMode = "static", k.cursor = "move", k.hitArea = new bt(0, 0, p, y), k.on("pointerdown", function (H) {
        var e_15, _a;
        var $, C, F, m, P, v, et, q;
        var D = function (nt) { try {
            typeof console != "undefined" && typeof console.log == "function" && console.log("[dialog pointerdown] ".concat(nt));
        }
        catch (Q) { } };
        if (D("start"), (H == null ? void 0 : H.button) === 2)
            return;
        D("pointer-id");
        var w = t.getPointerId ? t.getPointerId(H) : Number((F = (C = H == null ? void 0 : H.pointerId) != null ? C : ($ = H == null ? void 0 : H.data) == null ? void 0 : $.pointerId) != null ? F : 0);
        if (w <= 0 || w <= 0)
            return;
        D("clear-other-drags");
        try {
            for (var _b = __values(h.entries()), _c = _b.next(); !_c.done; _c = _b.next()) {
                var _d = __read(_c.value, 2), nt = _d[0], Q = _d[1];
                Q.key === b && nt !== w && h.delete(nt);
            }
        }
        catch (e_15_1) { e_15 = { error: e_15_1 }; }
        finally {
            try {
                if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
            }
            finally { if (e_15) throw e_15.error; }
        }
        D("select"), s.set(b, w), D("bring-to-front"), f == null || f(b), D("state");
        var T = Qe(a, b);
        D("set-drag"), h.set(w, { key: b, startGX: (P = (m = H.global) == null ? void 0 : m.x) != null ? P : 0, startGY: (et = (v = H.global) == null ? void 0 : v.y) != null ? et : 0, originX: T.x, originY: T.y }), D("request-paint"), d == null || d(), D("stop-propagation"), (q = H.stopPropagation) == null || q.call(H), D("done");
    }); {
        var H = n.getChildByLabel, D = (R = (B = H == null ? void 0 : H.call(n, "__children")) != null ? B : n.children.find(function (w) { return w && w.label === "__children"; })) != null ? R : null;
        if (D && k.parent === n) {
            var w = n.getChildIndex(D), T = Math.max(0, n.children.length - 1), $ = Math.max(0, Math.min(w - 1, T));
            n.getChildIndex(k) > $ && n.setChildIndex(k, $);
        }
    } }
    function Sn(t, e, n) { var l; var r = t.get(e); if (r)
        return r; var i = Number((l = n == null ? void 0 : n.value) != null ? l : "0"), s = { value: Number.isFinite(i) ? i : 0 }; return t.set(e, s), s; }
    function Vr(t, e) { t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0), t.setHeight(36), t.setMinHeight(36), t.setMinWidth(140), t.setFlexGrow(0), t.setFlexShrink(0); }
    function to(t, e, n) { return Math.max(e, Math.min(n, t)); }
    function kn(t, e, n) { var i; var r = Number((i = t == null ? void 0 : t[e]) != null ? i : ""); return Number.isFinite(r) ? r : n; }
    function eo(t, e, n, r, i, o) { var l = e + 3, a = e + r - 3, h = n + 3, f = n + i - 3; t.moveTo(l, f), t.lineTo((l + a) / 2, h), t.lineTo(a, f), t.stroke({ width: 2, color: o }); }
    function no(t, e, n, r, i, o) { var l = e + 3, a = e + r - 3, h = n + 3, f = n + i - 3; t.moveTo(l, h), t.lineTo((l + a) / 2, f), t.lineTo(a, h), t.stroke({ width: 2, color: o }); }
    function Jr(t) { var F; var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.theme, l = t.getValue, a = t.setValue, h = t.requestPaint, f = e.key, d = e.attrs, b = kn(d, "min", 0), c = kn(d, "max", 255), E = Math.max(1e-9, kn(d, "step", 1)), p = l(), y = 1, k = y / 2; r.rect(k, k, Math.max(0, i - y), Math.max(0, o - y)), r.fill(s.control.background), r.stroke({ width: y, color: s.control.border }); var M = 22, _ = Math.max(0, i - M); r.moveTo(_ + .5, 0), r.lineTo(_ + .5, o), r.stroke({ width: 1, color: s.control.border, alignment: 0 }); var B = Pt(n, "__arrows"); kt(B), eo(B, _, 0, M, o / 2, s.text), no(B, _, o / 2, M, o / 2, s.text); var R = ((F = d == null ? void 0 : d.channel) != null ? F : "").toLowerCase(), H = R === "r" ? "R" : R === "g" ? "G" : R === "b" ? "B" : R === "a" ? "A" : "", D = Ct(n, "__valueText", function (m) { m.style = { fontFamily: s.fontFamily, fontSize: s.fontSize, fill: s.text, fontWeight: "400", wordWrap: !1 }; }); if (D.text = H ? "".concat(H, ": ").concat(Math.round(p)) : String(Math.round(p)), D.position.set(8, 9 + xt), !f)
        return; var w = new bt(_, 0, M, o / 2), T = new bt(_, o / 2, M, o / 2), $ = function (m) { var P = l(), v = to(P + m * E, b, c); a(v), h == null || h(); }, C = Pt(n, "__hit"); kt(C), C.eventMode = "static", C.cursor = "default", C.hitArea = new bt(0, 0, Math.max(0, i), Math.max(0, o)), C.on("pointerdown", function (m) {
        var e_16, _a;
        var j, G, U, g, S, X;
        if ((m == null ? void 0 : m.button) === 2)
            return;
        var P = t.getPointerId ? t.getPointerId(m) : Number((U = (G = m == null ? void 0 : m.pointerId) != null ? G : (j = m == null ? void 0 : m.data) == null ? void 0 : j.pointerId) != null ? U : 0);
        if (P <= 0)
            return;
        var v = n.toLocal(m.global), et = (g = v == null ? void 0 : v.x) != null ? g : 0, q = (S = v == null ? void 0 : v.y) != null ? S : 0, nt = w.contains(et, q) ? 1 : T.contains(et, q) ? -1 : null;
        if (!nt)
            return;
        $(nt);
        var Q = t.numberHolds;
        if (Q && f) {
            try {
                for (var _b = __values(Q.entries()), _c = _b.next(); !_c.done; _c = _b.next()) {
                    var _d = __read(_c.value, 2), A = _d[0], O = _d[1];
                    A !== P && (O.timeoutId != null && window.clearTimeout(O.timeoutId), O.intervalId != null && window.clearInterval(O.intervalId), Q.delete(A));
                }
            }
            catch (e_16_1) { e_16 = { error: e_16_1 }; }
            finally {
                try {
                    if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
                }
                finally { if (e_16) throw e_16.error; }
            }
            var x = Q.get(P);
            x && (x.timeoutId != null && window.clearTimeout(x.timeoutId), x.intervalId != null && window.clearInterval(x.intervalId));
            var I_1 = { key: f, timeoutId: null, intervalId: null };
            I_1.timeoutId = window.setTimeout(function () { I_1.timeoutId = null, I_1.intervalId = window.setInterval(function () { $(nt); }, 250); }, 500), Q.set(P, I_1);
        }
        (X = m.stopPropagation) == null || X.call(m);
    }); }
    var qe = null;
    function Zr() { return qe || (qe = new Ve({ data: ie, label: "attribute-color-picker-colors", shrinkToFit: !1, usage: Tn.VERTEX | Tn.COPY_DST }), qe); }
    function Qr(t, e, n) { var h, f, d, b; t.setPadding(n.EDGE_LEFT, 0), t.setPadding(n.EDGE_RIGHT, 0), t.setPadding(n.EDGE_TOP, 0), t.setPadding(n.EDGE_BOTTOM, 0); var r = Number((f = (h = e.attrs) == null ? void 0 : h.width) != null ? f : "0"), i = Number((b = (d = e.attrs) == null ? void 0 : d.height) != null ? b : "0"), o = Number.isFinite(r) && r > 0, s = Number.isFinite(i) && i > 0, l = o ? r : 240, a = s ? i : 200; (o || s) && (t.setAlignSelf(n.ALIGN_FLEX_START), t.setFlexGrow(0), t.setFlexShrink(0)), t.setWidth(l), t.setHeight(a), t.setMinWidth(Math.min(240, l)), t.setMinHeight(Math.min(200, a)); }
    function ae(t) { return Number.isFinite(t) ? Math.max(0, Math.min(255, Math.round(t))) : 0; }
    function tn(t) { return ae(t).toString(16).padStart(2, "0"); }
    function ro(t, e, n, r, i, o, s, l) { var a = s - n, h = l - r, f = i - n, d = o - r, b = t - n, c = e - r, E = a * a + h * h, p = a * f + h * d, y = a * b + h * c, k = f * f + d * d, M = f * b + d * c, _ = 1 / (E * k - p * p), B = (k * y - p * M) * _, R = (E * M - p * y) * _; return B >= 0 && R >= 0 && B + R <= 1; }
    function io(t, e, n, r, i, o, s, l) { var a = i - n, h = o - r, f = s - n, d = l - r, b = t - n, c = e - r, E = a * d - f * h; if (Math.abs(E) < 1e-9)
        return { w0: 1, w1: 0, w2: 0 }; var p = (b * d - f * c) / E, y = (a * c - b * h) / E; return { w0: 1 - p - y, w1: p, w2: y }; }
    var oo = { name: "solid-out", fragment: { main: "\n      outColor = vec4(1.0);\n    " } }, en = null;
    function so() { if (en)
        return en; var t = { name: "color-picker-vertex-color", bits: [Qn, qn, Zn, oo] }; return en = new De({ glProgram: t, resources: {} }), en; }
    function qr(t, e, n) { var r = new Float32Array(12), i = [-90, -30, 30, 90, 150, 210]; for (var o = 0; o < 6; o++) {
        var s = i[o] * Math.PI / 180;
        r[o * 2 + 0] = t + Math.cos(s) * n, r[o * 2 + 1] = e + Math.sin(s) * n;
    } return r; }
    var ie = new Uint8Array([255, 0, 0, 255, 128, 128, 0, 255, 0, 255, 0, 255, 0, 128, 128, 255, 0, 0, 255, 255, 128, 0, 128, 255]), ke = new Uint32Array([0, 1, 2, 0, 2, 3, 0, 3, 4, 0, 4, 5]);
    function In(t) { var e = t.lx, n = t.ly, r = t.w, i = t.h, o = 10, s = Math.max(0, r - o * 2), l = Math.max(0, i - o * 2), a = o + s / 2, h = o + l / 2, f = Math.max(0, Math.min(s, l) / 2 - 2), d = qr(a, h, f); for (var b = 0; b < ke.length; b += 3) {
        var c = ke[b + 0], E = ke[b + 1], p = ke[b + 2], y = d[c * 2 + 0], k = d[c * 2 + 1], M = d[E * 2 + 0], _ = d[E * 2 + 1], B = d[p * 2 + 0], R = d[p * 2 + 1];
        if (!ro(e, n, y, k, M, _, B, R))
            continue;
        var H = io(e, n, y, k, M, _, B, R), D = c * 4, w = E * 4, T = p * 4, $ = H.w0 * ie[D + 0] + H.w1 * ie[w + 0] + H.w2 * ie[T + 0], C = H.w0 * ie[D + 1] + H.w1 * ie[w + 1] + H.w2 * ie[T + 1], F = H.w0 * ie[D + 2] + H.w1 * ie[w + 2] + H.w2 * ie[T + 2];
        return { r: ae($), g: ae(C), b: ae(F) };
    } return null; }
    function ti(t) { var Q, j; var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.theme, l = t.rgb, a = t.setRgb, h = t.alpha, f = t.setAlpha, d = t.pick, b = t.setPick, c = t.requestPaint, E = t.getPointerId, p = t.setDraggingPointerId, y = 1, k = y / 2; r.rect(k, k, Math.max(0, i - y), Math.max(0, o - y)), r.fill(16777215), r.stroke({ width: y, color: s.control.border, alignment: 0 }); var M = 10, _ = Math.max(0, i - M * 2), B = Math.max(0, o - M * 2), R = M + _ / 2, H = M + B / 2, D = Math.max(0, Math.min(_, B) / 2 - 2), w = qr(R, H, D), T = "".concat(Math.round(i), "x").concat(Math.round(o)), $ = n.getChildByLabel, C = $ ? $.call(n, "__mesh") : n.children.find(function (G) { return (G == null ? void 0 : G.label) === "__mesh"; }); if (C) {
        if (C.__sizeKey !== T) {
            var G = new Float32Array(w.length), U = new Te({ positions: w, uvs: G, indices: ke });
            U.addAttribute("aColor", { buffer: Zr(), format: "unorm8x4", stride: 4, offset: 0 });
            try {
                (j = (Q = C.geometry) == null ? void 0 : Q.destroy) == null || j.call(Q);
            }
            catch (g) { }
            C.geometry = U, C.__sizeKey = T;
        }
    }
    else {
        var G = new Float32Array(w.length), U = new Te({ positions: w, uvs: G, indices: ke });
        U.addAttribute("aColor", { buffer: Zr(), format: "unorm8x4", stride: 4, offset: 0 }), C = new je({ geometry: U, shader: so() }), C.label = "__mesh", n.addChild(C), C.__sizeKey = T;
    } C.removeAllListeners(), C.eventMode = "static", C.cursor = "crosshair", C.hitArea = new bt(M, M, _, B), C.on("pointerdown", function (G) { var I, A, O; if ((G == null ? void 0 : G.button) === 2)
        return; var U = E(G); if (U <= 0)
        return; var g = n.toLocal(G.global), S = (I = g == null ? void 0 : g.x) != null ? I : 0, X = (A = g == null ? void 0 : g.y) != null ? A : 0, x = In({ lx: S, ly: X, w: i, h: o }); x && (b({ x: S, y: X }), a(x), p(U), c == null || c(), (O = G.stopPropagation) == null || O.call(G)); }); {
        var G = Pt(n, "__border");
        kt(G), G.moveTo(w[0], w[1]);
        for (var U = 1; U < 6; U++)
            G.lineTo(w[U * 2 + 0], w[U * 2 + 1]);
        G.closePath(), G.stroke({ width: 2, color: 0 });
    } var F = Pt(n, "__overlay"); kt(F); var m = 44, P = 18, v = Math.max(M, i - M - m), et = M; F.rect(v, et, m, P), F.fill({ color: ae(l.r) << 16 | ae(l.g) << 8 | ae(l.b), alpha: Math.max(0, Math.min(1, ae(h) / 255)) }), F.rect(v + .5, et + .5, m - 1, P - 1), F.stroke({ width: 1, color: s.control.border, alignment: 0 }), d && (F.circle(d.x, d.y, 4), F.stroke({ width: 2, color: 16777215 }), F.circle(d.x, d.y, 4), F.stroke({ width: 1, color: 0 })); var q = "#".concat(tn(l.r)).concat(tn(l.g)).concat(tn(l.b)).concat(tn(h)).toUpperCase(), nt = Ct(n, "__label", function (G) { G.style = { fontFamily: s.fontFamily, fontSize: Math.max(10, Math.floor(s.fontSize * .75)), fill: s.mutedText, fontWeight: "400", wordWrap: !1 }; }); nt.text = q, nt.position.set(M, Math.max(M, o - M - nt.height)), f && f(ae(h)); }
    function de(t, e, n) { var r = t.get(e); if (r)
        return r; var i = { selectedIndex: Math.max(0, n | 0), open: !1 }; return t.set(e, i), i; }
    function ei(t, e) { t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0), t.setHeight(36), t.setMinHeight(36), t.setMinWidth(220); }
    function ao(t, e, n, r, i, o) { var l = e + 4, a = e + r - 4, h = n + 4, f = n + i - 4; t.moveTo(l, (h + f) / 2 - 2), t.lineTo((l + a) / 2, (h + f) / 2 + 2), t.lineTo(a, (h + f) / 2 - 2), t.stroke({ width: 2, color: o }); }
    function lo(t) {
        var r;
        var n = String((r = t == null ? void 0 : t["data-options"]) != null ? r : "").split("\n").map(function (i) { return i.trim(); }).filter(function (i) { return i.length > 0; });
        return n.length > 0 ? n : ["(empty)"];
    }
    function co(t) { var n; var e = Number((n = t == null ? void 0 : t["data-selected-index"]) != null ? n : "0"); return Number.isFinite(e) ? Math.max(0, e | 0) : 0; }
    function nn(t) { var C; var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.absX, l = t.absY, a = t.theme, h = t.selectStates, f = t.uiState, d = t.getPointerId, b = t.getCursorColor, c = t.requestPaint, E = t.popupSink, p = e.key; if (!p)
        return; var y = lo(e.attrs), k = co(e.attrs), M = de(h, p, k); M.selectedIndex = Math.max(0, Math.min(y.length - 1, M.selectedIndex | 0)); var _ = (function () {
        var e_17, _a;
        var F = f.keyboardOwnerPointerId;
        if (f.focusedKeyByPointer.get(F) === p)
            return F;
        try {
            for (var _b = __values(f.focusedKeyByPointer.entries()), _c = _b.next(); !_c.done; _c = _b.next()) {
                var _d = __read(_c.value, 2), m = _d[0], P = _d[1];
                if (P === p)
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
    })(), B = _ != null ? b(_) : null, R = B != null ? 2 : 1, H = R / 2; a.control.radius > 0 ? r.roundRect(H, H, Math.max(0, i - R), Math.max(0, o - R), a.control.radius) : r.rect(H, H, Math.max(0, i - R), Math.max(0, o - R)), r.fill(a.control.background), r.stroke({ width: R, color: B != null ? B : a.control.border }); var D = 22, w = Math.max(0, i - D); r.moveTo(w + .5, 0), r.lineTo(w + .5, o), r.stroke({ width: 1, color: a.control.border, alignment: 0 }), ao(r, w, 0, D, o, a.text); var T = (C = y[M.selectedIndex]) != null ? C : "", $ = Ct(n, "__label", function (F) { F.style = { fontFamily: a.fontFamily, fontSize: a.fontSize, fill: a.text, fontWeight: "400", wordWrap: !1 }; }); $.text = T, $.position.set(8, 9 + xt), Ut(n), n.eventMode = "static", n.cursor = "pointer", n.hitArea = new bt(0, 0, Math.max(0, i), Math.max(0, o)), n.on("pointerdown", function (F) { var P; if ((F == null ? void 0 : F.button) === 2)
        return; var m = d(F); m <= 0 || (f.focusedKeyByPointer.set(m, p), f.keyboardOwnerPointerId = m, M.open = !M.open, c == null || c(), (P = F.stopPropagation) == null || P.call(F)); }), M.open && E.push({ key: p, absX: s, absY: l, w: i, h: o, options: y, selectedIndex: M.selectedIndex }); }
    function ni(t) { var M; var e = t.popup, n = t.stage, r = t.theme, i = t.selectStates, o = t.uiState, s = t.getPointerId, l = t.requestPaint, a = t.viewportW, h = t.viewportH, f = 30, b = Math.min(7, e.options.length), c = b * f, E = e.absX, p = e.absY + e.h; E = Math.max(0, Math.min(E, Math.max(0, a - e.w))), p + c > h - 4 && (p = e.absY - c), p = Math.max(0, Math.min(p, Math.max(0, h - c))); var y = new wt; y.position.set(E, p), n.addChild(y); var k = new _t; k.rect(0, 0, e.w, c), k.fill(16777215), k.rect(.5, .5, Math.max(0, e.w - 1), Math.max(0, c - 1)), k.stroke({ width: 1, color: r.control.border, alignment: 0 }), y.addChild(k), y.eventMode = "static", y.cursor = "pointer", y.hitArea = new bt(0, 0, e.w, c), y.on("pointerdown", function (_) { var $, C, F; if ((_ == null ? void 0 : _.button) === 2)
        return; var B = s(_), R = y.toLocal(_.global), H = ($ = R == null ? void 0 : R.x) != null ? $ : -1, D = (C = R == null ? void 0 : R.y) != null ? C : -1; if (H < 0 || H > e.w || D < 0 || D > c)
        return; var w = Math.max(0, Math.min(e.options.length - 1, Math.floor(D / f))), T = i.get(e.key); T && (T.selectedIndex = w, T.open = !1), B > 0 && (o.focusedKeyByPointer.set(B, e.key), o.keyboardOwnerPointerId = B), l == null || l(), (F = _.stopPropagation) == null || F.call(_); }); for (var _ = 0; _ < b; _++) {
        var B = _ * f;
        if (_ === e.selectedIndex) {
            var H = new _t;
            H.rect(1, B + 1, Math.max(0, e.w - 2), f - 2), H.fill({ color: 0, alpha: .06 }), y.addChild(H);
        }
        var R = jt({ text: (M = e.options[_]) != null ? M : "", fontFamily: r.fontFamily, fontSize: r.fontSize, fill: r.text, wordWrap: !1 });
        R.position.set(8, B + 7 + xt), y.addChild(R);
    } }
    function Ot(t, e, n) { var r = Number.isFinite(t) ? t | 0 : 0; return Math.max(e, Math.min(n, r)); }
    function zt(t) { var e = Ot(t, 0, 99); return e < 10 ? "0".concat(e) : String(e); }
    function te(t, e, n) { var r = Number(t); if (!Number.isFinite(r))
        return null; var i = Math.trunc(r); return i < e || i > n ? null : i; }
    function sn(t) { if (t.length !== 4)
        return null; for (var r = 0; r < t.length; r += 1) {
        var i = t.charCodeAt(r);
        if (i < 48 || i > 57)
            return null;
    } var e = Number(t); if (!Number.isFinite(e))
        return null; var n = e - 2e3; return n < 0 || n > 99 ? null : n; }
    function uo(t) { var e = String(t != null ? t : "").trim().split(":"); if (e.length !== 2 && e.length !== 3)
        return null; var n = te(e[0], 0, 23), r = te(e[1], 0, 59), i = e.length === 3 ? te(e[2], 0, 59) : 0; return n == null || r == null || i == null ? null : { hour: n, minute: r, second: i }; }
    function ho(t) { var e = String(t != null ? t : "").trim().split("-"); if (e.length !== 2)
        return null; var n = sn(e[0]), r = te(e[1], 1, 12); return n == null || r == null ? null : { year2: n, month: r }; }
    function mo(t) { var e = String(t != null ? t : "").trim().split("-"); if (e.length !== 3)
        return null; var n = sn(e[0]), r = te(e[1], 1, 12), i = te(e[2], 1, 31); if (n == null || r == null || i == null)
        return null; var o = Ot(Math.floor((i - 1) / 7) + 1, 1, 4); return { year2: n, month: r, weekIndex: o }; }
    function fo(t) { var e = String(t != null ? t : "").trim(), n = e.indexOf("-W"); if (n < 0)
        return null; var r = sn(e.slice(0, n)), i = te(e.slice(n + 2), 1, 48); if (r == null || i == null)
        return null; var o = Ot(Math.floor((i - 1) / 4) + 1, 1, 12), s = Ot((i - 1) % 4 + 1, 1, 4); return { year2: r, month: o, weekIndex: s }; }
    function po(t) { var e = String(t != null ? t : "").trim(), n = e.indexOf("T"); if (n < 0 && (n = e.indexOf(" ")), n < 0)
        return null; var r = e.slice(0, n).split("-"), i = e.slice(n + 1).split(":"); if (r.length !== 3 || i.length !== 2 && i.length !== 3)
        return null; var o = sn(r[0]), s = te(r[1], 1, 12), l = te(r[2], 1, 31), a = te(i[0], 0, 23), h = te(i[1], 0, 59), f = i.length === 3 ? te(i[2], 0, 59) : 0; if (o == null || s == null || l == null || a == null || h == null || f == null)
        return null; var d = Ot(Math.floor((l - 1) / 7) + 1, 1, 4); return { year2: o, month: s, weekIndex: d, hour: a, minute: h, second: f }; }
    function rn(t) { return "20".concat(zt(t.year2), "-").concat(zt(t.month)); }
    function go(t) { return (Ot(t.month, 1, 12) - 1) * 4 + Ot(t.weekIndex, 1, 4); }
    function on(t) { return "20".concat(zt(t.year2), "-W").concat(zt(go(t))); }
    function Se(t) { var e = (Ot(t.weekIndex, 1, 4) - 1) * 7 + 1; return "20".concat(zt(t.year2), "-").concat(zt(t.month), "-").concat(zt(e)); }
    function Le(t) { return "".concat(zt(t.hour), ":").concat(zt(t.minute), ":").concat(zt(t.second)); }
    function Ge(t) { return "".concat(Se(t), "T").concat(Le(t)); }
    function bo(t) { var f; var e = t.map, n = t.yearSliderOwners, r = t.inputKey, i = t.kind, o = t.attrs, s = e.get(r); if (s)
        return s.kind = i, s; var l = new Date, a = { kind: i, year2: Ot(l.getFullYear() - 2e3, 0, 99), month: Ot(l.getMonth() + 1, 1, 12), weekIndex: 1, hour: Ot(l.getHours(), 0, 23), minute: Ot(l.getMinutes(), 0, 59), second: Ot(l.getSeconds(), 0, 59), openPanel: null, openYear: !1, openMonthGrid: !1, yearSliderKey: "".concat(r, ":year-slider") }, h = String((f = o == null ? void 0 : o.value) != null ? f : ""); if (h.trim().length > 0) {
        if (i === "time") {
            var d = uo(h);
            d && (a.hour = d.hour, a.minute = d.minute, a.second = d.second);
        }
        else if (i === "month") {
            var d = ho(h);
            d && (a.year2 = d.year2, a.month = d.month);
        }
        else if (i === "week") {
            var d = fo(h);
            d && (a.year2 = d.year2, a.month = d.month, a.weekIndex = d.weekIndex);
        }
        else if (i === "date") {
            var d = mo(h);
            d && (a.year2 = d.year2, a.month = d.month, a.weekIndex = d.weekIndex);
        }
        else if (i === "datetime-local") {
            var d = po(h);
            d && (a.year2 = d.year2, a.month = d.month, a.weekIndex = d.weekIndex, a.hour = d.hour, a.minute = d.minute, a.second = d.second);
        }
    } return e.set(r, a), n.set(a.yearSliderKey, r), a; }
    function ii(t, e, n) { t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0), t.setHeight(36), t.setMinHeight(36), t.setMinWidth(n === "datetime-local" ? 340 : 220); }
    function xo(t, e, n, r, i) { var o = i != null ? 2 : 1, s = o / 2; e.control.radius > 0 ? t.roundRect(s, s, Math.max(0, n - o), Math.max(0, r - o), e.control.radius) : t.rect(s, s, Math.max(0, n - o), Math.max(0, r - o)), t.fill(e.control.background), t.stroke({ width: o, color: i != null ? i : e.control.border }); }
    function ri(t, e, n, r, i) { var o = e + r / 2, s = n + r / 2; t.moveTo(e, s - 2), t.lineTo(o, s + 2), t.lineTo(e + r, s - 2), t.stroke({ width: 2, color: i }); }
    function oi(t) { var w, T; var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.absX, l = t.absY, a = t.theme, h = t.uiState, f = t.getPointerId, d = t.getCursorColor, b = t.temporalStates, c = t.yearSliderOwners, E = t.getOrInitInputValue, p = t.requestPaint, y = t.popupSink, k = e.key; if (!k || !e.tagName)
        return; var M = e.tagName === "timeinput" ? "time" : e.tagName === "monthinput" ? "month" : e.tagName === "weekinput" ? "week" : e.tagName === "dateinput" ? "date" : "datetime-local", _ = bo({ map: b, yearSliderOwners: c, inputKey: k, kind: M, attrs: e.attrs }), B = E(k, ge(ne({}, (w = e.attrs) != null ? w : {}), { type: "text" })); M === "time" ? B.value = Le(_) : M === "month" ? B.value = rn(_) : M === "week" ? B.value = on(_) : M === "date" ? B.value = Se(_) : B.value = Ge(_); var R = (function () {
        var e_18, _a;
        var $ = h.keyboardOwnerPointerId;
        if (h.focusedKeyByPointer.get($) === k)
            return $;
        try {
            for (var _b = __values(h.focusedKeyByPointer.entries()), _c = _b.next(); !_c.done; _c = _b.next()) {
                var _d = __read(_c.value, 2), C = _d[0], F = _d[1];
                if (F === k)
                    return C;
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
    })(), H = R != null ? d(R) : null; xo(r, a, i, o, H); var D = 8; if (M !== "datetime-local") {
        var $ = (T = B.value) != null ? T : "", C = Ct(n, "__shown", function (P) { P.style = { fontFamily: a.fontFamily, fontSize: a.fontSize, fill: a.text, fontWeight: "400", wordWrap: !1 }; });
        C.text = $, C.visible = !0, C.position.set(D, 9 + xt);
        var F = n.getChildByLabel ? n.getChildByLabel("__date") : n.children.find(function (P) { return (P == null ? void 0 : P.label) === "__date"; }), m = n.getChildByLabel ? n.getChildByLabel("__time") : n.children.find(function (P) { return (P == null ? void 0 : P.label) === "__time"; });
        F && (F.visible = !1), m && (m.visible = !1), ri(r, Math.max(0, i - 18), 11, 10, a.text);
    }
    else {
        var $ = Math.max(0, Math.round(i * .52));
        r.moveTo($ + .5, 0), r.lineTo($ + .5, o), r.stroke({ width: 1, color: a.control.border, alignment: 0 });
        var C = Se(_), F = Le(_), m = Ct(n, "__date", function (et) { et.style = { fontFamily: a.fontFamily, fontSize: a.fontSize, fill: a.text, fontWeight: "400", wordWrap: !1 }; });
        m.text = C, m.visible = !0, m.position.set(D, 9 + xt);
        var P = Ct(n, "__time", function (et) { et.style = { fontFamily: a.fontFamily, fontSize: a.fontSize, fill: a.text, fontWeight: "400", wordWrap: !1 }; });
        P.text = F, P.visible = !0, P.position.set($ + D, 9 + xt);
        var v = n.getChildByLabel ? n.getChildByLabel("__shown") : n.children.find(function (et) { return (et == null ? void 0 : et.label) === "__shown"; });
        v && (v.visible = !1), ri(r, Math.max($ + 0, $ + (i - $) - 18), 11, 10, a.text);
    } Ut(n), n.eventMode = "static", n.cursor = "pointer", n.hitArea = new bt(0, 0, Math.max(0, i), Math.max(0, o)), n.on("pointerdown", function ($) { var F, m, P; if (($ == null ? void 0 : $.button) === 2)
        return; var C = f($); if (!(C <= 0)) {
        if (h.focusedKeyByPointer.set(C, k), h.keyboardOwnerPointerId = C, M !== "datetime-local")
            _.openPanel = _.openPanel ? null : M === "time" ? "time" : M === "month" ? "month" : "week", _.openYear = !1, _.openMonthGrid = !1;
        else {
            var q = ((m = (F = $.global) == null ? void 0 : F.x) != null ? m : 0) - s <= i * .52;
            _.openPanel = q ? _.openPanel === "week" ? null : "week" : _.openPanel === "time" ? null : "time", _.openYear = !1, _.openMonthGrid = !1;
        }
        b.set(k, _), p == null || p(), (P = $.stopPropagation) == null || P.call($);
    } }), _.openPanel === "month" ? y.push({ kind: "month-panel", inputKey: k, absX: s, absY: l, anchorW: i, anchorH: o }) : _.openPanel === "week" ? y.push({ kind: "week-panel", inputKey: k, absX: s, absY: l, anchorW: i, anchorH: o }) : _.openPanel === "time" && y.push({ kind: "time-panel", inputKey: k, absX: s, absY: l, anchorW: i, anchorH: o }); }
    function ve(t, e, n, r) { t.rect(0, 0, n, r), t.fill(e.control.background), t.rect(.5, .5, Math.max(0, n - 1), Math.max(0, r - 1)), t.stroke({ width: 1, color: e.control.border, alignment: 0 }); }
    function yo(t) { var e = t.stage, n = t.theme, r = t.popup, i = t.st, o = t.viewportW, s = t.viewportH, l = t.getPointerId, a = t.requestPaint, h = t.onPick, f = 4, d = 3, b = 44, c = 34, E = 8, p = E * 2 + f * b, y = E * 2 + d * c, k = r.absX, M = r.absY + r.anchorH; k = Math.max(0, Math.min(k, Math.max(0, o - p))), M + y > s - 4 && (M = r.absY - y), M = Math.max(0, Math.min(M, Math.max(0, s - y))); var _ = new wt; _.position.set(k, M), e.addChild(_); var B = new _t; ve(B, n, p, y), _.addChild(B); for (var R = 0; R < 12; R++) {
        var H = R + 1, D = E + R % f * b, w = E + Math.floor(R / f) * c;
        if (H === i.month) {
            var $ = new _t;
            $.rect(D + 1, w + 1, b - 2, c - 2), $.fill({ color: 0, alpha: .06 }), _.addChild($);
        }
        var T = jt({ text: String(H), fontFamily: n.fontFamily, fontSize: n.fontSize, fill: n.text, wordWrap: !1 });
        T.position.set(D + 14, w + 8 + xt), _.addChild(T), B.rect(D, w, b, c), B.stroke({ width: 1, color: n.control.border, alignment: 0 });
    } _.eventMode = "static", _.cursor = "pointer", _.hitArea = new bt(0, 0, p, y), _.on("pointerdown", function (R) { var et, q, nt; if ((R == null ? void 0 : R.button) === 2 || l(R) <= 0)
        return; var D = _.toLocal(R.global), w = (et = D == null ? void 0 : D.x) != null ? et : -1, T = (q = D == null ? void 0 : D.y) != null ? q : -1, $ = w - E, C = T - E; if ($ < 0 || C < 0)
        return; var F = Math.floor($ / b), m = Math.floor(C / c); if (F < 0 || F >= f || m < 0 || m >= d)
        return; var v = m * f + F + 1; v < 1 || v > 12 || (h(v), a == null || a(), (nt = R.stopPropagation) == null || nt.call(R)); }); }
    function _o(t) {
        var e_19, _a;
        var e = t.stage, n = t.theme, r = t.popup, i = t.st, o = t.viewportW, s = t.viewportH, l = t.sliders, a = t.sliderBounds, h = t.sliderDrags, f = t.getPointerId, d = t.requestPaint, b = t.onChange, c = 10, E = 250, p = 78, y = r.absX, k = r.absY;
        y = r.absX + r.anchorW + 6, k = r.absY, y = Math.max(0, Math.min(y, Math.max(0, o - E))), k = Math.max(0, Math.min(k, Math.max(0, s - p)));
        var M = new wt;
        M.position.set(y, k), e.addChild(M);
        var _ = new _t;
        ve(_, n, E, p), M.addChild(_);
        var B = jt({ text: "20".concat(zt(i.year2)), fontFamily: n.fontFamily, fontSize: n.fontSize, fill: n.text, wordWrap: !1 });
        B.position.set(c, 8 + xt), M.addChild(B);
        var R = i.yearSliderKey, H = Math.max(0, Math.min(1, Ot(i.year2, 0, 99) / 99)), D = ye(l, R, { value: String(H) }), w = !1;
        try {
            for (var _b = __values(h.values()), _c = _b.next(); !_c.done; _c = _b.next()) {
                var F = _c.value;
                if (F.key === R) {
                    w = !0;
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
        w || (D.value = H);
        var T = new wt;
        T.position.set(c, 40), M.addChild(T);
        var $ = new _t;
        T.addChild($), Ze({ node: { key: R, attrs: { value: String(D.value) } }, container: T, graphics: $, w: E - c * 2, h: 14, absX: y + c, absY: k + 40, theme: { text: n.text, control: { progress: n.control.progress } }, sliderStates: l, sliderBounds: a, sliderDrags: h, requestPaint: d, getPointerId: f });
        var C = Ot(Math.round(D.value * 99), 0, 99);
        C !== i.year2 && b(C), M.eventMode = "static", M.hitArea = new bt(0, 0, E, p), M.on("pointerdown", function (F) { var m; (m = F.stopPropagation) == null || m.call(F); });
    }
    function wo(t) { var e = t.panel, n = t.theme, r = t.x, i = t.y, o = t.w, s = t.st, l = t.onPick, a = 30, h = 6, f = []; for (var d = 0; d < 4; d++) {
        var b = d + 1, c = i + d * (a + h), E = new _t;
        E.rect(r, c, o, a), E.fill({ color: 0, alpha: b === s.weekIndex ? .06 : .03 }), E.rect(r + .5, c + .5, Math.max(0, o - 1), Math.max(0, a - 1)), E.stroke({ width: 1, color: n.control.border, alignment: 0 }), e.addChild(E);
        var p = (Ot(s.month, 1, 12) - 1) * 4 + b, y = jt({ text: "".concat(b, " [").concat(zt(p), "]"), fontFamily: n.fontFamily, fontSize: n.fontSize, fill: n.text, wordWrap: !1 });
        y.position.set(r + 10, c + 7 + xt), e.addChild(y), f.push({ x: r, y: c, w: o, h: a, weekIndex: b });
    } return { hitRects: f }; }
    function si(t) {
        var e_20, _a, e_21, _b;
        var M, _, B, R, H, D;
        var e = t.popups, n = t.stage, r = t.theme, i = t.viewportW, o = t.viewportH, s = t.temporalStates, l = t.getOrInitInputValue, a = t.sliders, h = t.sliderBounds, f = t.sliderDrags, d = t.selects, b = t.selectPopups, c = t.getCursorColor, E = t.uiFocus, p = t.getPointerId, y = t.requestPaint, k = [];
        var _loop_1 = function (w) {
            var T = s.get(w.inputKey);
            if (T) {
                if (w.kind === "month-panel") {
                    var Q = w.absX, j = w.absY + w.anchorH;
                    Q = Math.max(0, Math.min(Q, Math.max(0, i - 196))), j + 156 > o - 4 && (j = w.absY - 156), j = Math.max(0, Math.min(j, Math.max(0, o - 156)));
                    var G_1 = new wt;
                    G_1.position.set(Q, j), n.addChild(G_1);
                    var U = new _t;
                    ve(U, r, 196, 156), G_1.addChild(U);
                    var g_1 = { x: 10, y: 10, w: 132, h: 24 };
                    {
                        var x = new _t;
                        x.rect(g_1.x, g_1.y, g_1.w, g_1.h), x.fill({ color: 0, alpha: .03 }), x.rect(g_1.x + .5, g_1.y + .5, Math.max(0, g_1.w - 1), Math.max(0, g_1.h - 1)), x.stroke({ width: 1, color: r.control.border, alignment: 0 }), G_1.addChild(x);
                        var I = jt({ text: "Year 20".concat(zt(T.year2)), fontFamily: r.fontFamily, fontSize: r.fontSize, fill: r.text, wordWrap: !1 });
                        I.position.set(g_1.x + 8, g_1.y + 4 + xt), G_1.addChild(I);
                    }
                    var S_1 = 10, X_1 = 44;
                    for (var x = 0; x < 12; x++) {
                        var I = x + 1, A = S_1 + x % 4 * 44, O = X_1 + Math.floor(x / 4) * 34;
                        if (I === T.month) {
                            var K = new _t;
                            K.rect(A + 1, O + 1, 42, 32), K.fill({ color: 0, alpha: .06 }), G_1.addChild(K);
                        }
                        var L = jt({ text: String(I), fontFamily: r.fontFamily, fontSize: r.fontSize, fill: r.text, wordWrap: !1 });
                        L.position.set(A + 14, O + 8 + xt), G_1.addChild(L), U.rect(A, O, 44, 34), U.stroke({ width: 1, color: r.control.border, alignment: 0 });
                    }
                    G_1.eventMode = "static", G_1.cursor = "pointer", G_1.hitArea = new bt(0, 0, 196, 156), G_1.on("pointerdown", function (x) { var mt, Tt, Mt, Gt; if ((x == null ? void 0 : x.button) === 2)
                        return; var I = p(x); if (I <= 0)
                        return; E.focusedKeyByPointer.set(I, w.inputKey), E.keyboardOwnerPointerId = I; var A = G_1.toLocal(x.global), O = (mt = A == null ? void 0 : A.x) != null ? mt : -1, L = (Tt = A == null ? void 0 : A.y) != null ? Tt : -1; if (O >= g_1.x && O <= g_1.x + g_1.w && L >= g_1.y && L <= g_1.y + g_1.h) {
                        T.openYear = !0, s.set(w.inputKey, T), y == null || y(), (Mt = x.stopPropagation) == null || Mt.call(x);
                        return;
                    } var Y = O - S_1, N = L - X_1; if (Y < 0 || N < 0)
                        return; var z = Math.floor(Y / 44), V = Math.floor(N / 34); if (z < 0 || z >= 4 || V < 0 || V >= 3)
                        return; var rt = V * 4 + z + 1; if (rt < 1 || rt > 12)
                        return; T.month = rt, T.openPanel = null, T.openYear = !1, T.openMonthGrid = !1, s.set(w.inputKey, T); var st = l(w.inputKey, { type: "text" }); st.value = rn(T), y == null || y(), (Gt = x.stopPropagation) == null || Gt.call(x); }), G_1.on("pointerdown", function (x) { var I; (I = x.stopPropagation) == null || I.call(x); }), T.openYear && k.push({ kind: "year-panel", inputKey: w.inputKey, absX: Q, absY: j, anchorW: 196, anchorH: 0 });
                }
                if (w.kind === "week-panel") {
                    var m = w.absX, P = w.absY + w.anchorH;
                    m = Math.max(0, Math.min(m, Math.max(0, i - 280))), P + 192 > o - 4 && (P = w.absY - 192), P = Math.max(0, Math.min(P, Math.max(0, o - 192)));
                    var v_1 = new wt;
                    v_1.position.set(m, P), n.addChild(v_1);
                    var et = new _t;
                    ve(et, r, 280, 192), v_1.addChild(et);
                    var q_1 = { x: 10, y: 10, w: 104, h: 24 }, nt_1 = { x: 10 + q_1.w + 10, y: 10, w: 120, h: 24 }, Q = function (U, g) { var S = new _t; S.rect(U.x, U.y, U.w, U.h), S.fill({ color: 0, alpha: .03 }), S.rect(U.x + .5, U.y + .5, Math.max(0, U.w - 1), Math.max(0, U.h - 1)), S.stroke({ width: 1, color: r.control.border, alignment: 0 }), v_1.addChild(S); var X = jt({ text: g, fontFamily: r.fontFamily, fontSize: r.fontSize, fill: r.text, wordWrap: !1 }); X.position.set(U.x + 8, U.y + 4 + xt), v_1.addChild(X); };
                    Q(q_1, "Month ".concat(T.month)), Q(nt_1, "Year 20".concat(zt(T.year2)));
                    var j = 44, G_2 = wo({ panel: v_1, theme: r, x: 10, y: j, w: 280 - 10 * 2, st: T, onPick: function () { } }).hitRects;
                    v_1.eventMode = "static", v_1.cursor = "pointer", v_1.hitArea = new bt(0, 0, 280, 192), v_1.on("pointerdown", function (U) {
                        var e_23, _a;
                        var A, O, L, K, Y;
                        if ((U == null ? void 0 : U.button) === 2)
                            return;
                        var g = p(U);
                        if (g <= 0)
                            return;
                        E.focusedKeyByPointer.set(g, w.inputKey), E.keyboardOwnerPointerId = g;
                        var S = v_1.toLocal(U.global), X = (A = S == null ? void 0 : S.x) != null ? A : -1, x = (O = S == null ? void 0 : S.y) != null ? O : -1, I = function (N) { return X >= N.x && X <= N.x + N.w && x >= N.y && x <= N.y + N.h; };
                        if (I(q_1)) {
                            T.openMonthGrid = !T.openMonthGrid, s.set(w.inputKey, T), y == null || y(), (L = U.stopPropagation) == null || L.call(U);
                            return;
                        }
                        if (I(nt_1)) {
                            T.openYear = !0, s.set(w.inputKey, T), y == null || y(), (K = U.stopPropagation) == null || K.call(U);
                            return;
                        }
                        try {
                            for (var G_3 = (e_23 = void 0, __values(G_2)), G_3_1 = G_3.next(); !G_3_1.done; G_3_1 = G_3.next()) {
                                var N = G_3_1.value;
                                if (I(N)) {
                                    T.weekIndex = N.weekIndex;
                                    var z = l(w.inputKey, { type: "text" });
                                    T.kind === "week" ? z.value = on(T) : T.kind === "date" ? z.value = Se(T) : z.value = Ge(T), T.openPanel = null, T.openYear = !1, T.openMonthGrid = !1, s.set(w.inputKey, T), y == null || y(), (Y = U.stopPropagation) == null || Y.call(U);
                                    return;
                                }
                            }
                        }
                        catch (e_23_1) { e_23 = { error: e_23_1 }; }
                        finally {
                            try {
                                if (G_3_1 && !G_3_1.done && (_a = G_3.return)) _a.call(G_3);
                            }
                            finally { if (e_23) throw e_23.error; }
                        }
                    }), T.openMonthGrid && k.push({ kind: "month-grid", inputKey: w.inputKey, absX: m, absY: P + q_1.y + q_1.h + 4, anchorW: 0, anchorH: 0 }), T.openYear && k.push({ kind: "year-panel", inputKey: w.inputKey, absX: m + nt_1.x, absY: P + nt_1.y, anchorW: nt_1.w, anchorH: 0 });
                }
                if (w.kind === "time-panel") {
                    var m_1 = w.absX, P_1 = w.absY + w.anchorH;
                    m_1 = Math.max(0, Math.min(m_1, Math.max(0, i - 330))), P_1 + 80 > o - 4 && (P_1 = w.absY - 80), P_1 = Math.max(0, Math.min(P_1, Math.max(0, o - 80)));
                    var v_2 = new wt;
                    v_2.position.set(m_1, P_1), n.addChild(v_2);
                    var et = new _t;
                    ve(et, r, 330, 80), v_2.addChild(et);
                    var q = jt({ text: "Time", fontFamily: r.fontFamily, fontSize: r.fontSize, fill: r.text, wordWrap: !1 });
                    q.position.set(10, 8 + xt), v_2.addChild(q);
                    var nt_2 = function (V) { return Array.from({ length: V }, function (J, rt) { return zt(rt); }).join("\n"); }, Q = w.inputKey, j = "".concat(Q, ":time-h"), G = "".concat(Q, ":time-m"), U = "".concat(Q, ":time-s"), g = de(d, j, Ot(T.hour, 0, 23)), S = de(d, G, Ot(T.minute, 0, 59)), X = de(d, U, Ot(T.second, 0, 59));
                    g.selectedIndex = Ot(T.hour, 0, 23), S.selectedIndex = Ot(T.minute, 0, 59), X.selectedIndex = Ot(T.second, 0, 59);
                    var x_1 = 96, I_2 = 36, A_1 = 32, O = 8, L = function (V, J, rt) { var st = new wt; st.position.set(J, A_1), v_2.addChild(st); var mt = new _t; st.addChild(mt), nn({ node: { key: V, attrs: { "data-options": nt_2(rt), "data-selected-index": String(de(d, V, 0).selectedIndex) } }, container: st, graphics: mt, w: x_1, h: I_2, absX: m_1 + J, absY: P_1 + A_1, theme: r, selectStates: d, uiState: E, getPointerId: p, getCursorColor: c, requestPaint: y, popupSink: b }); };
                    L(j, 10, 24), L(G, 10 + x_1 + O, 60), L(U, 10 + (x_1 + O) * 2, 60);
                    var K = Ot((_ = (M = d.get(j)) == null ? void 0 : M.selectedIndex) != null ? _ : T.hour, 0, 23), Y = Ot((R = (B = d.get(G)) == null ? void 0 : B.selectedIndex) != null ? R : T.minute, 0, 59), N = Ot((D = (H = d.get(U)) == null ? void 0 : H.selectedIndex) != null ? D : T.second, 0, 59);
                    T.hour = K, T.minute = Y, T.second = N, s.set(w.inputKey, T);
                    var z = l(w.inputKey, { type: "text" });
                    T.kind === "time" ? z.value = Le(T) : z.value = Ge(T), v_2.eventMode = "static", v_2.hitArea = new bt(0, 0, 330, 80), v_2.on("pointerdown", function (V) { var J; (J = V.stopPropagation) == null || J.call(V); });
                }
            }
        };
        try {
            for (var e_22 = __values(e), e_22_1 = e_22.next(); !e_22_1.done; e_22_1 = e_22.next()) {
                var w = e_22_1.value;
                _loop_1(w);
            }
        }
        catch (e_20_1) { e_20 = { error: e_20_1 }; }
        finally {
            try {
                if (e_22_1 && !e_22_1.done && (_a = e_22.return)) _a.call(e_22);
            }
            finally { if (e_20) throw e_20.error; }
        }
        var _loop_2 = function (w) {
            var T = s.get(w.inputKey);
            T && (w.kind === "month-grid" && yo({ stage: n, theme: r, popup: w, st: T, viewportW: i, viewportH: o, getPointerId: p, requestPaint: y, onPick: function ($) { T.month = $, T.openMonthGrid = !1, s.set(w.inputKey, T); var C = l(w.inputKey, { type: "text" }); T.kind === "month" ? C.value = rn(T) : T.kind === "week" ? C.value = on(T) : T.kind === "date" ? C.value = Se(T) : C.value = Ge(T); } }), w.kind === "year-panel" && _o({ stage: n, theme: r, popup: w, st: T, viewportW: i, viewportH: o, sliders: a, sliderBounds: h, sliderDrags: f, getPointerId: p, requestPaint: y, onChange: function ($) { T.year2 = $, s.set(w.inputKey, T); var C = l(w.inputKey, { type: "text" }); T.kind === "month" ? C.value = rn(T) : T.kind === "week" ? C.value = on(T) : T.kind === "date" ? C.value = Se(T) : T.kind === "time" ? C.value = Le(T) : C.value = Ge(T); } }));
        };
        try {
            for (var k_1 = __values(k), k_1_1 = k_1.next(); !k_1_1.done; k_1_1 = k_1.next()) {
                var w = k_1_1.value;
                _loop_2(w);
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
    var li = 5e4, Be = new WeakMap, ui = new Map, To = 1, di = 0, Mo = 0, ci = !1, _e = [], Pn = null;
    function We(t) { return t instanceof _t ? "Graphics" : t instanceof qt ? "Text" : t instanceof wt ? "Container" : "Object"; }
    function Eo(t) { var e = t && typeof t == "object" ? t.label : void 0, n = t && typeof t == "object" ? We(t) : "Object"; return e ? "".concat(n, ":").concat(String(e).slice(0, 80)) : n; }
    function he(t) { var e = Be.get(t); return e || (e = To++, Be.set(t, e)), ui.set(e, t), e; }
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
    function Co(t) { var s, l, a, h, f, d; if (!t || typeof t != "object")
        return; var e = t, n = Number((s = e.x) != null ? s : 0), r = Number((l = e.y) != null ? l : 0), i = Number((h = (a = e.width) != null ? a : e.w) != null ? h : 0), o = Number((d = (f = e.height) != null ? f : e.h) != null ? d : 0); if (!(!Number.isFinite(n) || !Number.isFinite(r) || !Number.isFinite(i) || !Number.isFinite(o)) && !(i <= 0 || o <= 0))
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
    function Ro(t) { if (_e.push(t), t.op === "snapshot") {
        Fe();
        return;
    } if (_e.length >= 512) {
        Fe();
        return;
    } Pn == null && (Pn = window.setTimeout(function () { Pn = null, Fe(); }, 50)); }
    function Fe() {
        if (_e.length === 0)
            return;
        var t = _e;
        _e = [];
        var e = t.map(function (n) { return JSON.stringify(n); }).join("\n") + "\n";
        navigator.sendBeacon && navigator.sendBeacon("/__pixi_capture", new Blob([e], { type: "application/x-ndjson" })) || fetch("/__pixi_capture", { method: "POST", headers: { "Content-Type": "application/x-ndjson" }, body: e, keepalive: !0 }).catch(function () { _e = t.concat(_e); });
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
                    var h = a_1_1.value;
                    h.parent = null;
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
        var E, p, y, k, M, _, B, R, H, D, w, T, $, C;
        var l = function (F) { try {
            window.__TRUEOS_PIXI_POINTER_DISPATCH_STEP__ = F, typeof console != "undefined" && typeof console.log == "function" && console.log("[trueos pointer dispatch] ".concat(F));
        }
        catch (m) { } };
        l("start node=".concat(Number(t) || 0, " event=").concat(String(e || "")));
        var a = window.__TRUEOS_PIXI_APP;
        if (String(e || "") === "wheel") {
            var F = a == null ? void 0 : a.canvas;
            if (!F || typeof F.dispatchEvent != "function")
                return l("wheel-canvas-missing"), { handled: 0, listenerCount: 0, painted: 0, targetFound: 0 };
            var m = (y = (p = (E = window.__pixiCapture) == null ? void 0 : E.commands) == null ? void 0 : p.length) != null ? y : 0, P = { type: "wheel", deltaX: 0, deltaY: Number(s) || 0, deltaMode: 0, offsetX: Number(n) || 0, offsetY: Number(r) || 0, clientX: Number(n) || 0, clientY: Number(r) || 0, pointerId: Number(i) || 1, buttons: Number(o) || 0, defaultPrevented: !1, propagationStopped: !1, preventDefault: function () { this.defaultPrevented = !0; }, stopPropagation: function () { this.propagationStopped = !0; } };
            l("wheel-dispatch deltaY=".concat(P.deltaY)), F.dispatchEvent(P);
            var v = 0;
            if (window.__TRUEOS_CAPTURE_ONLY__) {
                var j = window.__TRUEOS_REPAINT_NOW__;
                window.__TRUEOS_PIXI_DIRTY__ && typeof j == "function" && (l("wheel-repaint-call"), j(), l("wheel-repaint-return"), v = 1);
            }
            else
                (k = a == null ? void 0 : a.renderer) != null && k.render && (a != null && a.stage) && (a.renderer.render(a.stage), v = 1);
            var et = (B = (_ = (M = window.__pixiCapture) == null ? void 0 : M.commands) == null ? void 0 : _.length) != null ? B : m, q = (R = F.listeners) == null ? void 0 : R.wheel, nt = Array.isArray(q) ? q.length : typeof q == "function" ? 1 : 0, Q = P.defaultPrevented || nt > 0 ? 1 : 0;
            return l("wheel-done handled=".concat(Q, " listeners=").concat(nt, " painted=").concat(v)), { handled: Q, listenerCount: nt, painted: et > m || v ? 1 : 0, targetFound: 1 };
        }
        var h = ui.get(Number(t) || 0), f = 0, d = 0, b = 0;
        if (!h)
            return l("target-missing"), { handled: f, listenerCount: d, painted: b, targetFound: 0 };
        var c = { type: String(e || ""), button: Number(o) & 2 ? 2 : 0, buttons: Number(o) || 0, pointerId: Number(i) || 1, pointerType: "mouse", global: { x: Number(n) || 0, y: Number(r) || 0 }, data: { pointerId: Number(i) || 1, pointerType: "mouse", global: { x: Number(n) || 0, y: Number(r) || 0 } }, target: h, currentTarget: h, defaultPrevented: !1, propagationStopped: !1, preventDefault: function () { this.defaultPrevented = !0; }, stopPropagation: function () { this.propagationStopped = !0; } };
        l("target-found label=".concat(String((H = h.label) != null ? H : "")));
        for (var F = h; F; F = F.parent) {
            c.currentTarget = F;
            var m = (D = F.listeners) == null ? void 0 : D[c.type];
            if (!(!Array.isArray(m) || m.length === 0)) {
                d += m.length, l("listeners node=".concat((w = Be.get(F)) != null ? w : 0, " count=").concat(m.length));
                try {
                    for (var _b = (e_29 = void 0, __values(m.slice())), _c = _b.next(); !_c.done; _c = _b.next()) {
                        var P = _c.value;
                        if (typeof P == "function" && (f = 1, l("listener-call node=".concat((T = Be.get(F)) != null ? T : 0)), P.call(F, c), l("listener-return node=".concat(($ = Be.get(F)) != null ? $ : 0)), c.propagationStopped))
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
            var F = window.__TRUEOS_REPAINT_NOW__;
            window.__TRUEOS_PIXI_DIRTY__ && typeof F == "function" && (l("capture-repaint-call"), F(), l("capture-repaint-return"), b = 1);
        }
        else
            (C = a == null ? void 0 : a.renderer) != null && C.render && (a != null && a.stage) && (l("paint-call"), a.renderer.render(a.stage), l("paint-return"), b = 1);
        return l("done handled=".concat(f, " listeners=").concat(d, " painted=").concat(b)), { handled: f, listenerCount: d, painted: b, targetFound: 1 };
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
                catch (h) { }
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
        } var h = this; n === "text.text.set" ? h._text = String(a != null ? a : "") : n === "text.style.set" ? h._style = a != null ? a : {} : n === "text.resolution.set" ? h._resolution = Math.max(1, Number(a) || 1) : Object.defineProperty(h, i, { configurable: !0, enumerable: !1, writable: !0, value: a }); } }), t.constructor["__pixiCapturePatched_".concat(n)] = !0; }
    function mi(t, e) {
        if (e === void 0) { e = 0; }
        var s, l, a, h, f, d, b, c, E;
        if (!t || e > 64)
            return null;
        var n, r;
        try {
            var p = typeof t.getGlobalPosition == "function" ? t.getGlobalPosition() : null;
            p && Number.isFinite(Number(p.x)) && Number.isFinite(Number(p.y)) && (n = Number(p.x), r = Number(p.y));
        }
        catch (p) { }
        var i = { id: he(t), type: We(t), label: (s = t.label) != null ? s : void 0, x: (h = (a = (l = t.position) == null ? void 0 : l.x) != null ? a : t.x) != null ? h : 0, y: (b = (d = (f = t.position) == null ? void 0 : f.y) != null ? d : t.y) != null ? b : 0, globalX: n, globalY: r, scaleX: Number.isFinite(Number((c = t.scale) == null ? void 0 : c.x)) ? Number(t.scale.x) : 1, scaleY: Number.isFinite(Number((E = t.scale) == null ? void 0 : E.y)) ? Number(t.scale.y) : 1, visible: t.visible, alpha: Number.isFinite(Number(t.alpha)) ? Number(t.alpha) : 1, maskId: t.mask ? he(t.mask) : 0, zIndex: Number(t.zIndex) || 0, sortableChildren: t.sortableChildren === !0 }, o = Co(t.hitArea);
        return o && (i.hitArea = o), typeof t.text == "string" && (i.text = t.text.slice(0, 120)), Array.isArray(t.children) && t.children.length && (i.children = t.children.map(function (p) { return mi(p, e + 1); })), i;
    }
    function fi() {
        var e_30, _a, e_31, _b;
        if (window.__pixiCapture)
            return window.__pixiCapture;
        var t = { enabled: !0, persist: !window.__TRUEOS_CAPTURE_ONLY__, commands: [], counts: Object.create(null), clear: function () { this.commands.length = 0, this.counts = Object.create(null); }, dump: function (e) {
                if (e === void 0) { e = 200; }
                return this.commands.slice(-e);
            }, flush: function () { Fe(); }, summary: function () { return ne({}, this.counts); } };
        if (window.__pixiCapture = t, Ao(), window.addEventListener("beforeunload", function () { return Fe(); }), !ci) {
            ci = !0, typeof _t.prototype.image != "function" && (_t.prototype.image = function () { return this; });
            try {
                for (var _c = __values(["clear", "rect", "roundRect", "circle", "ellipse", "moveTo", "lineTo", "closePath", "poly", "fill", "stroke", "image", "svg"]), _d = _c.next(); !_d.done; _d = _c.next()) {
                    var e = _d.value;
                    Cn(_t.prototype, e);
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
                    Cn(wt.prototype, e);
                }
            }
            catch (e_31_1) { e_31 = { error: e_31_1 }; }
            finally {
                try {
                    if (_g && !_g.done && (_b = _f.return)) _b.call(_f);
                }
                finally { if (e_31) throw e_31.error; }
            }
            Ie(qt.prototype, "text", "text.text.set"), Ie(qt.prototype, "style", "text.style.set"), Ie(qt.prototype, "resolution", "text.resolution.set"), Cn(qt.prototype, "setSize", "text.setSize"), Ie(wt.prototype, "visible", "visible"), Ie(wt.prototype, "alpha", "alpha"), Ie(wt.prototype, "mask", "mask");
        }
        return t;
    }
    function pi(t) { var e = t.renderer, n = e == null ? void 0 : e.render; if (typeof n != "function" || n.__pixiCapturePatched)
        return; var r = function (o) { var s = o && typeof o == "object" && "container" in o ? o.container : o || t.stage; return di++, ln(s, "render", []), ln(s, "snapshot", [mi(s)]), window.__TRUEOS_CAPTURE_ONLY__ ? s : n.call(this, o); }; r.__pixiCapturePatched = !0, e.render = r; }
    fi();
    var it = null, An = 6, Pe = 10, Wt = 1, Ht = 3, Xt = 4, Ce = 512, wi = new Map;
    var u = { focusedKeyByPointer: new Map, keyboardOwnerPointerId: 1, inputs: new Map, sliders: new Map, sliderDrags: new Map, sliderBounds: new Map, dialogs: new Map, dialogDrags: new Map, dialogSelectedBy: new Map, dialogZ: new Map, dialogZCounter: 1, numbers: new Map, numberHolds: new Map, selects: new Map, temporals: new Map, temporalYearOwners: new Map, color: { rgb: { r: 255, g: 0, b: 0 }, a: 255, pick: null, draggingPointerId: null, bounds: null }, cursorColors: new Map, primaryMousePointerId: 1, harness: { enabled: !0, activeUserPointerId: Wt, periodMs: 3e3 }, userCursorPos: new Map, lastMouse: { x: 0, y: 0, has: !1 }, scroll: { y: 0, contentHeight: 0, viewportHeight: 0, draggingPointerId: null, dragOffsetY: 0, track: { x: 0, y: 0, w: Pe, h: 0 }, thumb: { x: 0, y: 0, w: Pe, h: 0 } }, iframeScroll: new Map, iframeRects: [], hoverRects: [], hoverHandlers: new Map, hoveredKeyByPointer: new Map, hoveredCursorByPointer: new Map, virtualCursor: { enabled: !1, x: 0, y: 0, t: 0, radius: 120, speed: .9 }, textDrags: new Map, fieldBounds: new Map, dialogDragBounds: new Map, detailsOpen: new Map, contextMenus: new Map, clipboards: new Map }, cn = null, Nn = 0;
    function vo(t) { if (!cn) {
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
    function Bo(t) { var e = Ue(t), n = e.indexOf("__trueos") >= 0 || e.indexOf("tsNu") >= 0 || e.indexOf("tsNum") >= 0; return e.indexOf("__TRUEOS_HOST_READY__") >= 0 && (e = e.replace(/__TRUEOS_HOST_READY__/g, "")), e.indexOf("__trueos") >= 0 && (e = Fo(e), e = e.replace(/__trueosNumberValue/g, "").replace(/__trueosHostNum/g, "").replace(/__trueosNum/g, "").replace(/__trueosNu/g, "").replace(/__trueos/g, "")), (e.indexOf("tsNu") >= 0 || e.indexOf("tsNum") >= 0) && (e = e.replace(/tsNum/g, "").replace(/tsNutsNutsNutsNu/g, "").replace(/tsNutsNutsNu/g, "").replace(/tsNutsNu/g, "").replace(/tsNu/g, "")), n && (e = Ti(e.trimStart())), e; }
    function Fo(t) { var e = "__trueosN", n = t, r = 0; for (; r < n.length;) {
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
    function Mi(t) { return Bo(t); }
    function Gn(t) { return Ti(Mi(t).trimStart()); }
    function Wo(t) { var e = me(Gn(t)); return !(e.length === 0 || e === "true" || e === "false" || e === "N" || e === "Nu" || e === "Num" || e.startsWith("<truesurfer-") || e.startsWith("__trueo")); }
    function Ei(t, e) { var r; var n = Ue(e) || "block"; t[n] = ((r = t[n]) != null ? r : 0) + 1; }
    function $o(t) {
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
    function Ho(t) { var e = { nodes: 0, blocks: 0, text: 0, maxDepth: 0, tags: {} }, n = function (r, i) {
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
    function vn(t, e) {
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
            n.push("#".concat(n.length, "@").concat(o, ":").concat(s, " chars=").concat(i.text.length, " sample=\"").concat(vn(i.text), "\""));
            return;
        } var l = Ue(i.tagName || "block") || "block", a = i.key || ""; for (var h = 0; h < i.children.length; h += 1)
            r(i.children[h], l, a); };
        for (var i = 0; i < t.length; i += 1)
            r(t[i], "root", "");
        return n.join("|");
    }
    function Xo(t, e) {
        if (e === void 0) { e = 12; }
        var n = [], r = function (i, o, s) { var h; if (n.length >= e)
            return; if (i.kind === "text") {
            var f = (h = i.text) != null ? h : "";
            n.push("#".concat(n.length, "@").concat(o, ":").concat(s, " chars=").concat(f.length, " box=").concat(Math.round(i.x), ",").concat(Math.round(i.y), ",").concat(Math.round(i.width), ",").concat(Math.round(i.height), " sample=\"").concat(vn(f), "\""));
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
    function ki(t) { var e = typeof t == "string" ? t : "", n = [], r = function (f) { var d = Dn(f); d.length !== 0 && (d.startsWith("<truesurfer-") || d.startsWith("__trueo") || n.push(d)); }, i = [], o = e.toLowerCase(), s = o.indexOf("<body"); if (s >= 0) {
        var f = e.indexOf(">", s);
        s = f >= 0 ? f + 1 : s;
    }
    else
        s = 0; var l = o.indexOf("</body>", s), a = l >= 0 ? l : e.length, h = ""; for (; s < a && n.length < Ce;) {
        var f = e.charAt(s);
        if (f !== "<") {
            h += f, s += 1;
            continue;
        }
        var d = mn(h);
        if (d.length > 0) {
            for (var _ = i.length - 1; _ >= 0; _ -= 1)
                if (i[_].wanted) {
                    i[_].text += " ".concat(d);
                    break;
                }
        }
        h = "";
        var b = e.indexOf(">", s + 1);
        if (b < 0)
            break;
        var c = e.slice(s, b + 1), E = e.slice(s + 1, b), p = Yo(E);
        if (E.trimStart().charAt(0) === "/") {
            for (var _ = i.length - 1; _ >= 0; _ -= 1) {
                var B = i.pop();
                if (B != null && B.wanted && r(B.text), (B == null ? void 0 : B.tag) === p)
                    break;
            }
            s = b + 1;
            continue;
        }
        if (p === "script" || p === "style" || p === "template") {
            var _ = "</".concat(p, ">"), B = o.indexOf(_, b + 1);
            s = B >= 0 ? B + _.length : b + 1;
            continue;
        }
        if (p === "input") {
            var _ = xi(c, "type").toLowerCase();
            (_ === "button" || _ === "submit" || _ === "reset") && r(xi(c, "value"));
        }
        var k = c.length - 1;
        for (; k >= 0 && c.charCodeAt(k) <= 32;)
            k -= 1;
        k >= 1 && c.charAt(k) === ">" && c.charAt(k - 1) === "/" || p === "input" || p === "br" || p === "hr" || p === "img" || i.push({ tag: p, wanted: Ko(p), text: "" }), s = b + 1;
    } if (h.length > 0) {
        var f = mn(h);
        for (var d = i.length - 1; d >= 0; d -= 1)
            if (i[d].wanted) {
                i[d].text += " ".concat(f);
                break;
            }
    } for (; i.length && n.length < Ce;) {
        var f = i.pop();
        f != null && f.wanted && r(f.text);
    } if (n.length === 0) {
        var f = o.indexOf("<body");
        if (f >= 0) {
            var p = e.indexOf(">", f);
            f = p >= 0 ? p + 1 : f;
        }
        else
            f = 0;
        var d = o.indexOf("</body>", f), b = d >= 0 ? d : e.length, c = !1, E = "";
        for (var p = f; p < b && n.length < Ce; p += 1) {
            var y = e.charAt(p);
            if (y === "<") {
                r(E), E = "", c = !0;
                continue;
            }
            if (y === ">") {
                c = !1;
                continue;
            }
            c || (E += y);
        }
        r(E);
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
    function He(t) { var e = []; for (var n = 0; n < t.length; n += 1) {
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
        return { source: "array", rows: s }; var l = ki(t); if ($t()) {
        var a = Array.isArray(n) && typeof n[0] == "string" ? hn(n[0], 72) : "", h = typeof e == "string" ? hn(e, 72) : "";
        console.log("[trueos pixi widgets] text-fallback-globals text_type=".concat(typeof e, " text_len=").concat(typeof e == "string" ? e.length : 0, " text_rows=").concat(o.length, " text_sample=\"").concat(h, "\" array=").concat(Array.isArray(n) ? n.length : -1, " array_rows=").concat(s.length, " array0=\"").concat(a, "\" html_len=").concat(t.length, " html_rows=").concat(l.length));
    } return { source: "html", rows: l }; }
    function ts() { var e; var t = fn("__TRUEOS_WIDGET_RENDER_TREE_JSON__"); if (typeof t == "string" && t.length > 0)
        try {
            return { source: "json", tree: JSON.parse(t) };
        }
        catch (n) {
            $t() && console.log("[trueos pixi widgets] render-tree-json parse failed err=".concat(String((e = n == null ? void 0 : n.message) != null ? e : n)));
        } return { source: "window", tree: fn("__TRUEOS_WIDGET_RENDER_TREE__") }; }
    function es(t) { var o, s, l, a; var e = [], n = String(t != null ? t : "").replace(/<script[^]*?<\/script>/gi, " ").replace(/<style[^]*?<\/style>/gi, " "), r = /<(h[1-6]|p|label|button)\b[^>]*>([^]*?)<\/\1>|<input\b[^>]*>/gi, i; for (; (i = r.exec(n)) && e.length < Ce;) {
        var h = (o = i[0]) != null ? o : "", f = String((s = i[1]) != null ? s : "").toLowerCase();
        if (h.toLowerCase().startsWith("<input"))
            continue;
        var d = Dn(f === "p" || f === "label" ? (l = i[2]) != null ? l : "" : (a = i[2]) != null ? a : "");
        d.length > 0 && e.push(d);
    } return e; }
    function ns(t) { var e = es(t), n = He(e); return He(n); }
    function rs(t, e, n, r) {
        var e_35, _a;
        var a, h, f, d, b, c;
        var i = He((h = wi.get(String((a = t.key) != null ? a : ""))) != null ? h : []), o = He(String((d = (f = t.attrs) == null ? void 0 : f["data-trueos-srcdoc-text"]) != null ? d : "").split("\n").map(function (E) { return me(E); })), s = i.length > 0 ? i : o.length > 0 ? o : ns(String((c = (b = t.attrs) == null ? void 0 : b.srcdoc) != null ? c : "")), l = n + 48;
        try {
            for (var s_2 = __values(s), s_2_1 = s_2.next(); !s_2_1.done; s_2_1 = s_2.next()) {
                var E = s_2_1.value;
                if (r.length >= Ce)
                    return;
                r.push({ x: e + 16, y: l, text: E }), l += 32;
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
        var p, y, k;
        if (e.length >= Ce)
            return;
        var l = i + r.x, a = o + r.y, h = r.kind === "block" && r.tagName === "iframe" && String((y = (p = r.attrs) == null ? void 0 : p["data-root"]) != null ? y : "") !== "1", f = s + (h ? 1 : 0), d = r.kind === "block" && r.tagName === "button", b = r.kind === "text" ? (k = r.text) != null ? k : "" : d ? Ln(r) : "", c = me(Gn(b)), E = e.length;
        if (Wo(c)) {
            var M = d ? l + 8 : l, _ = d ? a + Math.max(0, Math.floor((r.height - be.fontSize * 1.25) / 2)) : a;
            e.push({ x: M, y: _, text: c });
        }
        if (!d) {
            try {
                for (var _b = __values(r.children), _c = _b.next(); !_c.done; _c = _b.next()) {
                    var M = _c.value;
                    n(M, l, a, f);
                }
            }
            catch (e_36_1) { e_36 = { error: e_36_1 }; }
            finally {
                try {
                    if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
                }
                finally { if (e_36) throw e_36.error; }
            }
            h && e.length === E && rs(r, l, a, e);
        }
    }; return n(t, 0, 0, 0), e; }
    function os(t, e) {
        if (e === void 0) { e = 8; }
        var n = [];
        for (var r = 0; r < t.length && n.length < e; r += 1) {
            var i = t[r];
            n.push("#".concat(n.length, " x=").concat(Math.round(i.x), " y=").concat(Math.round(i.y), " text=\"").concat(vn(i.text), "\""));
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
                var h = Ue(a == null ? void 0 : a.op);
                h && (e[h] = ((s = e[h]) != null ? s : 0) + 1, r.has(h) || (n[h] = ((l = n[h]) != null ? l : 0) + 1));
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
    function as(t, e, n, r) { if (!$t())
        return; var i = ss(); window.__TRUEOS_PIXI_BRIDGE_STATS__ = { renderNodes: t.nodes, renderBlocks: t.blocks, renderText: t.text, renderTags: Rn(t.tags, 24), renderTextSamples: n, layoutBoxes: e.nodes, layoutBlocks: e.blocks, layoutText: e.text, layoutMaxDepth: e.maxDepth, layoutTextSamples: r, measureTextCalls: Nn, scrollbarVisible: u.scroll.track.h > 0 ? 1 : 0, scrollbarTrack: "".concat(Math.round(u.scroll.track.x), ",").concat(Math.round(u.scroll.track.y), ",").concat(Math.round(u.scroll.track.w), ",").concat(Math.round(u.scroll.track.h)), scrollbarThumb: "".concat(Math.round(u.scroll.thumb.x), ",").concat(Math.round(u.scroll.thumb.y), ",").concat(Math.round(u.scroll.thumb.w), ",").concat(Math.round(u.scroll.thumb.h)), pixiCommands: i.total, pixiOps: i.ops, pixiUnsupported: i.unsupported }; }
    var yi = new WeakMap;
    function Bn(t, e) { var n = t; for (; n;) {
        if (n === e)
            return !0;
        n = n.parent;
    } return !1; }
    function Si(t) { return Array.isArray(t.children) || (t.children = []), t.children; }
    function oe(t, e, n) { var r = Number(e) || 0, i = Number(n) || 0; (!t.position || typeof t.position != "object") && (t.position = { x: 0, y: 0 }), t.position.x = r, t.position.y = i; }
    function $e(t, e, n) { if (e === t || Bn(t, e))
        return; var r = Si(t); if (e.parent !== t) {
        var l = Math.max(0, Math.min(n, r.length));
        t.addChildAt(e, l);
        return;
    } var i = Math.max(0, r.length - 1), o = Math.max(0, Math.min(n, i)); t.getChildIndex(e) !== o && t.setChildIndex(e, o); }
    function _i(t, e, n) { if (e === t || Bn(t, e))
        return; var r = Si(t); if (e.parent !== t) {
        var l = Math.max(0, Math.min(n, r.length));
        t.addChildAt(e, l);
        return;
    } var i = Math.max(0, r.length - 1), o = Math.max(0, Math.min(n, i)); t.getChildIndex(e) !== o && t.setChildIndex(e, o); }
    var dn = null, ot = null;
    function Jt(t) { var e = u.cursorColors.get(t); if (e != null)
        return e; var n = [1118481, 2450411, 1483594, 14427686, 8141549, 959977, 16096779], r = Math.abs(Number(t) || 0) % n.length, i = n[r]; return u.cursorColors.set(t, i), i; }
    function Bt(t) { var i, o, s, l, a, h; var e = Number((s = (o = t == null ? void 0 : t.pointerId) != null ? o : (i = t == null ? void 0 : t.data) == null ? void 0 : i.pointerId) != null ? s : 0), r = String((h = (a = t == null ? void 0 : t.pointerType) != null ? a : (l = t == null ? void 0 : t.data) == null ? void 0 : l.pointerType) != null ? h : "").toLowerCase() === "mouse" || e === 1 || e === u.primaryMousePointerId; return u.harness.enabled && r ? u.harness.activeUserPointerId : e; }
    function $t() { return !!globalThis.__TRUEOS_CAPTURE_ONLY__; }
    function St(t) { $t() && (window.__TRUEOS_PIXI_APP_PHASE__ = t); }
    function W(t) { $t() && (window.__TRUEOS_PIXI_LAYOUT_STEP__ = t); }
    function Ii(t) { var l, a, h, f, d; var e = (l = window.__TRUEOS_PIXI_APP_PHASE__) != null ? l : "unknown", n = (a = window.__TRUEOS_PIXI_LAYOUT_STEP__) != null ? a : "", r = t, i = String((h = r == null ? void 0 : r.name) != null ? h : "Error"), o = String((f = r == null ? void 0 : r.message) != null ? f : t), s = String((d = r == null ? void 0 : r.stack) != null ? d : ""); return "phase=".concat(e, " layout=").concat(n, " name=").concat(i, " message=").concat(o, " stack=").concat(s); }
    function ls() { var t = Math.max(1, Number(window.innerWidth || 1920) | 0), e = Math.max(1, Number(window.innerHeight || 1080) | 0), n = new bt(0, 0, t, e), r = document.createElement("canvas"), i = { width: t, height: e, screen: n, render: function (o) { return o; }, resize: function (o, s) { var l = Math.max(1, Number(o || t) | 0), a = Math.max(1, Number(s || e) | 0); this.width = l, this.height = a, n.width = l, n.height = a; } }; return { stage: new wt, screen: n, canvas: r, renderer: i, ticker: { stop: function () { }, add: function () { }, remove: function () { } } }; }
    function cs() { var h = /** @class */ (function () {
        function h() {
            tt(this, "children");
            tt(this, "measureFunc");
            tt(this, "paddingLeft");
            tt(this, "paddingTop");
            tt(this, "paddingRight");
            tt(this, "paddingBottom");
            tt(this, "marginLeft");
            tt(this, "marginTop");
            tt(this, "marginRight");
            tt(this, "marginBottom");
            tt(this, "width");
            tt(this, "height");
            tt(this, "minWidth");
            tt(this, "minHeight");
            tt(this, "flexDirection");
            tt(this, "positionType");
            tt(this, "positionLeft");
            tt(this, "positionTop");
            tt(this, "positionRight");
            tt(this, "positionBottom");
            tt(this, "computed");
            this.children = [], this.measureFunc = null, this.paddingLeft = 0, this.paddingTop = 0, this.paddingRight = 0, this.paddingBottom = 0, this.marginLeft = 0, this.marginTop = 0, this.marginRight = 0, this.marginBottom = 0, this.width = 0, this.height = 0, this.minWidth = 0, this.minHeight = 0, this.flexDirection = 0, this.positionType = 0, this.positionLeft = null, this.positionTop = null, this.positionRight = null, this.positionBottom = null, this.computed = { left: 0, top: 0, width: 0, height: 0 };
        }
        h.create = function () { return new h; };
        h.prototype.setMeasureFunc = function (d) { this.measureFunc = d; };
        h.prototype.setMargin = function (d, b) { var c = Number(b) || 0; d === 0 ? this.marginLeft = c : d === 1 ? this.marginTop = c : d === 2 ? this.marginRight = c : d === 3 && (this.marginBottom = c); };
        h.prototype.setPadding = function (d, b) { var c = Number(b) || 0; d === 0 ? this.paddingLeft = c : d === 1 ? this.paddingTop = c : d === 2 ? this.paddingRight = c : d === 3 && (this.paddingBottom = c); };
        h.prototype.setFlexDirection = function (d) { this.flexDirection = d; };
        h.prototype.setAlignItems = function (d) { };
        h.prototype.setJustifyContent = function (d) { };
        h.prototype.setFlexWrap = function (d) { };
        h.prototype.setFlexGrow = function (d) { };
        h.prototype.setFlexShrink = function (d) { };
        h.prototype.setAlignSelf = function (d) { };
        h.prototype.setPositionType = function (d) { this.positionType = Number(d) === 1 ? 1 : 0; };
        h.prototype.setPosition = function (d, b) { var c = Number(b) || 0; d === 0 ? this.positionLeft = c : d === 1 ? this.positionTop = c : d === 2 ? this.positionRight = c : d === 3 && (this.positionBottom = c); };
        h.prototype.setWidth = function (d) { this.width = Math.max(0, Number(d) || 0); };
        h.prototype.setHeight = function (d) { this.height = Math.max(0, Number(d) || 0); };
        h.prototype.setMinWidth = function (d) { this.minWidth = Math.max(0, Number(d) || 0); };
        h.prototype.setMinHeight = function (d) { this.minHeight = Math.max(0, Number(d) || 0); };
        h.prototype.insertChild = function (d, b) { this.children.splice(Math.max(0, Math.min(b, this.children.length)), 0, d); };
        h.prototype.getChildCount = function () { return this.children.length; };
        h.prototype.getComputedLeft = function () { return this.computed.left; };
        h.prototype.getComputedTop = function () { return this.computed.top; };
        h.prototype.getComputedWidth = function () { return this.computed.width; };
        h.prototype.getComputedHeight = function () { return this.computed.height; };
        h.prototype.freeRecursive = function () { };
        h.prototype.calculateLayout = function (d, b) {
            if (d === void 0) { d = this.width; }
            if (b === void 0) { b = this.height; }
            this.layout(0, 0, Math.max(1, Number(d) || this.width || 1), Math.max(1, Number(b) || this.height || 1));
        };
        h.prototype.layout = function (d, b, c, E) {
            var e_38, _a, e_39, _b;
            var _, B, R, H;
            var p = this.paddingLeft + this.paddingRight, y = this.paddingTop + this.paddingBottom, k = Math.max(this.minWidth, this.width || c), M = Math.max(this.minHeight, this.height || 0);
            if (this.computed.left = d, this.computed.top = b, this.computed.width = k, this.measureFunc) {
                var D = this.measureFunc(Math.max(0, k - p), 0);
                M = Math.max(M, Math.ceil(Number(D.height) || 0) + y), this.computed.height = M;
                return;
            }
            if (this.flexDirection === 1) {
                var D = this.paddingLeft, w = 0, T = this.children.filter(function (C) { return C.positionType !== 1; }), $ = Math.max(1, T.length);
                try {
                    for (var _c = __values(this.children), _d = _c.next(); !_d.done; _d = _c.next()) {
                        var C = _d.value;
                        if (C.positionType === 1) {
                            var m = C.width || C.minWidth || Math.max(0, k - p - C.marginLeft - C.marginRight), P = C.height || C.minHeight || E, v = C.positionLeft != null ? this.paddingLeft + C.positionLeft : Math.max(0, k - this.paddingRight - ((_ = C.positionRight) != null ? _ : 0) - m), et = C.positionTop != null ? this.paddingTop + C.positionTop : Math.max(0, M - this.paddingBottom - ((B = C.positionBottom) != null ? B : 0) - P);
                            C.layout(v + C.marginLeft, et + C.marginTop, m, P);
                            continue;
                        }
                        var F = C.width || C.minWidth || Math.max(24, (k - p) / $);
                        C.layout(D + C.marginLeft, this.paddingTop + C.marginTop, F, E), D += C.computed.width + C.marginLeft + C.marginRight, w = Math.max(w, C.computed.height + C.marginTop + C.marginBottom);
                    }
                }
                catch (e_38_1) { e_38 = { error: e_38_1 }; }
                finally {
                    try {
                        if (_d && !_d.done && (_a = _c.return)) _a.call(_c);
                    }
                    finally { if (e_38) throw e_38.error; }
                }
                M = Math.max(M, w + y);
            }
            else {
                var D = this.paddingTop;
                try {
                    for (var _f = __values(this.children), _g = _f.next(); !_g.done; _g = _f.next()) {
                        var w = _g.value;
                        if (w.positionType === 1) {
                            var $ = w.width || w.minWidth || Math.max(0, k - p - w.marginLeft - w.marginRight), C = w.height || w.minHeight || E, F = w.positionLeft != null ? this.paddingLeft + w.positionLeft : Math.max(0, k - this.paddingRight - ((R = w.positionRight) != null ? R : 0) - $), m = w.positionTop != null ? this.paddingTop + w.positionTop : Math.max(0, M - this.paddingBottom - ((H = w.positionBottom) != null ? H : 0) - C);
                            w.layout(F + w.marginLeft, m + w.marginTop, $, C);
                            continue;
                        }
                        var T = Math.max(0, k - p - w.marginLeft - w.marginRight);
                        w.layout(this.paddingLeft + w.marginLeft, D + w.marginTop, T, E), D += w.computed.height + w.marginTop + w.marginBottom;
                    }
                }
                catch (e_39_1) { e_39 = { error: e_39_1 }; }
                finally {
                    try {
                        if (_g && !_g.done && (_b = _f.return)) _b.call(_f);
                    }
                    finally { if (e_39) throw e_39.error; }
                }
                M = Math.max(M, D + this.paddingBottom);
            }
            this.computed.height = Math.max(this.minHeight, M);
        };
        return h;
    }()); return { Node: h, EDGE_LEFT: 0, EDGE_TOP: 1, EDGE_RIGHT: 2, EDGE_BOTTOM: 3, FLEX_DIRECTION_COLUMN: 0, FLEX_DIRECTION_ROW: 1, FLEX_DIRECTION_ROW_REVERSE: 1, ALIGN_STRETCH: 0, ALIGN_CENTER: 1, ALIGN_FLEX_START: 2, JUSTIFY_CENTER: 0, JUSTIFY_FLEX_START: 1, JUSTIFY_SPACE_BETWEEN: 2, WRAP_WRAP: 1, WRAP_NO_WRAP: 0, POSITION_TYPE_RELATIVE: 0, POSITION_TYPE_ABSOLUTE: 1, DIRECTION_LTR: 0, MEASURE_MODE_UNDEFINED: 0 }; }
    function us(t) {
        var e_40, _a;
        var r;
        var e = 0, n = function (i, o, s) {
            var e_41, _a;
            var h;
            var l = o + i.x, a = s + i.y;
            if (!(i.kind === "block" && i.tagName === "dialog")) {
                e = Math.max(e, a + i.height);
                try {
                    for (var _b = __values((h = i.children) != null ? h : []), _c = _b.next(); !_c.done; _c = _b.next()) {
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
            var h = ((s = e == null ? void 0 : e["aria-checked"]) != null ? s : "").toLowerCase(), f = ((l = e == null ? void 0 : e["data-indeterminate"]) != null ? l : "").toLowerCase();
            r.indeterminate = (e ? Object.prototype.hasOwnProperty.call(e, "indeterminate") : !1) || h === "mixed" || f === "true" || f === "1" || f === "yes";
        }
    }
    else
        r.value = (a = e == null ? void 0 : e.value) != null ? a : ""; return u.inputs.set(t, r), r; }
    function ds(t) { var e = new Map; function n(r) {
        var e_42, _a;
        var i, o, s, l, a;
        if (r.kind === "block" && r.tagName === "input" && ((o = (i = r.attrs) == null ? void 0 : i.type) != null ? o : "text").toLowerCase() === "radio") {
            var d = "radio:".concat((l = (s = r.attrs) == null ? void 0 : s.name) != null ? l : "__default__"), b = r.key;
            if (b) {
                var c = (a = e.get(d)) != null ? a : [];
                c.push(b), e.set(d, c);
            }
        }
        try {
            for (var _b = __values(r.children), _c = _b.next(); !_c.done; _c = _b.next()) {
                var h = _c.value;
                n(h);
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
    function Pi(t, e, n) { var h, f; if (!t || typeof t != "object")
        return null; var r = t, i = typeof r.kind == "string" ? r.kind : ""; if (i === "text") {
        var d = typeof r.text == "string" ? r.text : "", b = "", c = (h = n == null ? void 0 : n.rows[n.index]) != null ? h : "", E = !1;
        if (n && n.index < n.rows.length ? (n.index += 1, b = c, E = !0) : b = me(Gn(d)), !E && (d.indexOf("<truesurfer-") >= 0 || d.indexOf("__trueo") >= 0) || b.startsWith("<truesurfer-") || b.startsWith("__trueo"))
            b = "";
        else if (b.length === 0) {
            var y = (f = n == null ? void 0 : n.rows[n.index]) != null ? f : "";
            n && y && (n.index += 1), y && (b = y);
        }
        return b.length > 0 ? { kind: "text", text: b } : null;
    } if (i !== "block")
        return null; var o = typeof r.tagName == "string" ? r.tagName.toLowerCase() : ""; if (o.length === 0)
        return null; var s = typeof r.key == "string" ? r.key : "".concat(e, ":").concat(o), l = [], a = Array.isArray(r.children) ? r.children : []; for (var d = 0; d < a.length; d += 1) {
        var b = Pi(a[d], "".concat(e, ".").concat(d), n);
        b && l.push(b);
    } return { kind: "block", key: s, tagName: o, attrs: hs(r.attrs), children: l }; }
    function ms(t, e) { var n = Array.isArray(t) ? t : t && typeof t == "object" && Array.isArray(t.widgetRenderTree) ? t.widgetRenderTree : [], i = { rows: Array.isArray(e) ? He(e) : ki(e), index: 0 }, o = []; for (var s = 0; s < n.length; s += 1) {
        var l = Pi(n[s], "0.".concat(s), i);
        l && o.push(l);
    } return o; }
    function fs(t, e) { if (!Array.isArray(e) || e.length === 0)
        return 0; var n = 0, r = 0, i = function (o) { if (o.kind === "text") {
        if (n < e.length) {
            var s = e[n];
            n += 1, typeof s == "string" && s.length > 0 && s.indexOf("<truesurfer-") !== 0 && s.indexOf("__trueo") !== 0 && (o.text = s, r += 1);
        }
        return;
    } for (var s = 0; s < o.children.length; s += 1)
        i(o.children[s]); }; for (var o = 0; o < t.length; o += 1)
        i(t[o]); return r; }
    function ps(t) { var n = document.createElement("canvas").getContext("2d"); if (!n)
        throw new Error("2D canvas not available"); n.font = t; var r = t.indexOf("px"), i = r; for (; i > 0;) {
        var l = t.charCodeAt(i - 1);
        if (l < 48 || l > 57)
            break;
        i -= 1;
    } var o = r > i ? Number(t.slice(i, r)) : 16, s = Math.ceil(o * 1.25); return { measure: function (l, a) {
            var e_44, _a;
            Nn += 1;
            var h = me(l).split(" ").filter(Boolean);
            if (h.length === 0)
                return { width: 0, height: s, lines: [""] };
            var f = [], d = "";
            try {
                for (var h_1 = __values(h), h_1_1 = h_1.next(); !h_1_1.done; h_1_1 = h_1.next()) {
                    var E = h_1_1.value;
                    var p = d ? "".concat(d, " ").concat(E) : E, y = n.measureText(p).width, k = a != null ? a : Number.POSITIVE_INFINITY;
                    y <= k || !d ? d = p : (f.push(d), d = E);
                }
            }
            catch (e_44_1) { e_44 = { error: e_44_1 }; }
            finally {
                try {
                    if (h_1_1 && !h_1_1.done && (_a = h_1.return)) _a.call(h_1);
                }
                finally { if (e_44) throw e_44.error; }
            }
            d && f.push(d);
            var b = Math.min(Math.max.apply(Math, __spreadArray([], __read(f.map(function (E) { return n.measureText(E).width; })), false)), a != null ? a : Number.POSITIVE_INFINITY), c = f.length * s;
            return { width: Math.ceil(b), height: Math.ceil(c), lines: f };
        }, lineHeight: s, font: t }; }
    function gs(t, e, n) { var b; W("build:start nodes=".concat(t.length, " viewport=").concat(e, "x").concat(n)); var r = 12, i = 8, o = be; W("build:measurer"); var s = ps("".concat(o.fontSize, "px ").concat(o.fontFamily)); function l(c) { return c.kind !== "block" || c.tagName === "hr" || c.tagName === "tr" || c.tagName === "td" || c.tagName === "th" ? 0 : i; } function a(c) { var E = c.kind === "text" ? "text:".concat(c.text.slice(0, 24)) : "".concat(c.tagName, ":").concat(c.key); if (W("node:".concat(E, ":start")), c.kind === "text") {
        var M_1 = it.Node.create();
        return W("node:".concat(E, ":measure-func")), M_1.setMeasureFunc(function (_, B) { W("node:".concat(E, ":measure-call")); var R = B === it.MEASURE_MODE_UNDEFINED ? void 0 : Math.max(0, _), H = s.measure(c.text, R); return { width: H.width, height: H.height }; }), M_1.setMargin(it.EDGE_RIGHT, 6), M_1.setMargin(it.EDGE_BOTTOM, 0), { yogaNode: M_1, buildBox: function () { return ({ kind: "text", text: c.text, x: M_1.getComputedLeft(), y: M_1.getComputedTop(), width: M_1.getComputedWidth(), height: M_1.getComputedHeight(), children: [] }); } };
    } if (c.tagName === "sliderlabel")
        return W("node:".concat(c.tagName, ":").concat(c.key, ":sliderlabel")), rr({ node: c, Yoga: it, measurer: s }); W("node:".concat(c.tagName, ":").concat(c.key, ":create")); var p = it.Node.create(); if (W("node:".concat(c.tagName, ":").concat(c.key, ":base-defaults")), p.setFlexDirection(it.FLEX_DIRECTION_COLUMN), p.setAlignItems(it.ALIGN_STRETCH), p.setPadding(it.EDGE_LEFT, r), p.setPadding(it.EDGE_RIGHT, r), p.setPadding(it.EDGE_TOP, r), p.setPadding(it.EDGE_BOTTOM, r), p.setMargin(it.EDGE_BOTTOM, 0), En(c.tagName) && (W("node:".concat(c.tagName, ":").concat(c.key, ":heading-defaults")), xr(p, it)), c.tagName === "hr" && (W("node:".concat(c.tagName, ":").concat(c.key, ":hr-defaults")), ur(p, it)), (c.tagName === "p" || c.tagName === "label") && (W("node:".concat(c.tagName, ":").concat(c.key, ":inline-scan")), c.children.some(function (_) { return _.kind === "block" && (_.tagName === "input" || _.tagName === "button" || _.tagName === "select" || _.tagName === "textarea" || _.tagName === "timeinput" || _.tagName === "dateinput" || _.tagName === "monthinput" || _.tagName === "weekinput" || _.tagName === "datetimelocalinput" || _.tagName === "progress" || _.tagName === "meter" || _.tagName === "slider" || _.tagName === "number" || _.tagName === "color"); }) && (p.setFlexDirection(it.FLEX_DIRECTION_ROW), p.setFlexWrap(it.WRAP_WRAP), p.setAlignItems(it.ALIGN_CENTER)), p.setPadding(it.EDGE_TOP, 4), p.setPadding(it.EDGE_BOTTOM, 4), p.setPadding(it.EDGE_LEFT, 4), p.setPadding(it.EDGE_RIGHT, 4)), c.tagName === "table" && (W("node:".concat(c.tagName, ":").concat(c.key, ":table-defaults")), pr(p, it)), c.tagName === "tr" && (W("node:".concat(c.tagName, ":").concat(c.key, ":tr-defaults")), gr(p, it)), (c.tagName === "td" || c.tagName === "th") && (W("node:".concat(c.tagName, ":").concat(c.key, ":cell-defaults")), br(p, it)), c.tagName === "input" && (W("node:".concat(c.tagName, ":").concat(c.key, ":input-defaults")), Fr(p, c, it)), c.tagName === "textarea" && (W("node:".concat(c.tagName, ":").concat(c.key, ":textarea-defaults")), $r(p, it)), c.tagName === "select" && (W("node:".concat(c.tagName, ":").concat(c.key, ":select-defaults")), ei(p, it)), c.tagName === "timeinput" || c.tagName === "dateinput" || c.tagName === "monthinput" || c.tagName === "weekinput" || c.tagName === "datetimelocalinput") {
        var M = c.tagName === "timeinput" ? "time" : c.tagName === "monthinput" ? "month" : c.tagName === "weekinput" ? "week" : c.tagName === "dateinput" ? "date" : "datetime-local";
        W("node:".concat(c.tagName, ":").concat(c.key, ":temporal-defaults")), ii(p, it, M);
    } c.tagName === "img" && (W("node:".concat(c.tagName, ":").concat(c.key, ":img-defaults")), Ir(p, c, it)), c.tagName === "svg" && (W("node:".concat(c.tagName, ":").concat(c.key, ":svg-defaults")), Ar(p, c, it)), c.tagName === "canvas" && (W("node:".concat(c.tagName, ":").concat(c.key, ":canvas-defaults")), Gr(p, c, it)), c.tagName === "iframe" && (W("node:".concat(c.tagName, ":").concat(c.key, ":iframe-defaults")), Lr(p, c, it)), c.tagName === "button" && (W("node:".concat(c.tagName, ":").concat(c.key, ":button-defaults")), hr(p, it)), c.tagName === "dialog" && (W("node:".concat(c.tagName, ":").concat(c.key, ":dialog-defaults")), zr(p, it)), c.tagName === "number" && (W("node:".concat(c.tagName, ":").concat(c.key, ":number-defaults")), Vr(p, it)), c.tagName === "color" && (W("node:".concat(c.tagName, ":").concat(c.key, ":color-defaults")), Qr(p, c, it)), c.tagName === "searchrow" && (W("node:".concat(c.tagName, ":").concat(c.key, ":searchrow-defaults")), Xr(p, it)), c.tagName === "searchbutton" && (W("node:".concat(c.tagName, ":").concat(c.key, ":searchbutton-defaults")), Yr(p, it)), c.tagName === "summary" && (W("node:".concat(c.tagName, ":").concat(c.key, ":summary-defaults")), sr(p, it)), c.tagName === "details" && (W("node:".concat(c.tagName, ":").concat(c.key, ":details-defaults")), ar(p, it)), c.tagName === "barrow" && (W("node:".concat(c.tagName, ":").concat(c.key, ":barrow-defaults")), Ur(p, it)), (c.tagName === "progress" || c.tagName === "meter") && (W("node:".concat(c.tagName, ":").concat(c.key, ":progress-defaults")), er(p, it)), c.tagName === "slider" && (W("node:".concat(c.tagName, ":").concat(c.key, ":slider-defaults")), nr(p, it)), W("node:".concat(c.tagName, ":").concat(c.key, ":children-effective")); var y = lr(c, u.detailsOpen); W("node:".concat(c.tagName, ":").concat(c.key, ":children-map count=").concat(y.length)); var k = y.map(a); W("node:".concat(c.tagName, ":").concat(c.key, ":children-insert")); for (var M = 0; M < k.length; M++) {
        var _ = y[M], B = k[M];
        if (_ && _.kind === "block") {
            var R = M === k.length - 1 ? 0 : l(_);
            B.yogaNode.setMargin(it.EDGE_BOTTOM, R);
        }
        p.insertChild(B.yogaNode, p.getChildCount());
    } return { yogaNode: p, buildBox: function () { return ({ kind: "block", key: c.key, tagName: c.tagName, attrs: c.attrs, x: p.getComputedLeft(), y: p.getComputedTop(), width: p.getComputedWidth(), height: p.getComputedHeight(), children: k.map(function (M) { return M.buildBox(); }) }); } }; } var h = it.Node.create(); W("root:flex-direction"), h.setFlexDirection(it.FLEX_DIRECTION_COLUMN), W("root:align-items"), h.setAlignItems(it.ALIGN_STRETCH), W("root:width"), h.setWidth(e), W("root:height"), h.setHeight(n), W("root:padding-left"), h.setPadding(it.EDGE_LEFT, 16), W("root:padding-top"), h.setPadding(it.EDGE_TOP, 16), W("root:padding-right"), h.setPadding(it.EDGE_RIGHT, 16 + An), W("root:padding-bottom"), h.setPadding(it.EDGE_BOTTOM, 16), W("root:children-map count=".concat(t.length)); var f = t.map(a); W("root:children-insert"); for (var c = 0; c < f.length; c++) {
        var E = t[c], p = f[c];
        if (E && E.kind === "block") {
            var y = c === f.length - 1 ? 0 : l(E);
            p.yogaNode.setMargin(it.EDGE_BOTTOM, y);
        }
        h.insertChild(p.yogaNode, h.getChildCount());
    } W("root:calculate"), h.calculateLayout(e, n, it.DIRECTION_LTR), W("root:build-box"); var d = { kind: "block", tagName: "root", x: 0, y: 0, width: h.getComputedWidth(), height: h.getComputedHeight(), children: f.map(function (c) { return c.buildBox(); }) }; return W("root:free"), (b = h.freeRecursive) == null || b.call(h), W("build:done"), d; }
    function bs(t, e, n) {
        var e_45, _a, e_46, _b, e_47, _c, e_48, _d, e_49, _f;
        var C, F;
        W("render:start");
        var r = be, i = n != null ? n : t.stage;
        W("render:get-background");
        var o = Pt(i, "__background");
        W("render:get-content-root");
        var s = xe(i, "__contentRoot");
        W("render:get-dialog-root");
        var l = xe(i, "__dialogRoot");
        W("render:get-overlay-root");
        var a = xe(i, "__overlayRoot");
        W("render:ensure-background"), _i(i, o, 0), W("render:ensure-content-root"), $e(i, s, 1), W("render:ensure-dialog-root"), $e(i, l, 2), W("render:ensure-overlay-root"), $e(i, a, 3), W("render:overlay-remove-children"), a.removeChildren(), W("render:overlay-removed");
        var h = [], f = [], d = ds(e);
        W("render:clear-ui-state"), u.fieldBounds.clear(), u.sliderBounds.clear(), u.dialogDragBounds.clear(), u.hoverRects.length = 0, u.hoverHandlers.clear(), u.iframeRects.length = 0, W("render:node-cache");
        var b = (C = yi.get(i)) != null ? C : new Map;
        yi.set(i, b);
        var c = new Set, E = function (m) {
            var e_50, _a;
            var et;
            var P = 0, v = function (q, nt, Q) {
                var e_51, _a;
                var U;
                if (q.kind === "block" && q.tagName === "dialog")
                    return;
                var j = nt + q.x, G = Q + q.y;
                P = Math.max(P, G + q.height);
                try {
                    for (var _b = __values((U = q.children) != null ? U : []), _c = _b.next(); !_c.done; _c = _b.next()) {
                        var g = _c.value;
                        v(g, j, G);
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
                for (var _b = __values((et = m.children) != null ? et : []), _c = _b.next(); !_c.done; _c = _b.next()) {
                    var q = _c.value;
                    v(q, 0, 0);
                }
            }
            catch (e_50_1) { e_50 = { error: e_50_1 }; }
            finally {
                try {
                    if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
                }
                finally { if (e_50) throw e_50.error; }
            }
            return P;
        }, p = new Set;
        try {
            for (var _g = __values(u.textDrags.values()), _h = _g.next(); !_h.done; _h = _g.next()) {
                var m = _h.value;
                p.add(m.key);
            }
        }
        catch (e_45_1) { e_45 = { error: e_45_1 }; }
        finally {
            try {
                if (_h && !_h.done && (_a = _g.return)) _a.call(_g);
            }
            finally { if (e_45) throw e_45.error; }
        }
        W("render:measure");
        var y = vo(r);
        function k(m, P, v) { return Math.max(P, Math.min(v, m)); }
        var M = function (m) {
            var e_52, _a;
            try {
                for (var _b = __values(u.textDrags.entries()), _c = _b.next(); !_c.done; _c = _b.next()) {
                    var _d = __read(_c.value, 2), P = _d[0], v = _d[1];
                    if (v.key === m)
                        return P;
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
        }, _ = function (m) {
            var e_53, _a;
            var P = u.keyboardOwnerPointerId;
            if (u.focusedKeyByPointer.get(P) === m)
                return P;
            try {
                for (var _b = __values(u.focusedKeyByPointer.entries()), _c = _b.next(); !_c.done; _c = _b.next()) {
                    var _d = __read(_c.value, 2), v = _d[0], et = _d[1];
                    if (et === m)
                        return v;
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
        W("render:background-clear"), kt(o), W("render:background-rect"), o.rect(0, 0, t.renderer.width, t.renderer.height), W("render:background-fill"), o.fill(r.background), W("render:content-position");
        {
            var m = u.scroll, P = m && Number(m.y || 0) || 0, v = s.position;
            v && (v.x = 0, v.y = -P);
        }
        W("render:content-position-done");
        function B(m, P, v, et, q, nt, Q, j, G) {
            var e_54, _a;
            if (et === void 0) { et = 0; }
            if (q === void 0) { q = 0; }
            var A, O, L, K, Y, N, z, V, J, rt, st, mt, Tt, Mt, Gt, vt, lt, yt, It, At;
            W("render:draw:".concat(j, ":").concat(m.kind, ":").concat(m.kind === "block" ? m.tagName : "text", ":start"));
            var U = m.kind === "block" ? m.key && m.key.length > 0 ? m.key : "".concat(j, ":").concat((A = m.tagName) != null ? A : "block") : "", g = m.kind === "block" ? "b:".concat(U) : "t:".concat(j);
            W("render:draw:".concat(j, ":cache"));
            var S = b.get(g);
            (!S || Bn(P, S)) && (W("render:draw:".concat(j, ":new-container")), S = new wt, S.label = g, b.set(g, S)), W("render:draw:".concat(j, ":ensure-child")), c.add(g), $e(P, S, G), W("render:draw:".concat(j, ":children-root"));
            var X = xe(S, "__children");
            if (W("render:draw:".concat(j, ":ensure-children-root")), $e(S, X, 1), W("render:draw:".concat(j, ":position")), oe(S, m.x, m.y), m.kind === "block" && m.tagName === "hr" && oe(S, Math.round(m.x), Math.round(m.y)), m.kind === "block" && m.tagName === "dialog" && m.key) {
                var ft = Qe(u.dialogs, m.key), pt = Math.max(0, m.width), ct = Math.max(0, m.height), at = Q.x, Yt = Q.y, Nt = Math.max(at, Q.x + Q.w - pt), Ft = Math.max(Yt, Q.y + Q.h - ct);
                if (u.dialogDragBounds.set(m.key, { minX: at, minY: Yt, maxX: Nt, maxY: Ft }), $t() && !ft.__trueosInitialPositionSeeded) {
                    var se = Q.w <= 760 && Q.h <= 800, re = at + Math.max(12, Math.floor((Q.w - pt) / 2)), ut = Yt + Math.max(se ? 190 : 40, Math.floor((Q.h - ct) / 2));
                    ft.x = Math.max(at, Math.min(Nt, re)), ft.y = Math.max(Yt, Math.min(Ft, ut)), ft.__trueosInitialPositionSeeded = !0;
                }
                ft.x = Math.max(at, Math.min(Nt, ft.x)), ft.y = Math.max(Yt, Math.min(Ft, ft.y)), oe(S, ft.x, ft.y);
            }
            var x = et + S.position.x, I = q + S.position.y;
            if (m.kind === "block") {
                W("render:draw:".concat(j, ":block:").concat(m.tagName, ":begin"));
                var ft = v;
                (m.tagName === "h1" || m.tagName === "h2" || m.tagName === "h3" || m.tagName === "summary" || m.tagName === "th") && (ft = { bold: !0 }), W("render:draw:".concat(j, ":graphics"));
                var pt = Pt(S, "__g");
                W("render:draw:".concat(j, ":graphics-clear")), kt(pt), W("render:draw:".concat(j, ":graphics-ensure")), _i(S, pt, 0), pt.zIndex = -10;
                var ct = Math.max(0, m.width), at = Math.max(0, m.height), Yt = null;
                if ((m.tagName === "h1" || m.tagName === "h2" || m.tagName === "h3") && (oe(S, Math.round(m.x), Math.round(m.y)), ct = Math.round(ct), at = Math.round(at)), W("render:draw:".concat(j, ":widget:").concat(m.tagName)), m.tagName === "hr")
                    cr({ graphics: pt, w: ct, theme: r });
                else if (m.tagName !== "barrow") {
                    if (m.tagName !== "searchrow") {
                        if (m.tagName === "searchbutton")
                            Kr({ node: m, container: S, graphics: pt, w: ct, h: at, theme: r, uiState: u, getPointerId: Bt, focusInputKey: (O = m.attrs) == null ? void 0 : O["data-focus-key"], requestPaint: ot });
                        else if (m.tagName === "progress" || m.tagName === "meter")
                            tr({ node: m, graphics: pt, w: ct, h: at, theme: r });
                        else if (m.tagName === "sliderlabel")
                            ir({ node: m, container: S, theme: r, sliderStates: u.sliders });
                        else if (m.tagName === "slider")
                            Ze({ node: m, container: S, graphics: pt, w: ct, h: at, absX: x, absY: I, theme: r, sliderStates: u.sliders, sliderBounds: u.sliderBounds, sliderDrags: u.sliderDrags, requestPaint: ot, getPointerId: Bt });
                        else if (m.tagName === "timeinput" || m.tagName === "dateinput" || m.tagName === "monthinput" || m.tagName === "weekinput" || m.tagName === "datetimelocalinput")
                            oi({ node: m, container: S, graphics: pt, w: ct, h: at, absX: x, absY: I, theme: r, uiState: u, getPointerId: Bt, getCursorColor: Jt, temporalStates: u.temporals, yearSliderOwners: u.temporalYearOwners, getOrInitInputValue: function (Z, ht) { return un(Z, ht); }, requestPaint: ot, popupSink: f });
                        else if (m.tagName === "input") {
                            var Z = m.key, ht = Z != null ? _(Z) : null, Vt = Z != null && u.focusedKeyByPointer.get(u.keyboardOwnerPointerId) === Z, Rt = Z == null ? null : Vt ? u.keyboardOwnerPointerId : p.has(Z) ? M(Z) : null, ee = Rt != null, Zt = ht != null ? Jt(ht) : null;
                            Wr({ node: m, container: S, graphics: pt, w: ct, h: at, absX: x, absY: I, theme: r, textMeasure: y, uiState: u, getOrInitInputState: un, clamp: k, radioGroups: d, textDrags: u.textDrags, requestPaint: ot, showCaret: ee, caretPointerId: Rt, focusColor: Zt != null ? Zt : void 0, getCursorColor: Jt, getPointerId: Bt });
                        }
                        else if (m.tagName === "textarea") {
                            var Z = m.key, ht = Z != null ? _(Z) : null, Vt = Z != null && u.focusedKeyByPointer.get(u.keyboardOwnerPointerId) === Z, Rt = Z == null ? null : Vt ? u.keyboardOwnerPointerId : p.has(Z) ? M(Z) : null, ee = Rt != null, Zt = ht != null ? Jt(ht) : null;
                            Hr({ node: m, container: S, graphics: pt, w: ct, h: at, absX: x, absY: I, theme: r, textMeasure: y, uiState: u, getOrInitInputState: un, clamp: k, textDrags: u.textDrags, requestPaint: ot, showCaret: ee, caretPointerId: Rt, focusColor: Zt != null ? Zt : void 0, getCursorColor: Jt, getPointerId: Bt });
                        }
                        else if (m.tagName === "select") {
                            if (m.key) {
                                var Z = Number((K = (L = m.attrs) == null ? void 0 : L["data-selected-index"]) != null ? K : "0");
                                de(u.selects, m.key, Number.isFinite(Z) ? Z : 0);
                            }
                            nn({ node: m, container: S, graphics: pt, w: ct, h: at, absX: x, absY: I, theme: r, selectStates: u.selects, uiState: u, getPointerId: Bt, getCursorColor: Jt, requestPaint: ot, popupSink: h });
                        }
                        else if (m.tagName === "summary")
                            m.key && u.hoverRects.push({ key: m.key, kind: "summary", cursor: "pointer", x: x, y: I, w: ct, h: at }), or({ node: m, container: S, w: ct, h: at, theme: r, detailsOpen: u.detailsOpen, requestRerender: dn });
                        else if (m.tagName === "dialog")
                            jr({ node: m, container: S, w: ct, h: at, theme: r, selectedBy: u.dialogSelectedBy, getCursorColor: Jt, dialogStates: u.dialogs, dialogDrags: u.dialogDrags, bringToFront: function (Z) { u.dialogZ.set(Z, u.dialogZCounter++); }, requestPaint: ot, getPointerId: Bt });
                        else if (m.tagName === "img")
                            Sr({ node: m, container: S, graphics: pt, w: ct, h: at, theme: r, requestRerender: dn });
                        else if (m.tagName === "svg") {
                            var Z = (N = (Y = m.attrs) == null ? void 0 : Y["data-svg"]) != null ? N : "";
                            Nr({ svgMarkup: Z, container: S, w: ct, h: at, requestRerender: dn });
                        }
                        else if (m.tagName === "canvas")
                            vr({ node: m, container: S, graphics: pt, w: ct, h: at, theme: r });
                        else if (m.tagName === "iframe")
                            Br({ node: m, container: S, graphics: pt, w: ct, h: at, theme: r });
                        else if (m.tagName === "color")
                            u.color.bounds = { x: x, y: I, w: Math.max(0, ct), h: Math.max(0, at) }, ti({ node: m, container: S, graphics: pt, w: ct, h: at, theme: r, rgb: u.color.rgb, setRgb: function (Z) { u.color.rgb = Z; }, alpha: u.color.a, setAlpha: function (Z) { u.color.a = Math.max(0, Math.min(255, Math.round(Z))); }, pick: u.color.pick, setPick: function (Z) { u.color.pick = Z; }, requestPaint: ot, getPointerId: Bt, setDraggingPointerId: function (Z) { u.color.draggingPointerId = Z; } });
                        else if (m.tagName === "number") {
                            var Z_1 = m.key, ht_1 = String((V = (z = m.attrs) == null ? void 0 : z.channel) != null ? V : "").toLowerCase(), Vt_1 = ht_1 === "r" || ht_1 === "g" || ht_1 === "b" || ht_1 === "a";
                            Z_1 && Jr({ node: m, container: S, graphics: pt, w: ct, h: at, theme: r, getValue: function () { var Rt, ee; return Vt_1 ? ht_1 === "a" ? (Rt = u.color.a) != null ? Rt : 255 : (ee = u.color.rgb[ht_1]) != null ? ee : 0 : Sn(u.numbers, Z_1, m.attrs).value; }, setValue: function (Rt) { Vt_1 ? ht_1 === "a" ? u.color.a = Math.max(0, Math.min(255, Math.round(Rt))) : u.color.rgb[ht_1] = Math.max(0, Math.min(255, Math.round(Rt))) : Sn(u.numbers, Z_1, m.attrs).value = Rt; }, requestPaint: ot, numberHolds: u.numberHolds, getPointerId: Bt });
                        }
                        else if (m.tagName === "button") {
                            var Z = me(Ln(m));
                            m.key && u.hoverRects.push({ key: m.key, kind: "button", cursor: "pointer", x: x, y: I, w: ct, h: at }), dr({ container: S, graphics: pt, w: ct, h: at, label: $t() ? "" : Z, theme: r, registerHoverHandlers: m.key ? function (ht) { u.hoverHandlers.set(m.key, ht); } : void 0 });
                        }
                        else if (!En(m.tagName))
                            if (m.tagName === "table")
                                mr({ graphics: pt, w: ct, h: at, boxBorder: r.boxBorder });
                            else if (m.tagName === "td" || m.tagName === "th")
                                fr({ nodeTag: m.tagName, graphics: pt, w: ct, h: at, theme: r });
                            else {
                                var Z = Math.max(0, Math.round(ct)), ht = Math.max(0, Math.round(at));
                                pt.rect(0, 0, Z, ht), pt.stroke({ width: 1, color: r.boxBorder, alignment: 0 });
                            }
                    }
                }
                W("render:draw:".concat(j, ":overlay-label")), Yt && S.addChild(Yt);
                var Nt = null, Ft = null, se = m.tagName === "iframe" && String((rt = (J = m.attrs) == null ? void 0 : J["data-root"]) != null ? rt : "") === "1";
                if (m.tagName === "iframe" && !se) {
                    m.key && u.iframeRects.push({ key: m.key, x: x, y: I, w: Math.max(0, ct), h: Math.max(0, at) }), Nt = xe(S, "__iframeContentRoot"), oe(Nt, 0, 0);
                    var Rt = Pt(S, "__iframeContentMask");
                    kt(Rt);
                    var ee = 0, Zt = 34, Ci = Math.max(0, ct), Oi = Math.max(0, at - 34);
                    Rt.rect(ee, Zt, Ci, Oi), Rt.fill(16777215), Rt.alpha = 0, Nt.mask = Rt;
                    var Xe_1 = (st = m.key) != null ? st : "", dt_1 = (mt = u.iframeScroll.get(Xe_1)) != null ? mt : { y: 0, contentHeight: 0, viewportHeight: 0, draggingPointerId: null, dragOffsetY: 0, track: { x: 0, y: 0, w: Pe, h: 0 }, thumb: { x: 0, y: 0, w: Pe, h: 0 }, rect: { x: x, y: I, w: Math.max(0, ct), h: Math.max(0, at) } };
                    dt_1.rect = { x: x, y: I, w: Math.max(0, ct), h: Math.max(0, at) }, dt_1.contentHeight = E(m), dt_1.viewportHeight = Math.max(0, at - 34 - 8);
                    var we_1 = Math.max(0, dt_1.contentHeight - dt_1.viewportHeight);
                    dt_1.y = Math.max(0, Math.min(dt_1.y, we_1)), Ft = xe(Nt, "__iframeScrollRoot"), oe(Ft, 0, -dt_1.y);
                    var le = Pt(S, "__iframeScrollbar");
                    kt(le), le.eventMode = "static";
                    var gn = An, fe = Pe, Ye = Math.max(0, ct - fe - gn), bn = 34 + gn, Oe = Math.max(0, at - 34 - gn * 2), Fn = we_1 > .5 && Oe > 1;
                    if (le.visible = Fn, Fn) {
                        var xn = Math.max(24, (dt_1.viewportHeight || 1) / Math.max(1, dt_1.contentHeight) * Oe), Ri = Math.max(1, Oe - xn), Di = we_1 <= 0 ? 0 : dt_1.y / we_1, Wn = bn + Ri * Di;
                        dt_1.track = { x: x + Ye, y: I + bn, w: fe, h: Oe }, dt_1.thumb = { x: x + Ye, y: I + Wn, w: fe, h: xn }, le.rect(Ye, bn, fe, Oe), le.fill({ color: 0, alpha: .06 }), le.rect(Ye, Wn, fe, xn), le.fill({ color: 0, alpha: .25 }), le.on("pointerdown", function (Qt) { var Un, Xn, Yn, Kn, zn, jn; if ((Qt == null ? void 0 : Qt.button) === 2)
                            return; var yn = Bt(Qt); if (yn <= 0)
                            return; var Ke = (Xn = (Un = Qt.global) == null ? void 0 : Un.x) != null ? Xn : 0, pe = (Kn = (Yn = Qt.global) == null ? void 0 : Yn.y) != null ? Kn : 0; if (!(Ke >= dt_1.track.x && Ke <= dt_1.track.x + dt_1.track.w && pe >= dt_1.track.y && pe <= dt_1.track.y + dt_1.track.h))
                            return; if (Ke >= dt_1.thumb.x && Ke <= dt_1.thumb.x + dt_1.thumb.w && pe >= dt_1.thumb.y && pe <= dt_1.thumb.y + dt_1.thumb.h) {
                            dt_1.draggingPointerId = yn, dt_1.dragOffsetY = pe - dt_1.thumb.y, u.iframeScroll.set(Xe_1, dt_1), (zn = Qt.stopPropagation) == null || zn.call(Qt);
                            return;
                        } var $n = Math.max(1, dt_1.track.h - dt_1.thumb.h), Hn = Math.max(dt_1.track.y, Math.min(dt_1.track.y + $n, pe - dt_1.thumb.h / 2)), Ai = (Hn - dt_1.track.y) / $n; dt_1.y = Math.max(0, Math.min(we_1, Ai * we_1)), dt_1.draggingPointerId = yn, dt_1.dragOffsetY = pe - Hn, u.iframeScroll.set(Xe_1, dt_1), ot == null || ot(), (jn = Qt.stopPropagation) == null || jn.call(Qt); });
                    }
                    else
                        dt_1.track = { x: 0, y: 0, w: fe, h: 0 }, dt_1.thumb = { x: 0, y: 0, w: fe, h: 0 };
                    u.iframeScroll.set(Xe_1, dt_1);
                }
                var re = [], ut = m.tagName === "dialog" || m.tagName === "iframe" && !se ? re : nt, gt = Q;
                if (m.tagName === "dialog")
                    gt = { x: 0, y: 0, w: Math.max(0, ct), h: Math.max(0, at) };
                else if (m.tagName === "iframe" && !se) {
                    var Z = (Tt = m.key) != null ? Tt : "", ht = u.iframeScroll.get(Z), Vt = ht ? ht.y : 0, Rt = 34;
                    gt = { x: 0, y: Rt + Vt, w: Math.max(0, ct), h: Math.max(0, at - Rt) };
                }
                var Et = (Mt = Ft != null ? Ft : Nt) != null ? Mt : X, Dt = x + ((Gt = Nt == null ? void 0 : Nt.position.x) != null ? Gt : 0), Lt = I + ((vt = Nt == null ? void 0 : Nt.position.y) != null ? vt : 0) + ((lt = Ft == null ? void 0 : Ft.position.y) != null ? lt : 0);
                W("render:draw:".concat(j, ":children"));
                var Kt = 0;
                for (var Z = 0; Z < ((yt = m.children) != null ? yt : []).length; Z++) {
                    var ht = ((It = m.children) != null ? It : [])[Z];
                    if (ht.kind === "block" && ht.tagName === "dialog")
                        ut.push(ht);
                    else {
                        if (m.tagName === "button" && ht.kind === "text" && !$t())
                            continue;
                        B(ht, Et, ft, Dt, Lt, ut, gt, "".concat(j, ".").concat(Z), Kt++);
                    }
                }
                if ((m.tagName === "dialog" || m.tagName === "iframe" && !se) && re.length > 0) {
                    re.sort(function (Z, ht) { var ee, Zt; var Vt = Z.key && (ee = u.dialogZ.get(Z.key)) != null ? ee : 0, Rt = ht.key && (Zt = u.dialogZ.get(ht.key)) != null ? Zt : 0; return Vt - Rt; });
                    try {
                        for (var re_1 = __values(re), re_1_1 = re_1.next(); !re_1_1.done; re_1_1 = re_1.next()) {
                            var Z = re_1_1.value;
                            var ht = Z.key && Z.key.length > 0 ? Z.key : "".concat(j, ".dlg.").concat(Kt);
                            B(Z, Et, ft, Dt, Lt, re, gt, "".concat(j, ".dlg.").concat(ht), Kt++);
                        }
                    }
                    catch (e_54_1) { e_54 = { error: e_54_1 }; }
                    finally {
                        try {
                            if (re_1_1 && !re_1_1.done && (_a = re_1.return)) _a.call(re_1);
                        }
                        finally { if (e_54) throw e_54.error; }
                    }
                }
            }
            else {
                W("render:draw:".concat(j, ":text:begin"));
                var ft = Ct(S, "__text", function (pt) { pt.style = { fontFamily: r.fontFamily, fontSize: r.fontSize, fill: r.text, fontWeight: v.bold ? "700" : "400", wordWrap: !0, wordWrapWidth: 0 }; });
                ft.text = (At = m.text) != null ? At : "", ft.style.fontFamily = r.fontFamily, ft.style.fontSize = r.fontSize, ft.style.fill = r.text, ft.style.fontWeight = v.bold ? "700" : "400", ft.style.wordWrap = !0, ft.style.wordWrapWidth = Math.max(0, Math.ceil(m.width) + Me), oe(ft, 0, xt), W("render:draw:".concat(j, ":text:done"));
            }
        }
        W("render:root-loop");
        var R = { bold: !1 }, H = { x: 0, y: 0, w: t.renderer.width, h: t.renderer.height }, D = [], w = s.position, T = w && Number(w.y || 0) || 0, $ = 0;
        for (var m = 0; m < e.children.length; m++) {
            W("render:root-loop:".concat(m));
            var P = e.children[m];
            P && (P.kind === "block" && P.tagName === "dialog" ? D.push(P) : (W("render:root-loop:".concat(m, ":dispatch")), B(P, s, R, 0, T, D, H, "root.".concat(m), $++)));
        }
        if (W("render:root-dialogs"), D.length > 0) {
            D.sort(function (P, v) { var nt, Q; var et = P.key && (nt = u.dialogZ.get(P.key)) != null ? nt : 0, q = v.key && (Q = u.dialogZ.get(v.key)) != null ? Q : 0; return et - q; });
            var m = 0;
            try {
                for (var D_1 = __values(D), D_1_1 = D_1.next(); !D_1_1.done; D_1_1 = D_1.next()) {
                    var P = D_1_1.value;
                    var v = P.key && P.key.length > 0 ? P.key : "rootdlg.".concat(m);
                    B(P, l, R, 0, 0, D, H, "dlg.".concat(v), m++);
                }
            }
            catch (e_46_1) { e_46 = { error: e_46_1 }; }
            finally {
                try {
                    if (D_1_1 && !D_1_1.done && (_b = D_1.return)) _b.call(D_1);
                }
                finally { if (e_46) throw e_46.error; }
            }
        }
        if (W("render:temporal-popups"), f.length > 0 && si({ popups: f, stage: a, theme: r, viewportW: t.renderer.width, viewportH: t.renderer.height, temporalStates: u.temporals, getOrInitInputValue: function (m, P) { return un(m, P); }, sliders: u.sliders, sliderBounds: u.sliderBounds, sliderDrags: u.sliderDrags, selects: u.selects, selectPopups: h, uiFocus: u, getPointerId: Bt, getCursorColor: Jt, requestPaint: ot }), W("render:select-popups"), h.length > 0)
            try {
                for (var h_2 = __values(h), h_2_1 = h_2.next(); !h_2_1.done; h_2_1 = h_2.next()) {
                    var m = h_2_1.value;
                    ni({ popup: m, stage: a, theme: r, selectStates: u.selects, uiState: u, getPointerId: Bt, requestPaint: ot, viewportW: t.renderer.width, viewportH: t.renderer.height });
                }
            }
            catch (e_47_1) { e_47 = { error: e_47_1 }; }
            finally {
                try {
                    if (h_2_1 && !h_2_1.done && (_c = h_2.return)) _c.call(h_2);
                }
                finally { if (e_47) throw e_47.error; }
            }
        W("render:context-menus");
        var _loop_3 = function (m, P) {
            if (!(P != null && P.open))
                return "continue";
            var v = new wt;
            v.eventMode = "static", v.cursor = "default", oe(v, P.x, P.y);
            var et = 140, q = 28, nt = 6, Q = ["Copy", "Paste", "Close"], j = new _t;
            j.rect(0, 0, et + nt * 2, Q.length * q + nt * 2), j.fill(16777215);
            var G = 1;
            j.rect(G, G, et + nt * 2 - G * 2, Q.length * q + nt * 2 - G * 2), j.stroke({ width: 2, color: Jt(m), alignment: 0 }), v.addChild(j), Q.forEach(function (U, g) { var S = nt + g * q, X = new wt; X.eventMode = "static", X.cursor = "pointer", oe(X, nt, S); var x = new _t; x.rect(0, 0, et, q), x.fill(16777215), X.addChild(x); var I = jt({ text: U, fontFamily: r.fontFamily, fontSize: r.fontSize, fill: r.text }); oe(I, 8, Math.max(0, (q - I.height) / 2) + xt), X.addChild(I); var A = function (O) { return Bt(O) === m; }; X.on("pointerover", function (O) { A(O) && (x.clear(), x.rect(0, 0, et, q), x.fill(15921906)); }), X.on("pointerout", function (O) { A(O) && (x.clear(), x.rect(0, 0, et, q), x.fill(16777215)); }), X.on("pointerdown", function (O) { var z, V, J, rt, st, mt, Tt, Mt, Gt, vt, lt; if (!A(O))
                return; (z = O.stopPropagation) == null || z.call(O); var L = (V = u.focusedKeyByPointer.get(m)) != null ? V : null, K = L ? u.inputs.get(L) : null, Y = L != null && u.fieldBounds.has(L) && K != null && typeof K.value == "string"; if (U === "Copy" && Y) {
                var yt = K, It = (J = yt.value) != null ? J : "", At = (st = (rt = yt.selections) == null ? void 0 : rt.get(m)) != null ? st : null, ft = At ? Math.max(0, Math.min(It.length, (mt = At.start) != null ? mt : 0)) : 0, pt = At ? Math.max(0, Math.min(It.length, (Tt = At.end) != null ? Tt : ft)) : ft, ct = Math.min(ft, pt), at = Math.max(ft, pt), Yt = ct !== at ? It.slice(ct, at) : It;
                u.clipboards.set(m, Yt);
            }
            else if (U === "Paste" && Y) {
                var yt = (Mt = u.clipboards.get(m)) != null ? Mt : "";
                if (yt.length > 0) {
                    var It = K, At = (Gt = It.value) != null ? Gt : "";
                    if (It.selections || (It.selections = new Map), !It.selections.has(m)) {
                        var Ft = At.length;
                        It.selections.set(m, { start: Ft, end: Ft });
                    }
                    var ft = It.selections.get(m), pt = Math.max(0, Math.min(At.length, (vt = ft.start) != null ? vt : At.length)), ct = Math.max(0, Math.min(At.length, (lt = ft.end) != null ? lt : pt)), at = Math.min(pt, ct), Yt = Math.max(pt, ct);
                    It.value = At.slice(0, at) + yt + At.slice(Yt);
                    var Nt = at + yt.length;
                    ft.start = Nt, ft.end = Nt;
                }
            } var N = u.contextMenus.get(m); N && (N.open = !1, u.contextMenus.set(m, N)), ot == null || ot(); }), v.addChild(X); }), a.addChild(v);
        };
        try {
            for (var _j = __values(u.contextMenus.entries()), _k = _j.next(); !_k.done; _k = _j.next()) {
                var _l = __read(_k.value, 2), m = _l[0], P = _l[1];
                _loop_3(m, P);
            }
        }
        catch (e_48_1) { e_48 = { error: e_48_1 }; }
        finally {
            try {
                if (_k && !_k.done && (_d = _j.return)) _d.call(_j);
            }
            finally { if (e_48) throw e_48.error; }
        }
        W("render:prune-cache");
        try {
            for (var _m = __values(b.entries()), _p = _m.next(); !_p.done; _p = _m.next()) {
                var _q = __read(_p.value, 2), m = _q[0], P = _q[1];
                if (!c.has(m)) {
                    try {
                        P.removeFromParent(), (F = P.destroy) == null || F.call(P, { children: !0 });
                    }
                    catch (v) { }
                    b.delete(m);
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
        W("render:done");
    }
    function xs() {
        return ze(this, null, function () {
            var t, e, n, r, _a, i_1, _b, o_3, s, l, a_2, h, f_1, d_1, b_1, c_1, p_1, y_1, k_2, M, _c, _, B, R_1, H, D_2, w_1, T_1, $_1, C_1, F_1, m_2, P_2, v_3, et_1, q_2, nt, Q, j_1, G_4, g, S, X, U_1, n_3, r;
            return __generator(this, function (_d) {
                switch (_d.label) {
                    case 0:
                        _d.trys.push([0, 9, , 10]);
                        St("main:start");
                        n = (t = document.getElementById("app")) != null ? t : document.body, r = !0;
                        St("main:yoga");
                        if (!r) return [3 /*break*/, 1];
                        _a = cs();
                        return [3 /*break*/, 3];
                    case 1: return [4 /*yield*/, Promise.resolve().then(function () { return (bi(), gi); })];
                    case 2:
                        _a = (_d.sent()).default;
                        _d.label = 3;
                    case 3:
                        it = _a, St("main:create-app");
                        i_1 = r ? ls() : new Je;
                        _b = r;
                        if (_b) return [3 /*break*/, 5];
                        return [4 /*yield*/, i_1.init({ background: "#ffffff", resizeTo: window, antialias: !1, preference: "webgl" })];
                    case 4:
                        _b = (_d.sent());
                        _d.label = 5;
                    case 5:
                        _b, St("main:attach-capture"), pi(i_1), window.__TRUEOS_PIXI_APP = i_1, St("main:append-canvas"), n.appendChild(i_1.canvas), i_1.ticker.stop(), St("main:capture-flags"), r && (u.harness.enabled = !1, u.virtualCursor.enabled = !1, window.__pixiCapture && (window.__pixiCapture.persist = !1)), St("main:canvas-listeners"), i_1.canvas.addEventListener("contextmenu", function (g) { return g.preventDefault(); }), i_1.canvas.addEventListener("wheel", function (g) { var K, Y; var S = (K = g.offsetX) != null ? K : 0, X = (Y = g.offsetY) != null ? Y : 0, x = function (N) { var J; if (!$t())
                            return; var z = window, V = Number((J = z.__TRUEOS_WHEEL_ROUTE_LOG_COUNT__) != null ? J : 0) || 0; V >= 32 || (z.__TRUEOS_WHEEL_ROUTE_LOG_COUNT__ = V + 1, console.log("[trueos pixi widgets] wheel-route ".concat(N))); }, I = null; for (var N = u.iframeRects.length - 1; N >= 0; N--) {
                            var z = u.iframeRects[N];
                            if (S >= z.x && S <= z.x + z.w && X >= z.y && X <= z.y + z.h) {
                                I = z.key;
                                break;
                            }
                        } var A = !1; if (I) {
                            var N = u.iframeScroll.get(I);
                            if (N) {
                                var z = Math.max(0, N.contentHeight - N.viewportHeight);
                                if (x("hit=iframe x=".concat(Math.round(S), " y=").concat(Math.round(X), " delta=").concat(Math.round(g.deltaY), " y0=").concat(Math.round(N.y), " max=").concat(Math.round(z))), z > 0) {
                                    var V = Math.max(0, Math.min(z, N.y + g.deltaY));
                                    V !== N.y && (N.y = V, u.iframeScroll.set(I, N), ot == null || ot(), g.preventDefault(), A = !0, x("owner=iframe y1=".concat(Math.round(V), " repaint=1")));
                                }
                            }
                        } if (A)
                            return; var O = Math.max(0, u.scroll.contentHeight - u.scroll.viewportHeight); if (O <= 0) {
                            x("owner=none x=".concat(Math.round(S), " y=").concat(Math.round(X), " delta=").concat(Math.round(g.deltaY), " root_y=").concat(Math.round(u.scroll.y), " root_max=0"));
                            return;
                        } var L = Math.max(0, Math.min(O, u.scroll.y + g.deltaY)); if (L !== u.scroll.y) {
                            var N = u.scroll.y;
                            u.scroll.y = L, ot == null || ot(), g.preventDefault(), x("owner=root x=".concat(Math.round(S), " y=").concat(Math.round(X), " delta=").concat(Math.round(g.deltaY), " y0=").concat(Math.round(N), " y1=").concat(Math.round(L), " max=").concat(Math.round(O), " repaint=1"));
                        }
                        else
                            x("owner=root-boundary x=".concat(Math.round(S), " y=").concat(Math.round(X), " delta=").concat(Math.round(g.deltaY), " y0=").concat(Math.round(u.scroll.y), " max=").concat(Math.round(O))); }, { passive: !1 }), St("main:stage:eventMode"), i_1.stage.eventMode = "static", St("main:stage:hitArea"), i_1.stage.hitArea = i_1.screen, St("main:stage:on:pointerdown"), i_1.stage.on("pointerdown", function (g) {
                            var e_55, _a;
                            var S, X, x, I, A, O;
                            if ((g == null ? void 0 : g.button) === 2) {
                                var L = Bt(g);
                                if (L > 0) {
                                    var K = (S = u.contextMenus.get(L)) != null ? S : { open: !1, x: 0, y: 0 };
                                    K.open = !0, K.x = (x = (X = g.global) == null ? void 0 : X.x) != null ? x : 0, K.y = (A = (I = g.global) == null ? void 0 : I.y) != null ? A : 0, u.contextMenus.set(L, K);
                                }
                                ot == null || ot(), (O = g.preventDefault) == null || O.call(g);
                                return;
                            }
                            if ((g == null ? void 0 : g.button) !== 2) {
                                var L = Bt(g), K = L > 0 ? u.contextMenus.get(L) : null;
                                K && K.open && (K.open = !1, u.contextMenus.set(L, K), ot == null || ot());
                            }
                            if ((g == null ? void 0 : g.button) !== 2) {
                                var L = !1;
                                try {
                                    for (var _b = __values(u.selects.values()), _c = _b.next(); !_c.done; _c = _b.next()) {
                                        var K = _c.value;
                                        K.open && (K.open = !1, L = !0);
                                    }
                                }
                                catch (e_55_1) { e_55 = { error: e_55_1 }; }
                                finally {
                                    try {
                                        if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
                                    }
                                    finally { if (e_55) throw e_55.error; }
                                }
                                L && (ot == null || ot());
                            }
                            (g == null ? void 0 : g.button) !== 2 && ai(u.temporals) && (ot == null || ot()), q_2();
                        }), St("main:stage:done"), St("main:roots");
                        o_3 = new wt, s = new wt;
                        s.eventMode = "static";
                        l = new wt;
                        l.eventMode = "none", i_1.stage.addChild(o_3), i_1.stage.addChild(s), i_1.stage.addChild(l);
                        a_2 = new _t;
                        a_2.label = "__trueosGlobalScrollbar", a_2.eventMode = "static", s.addChild(a_2);
                        h = function (g, S) { g.clear(); var X = S.half, x = S.strokeWidth, I = S.color; g.moveTo(-X, 0), g.lineTo(X, 0), g.stroke({ width: x, color: I }), g.moveTo(0, -X), g.lineTo(0, X), g.stroke({ width: x, color: I }); }, f_1 = new _t;
                        f_1.eventMode = "none", f_1.visible = !1, l.addChild(f_1);
                        d_1 = new _t;
                        d_1.eventMode = "none", d_1.visible = !1, l.addChild(d_1);
                        b_1 = new _t;
                        b_1.eventMode = "none", b_1.visible = !1, l.addChild(b_1);
                        c_1 = new _t;
                        c_1.eventMode = "none", l.addChild(c_1), St("main:text-measure");
                        p_1 = document.createElement("canvas").getContext("2d");
                        if (!p_1)
                            throw new Error("2D canvas not available");
                        p_1.font = "".concat(be.fontSize, "px ").concat(be.fontFamily);
                        y_1 = function (g) { return p_1.measureText(g).width; }, k_2 = be.fontSize * 1.25;
                        St("main:html");
                        if (!(typeof window.__TRUEOS_INPUT_HTML__ == "string")) return [3 /*break*/, 6];
                        _c = window.__TRUEOS_INPUT_HTML__;
                        return [3 /*break*/, 8];
                    case 6: return [4 /*yield*/, fetch("/input.html").then(function (g) { return g.text(); })];
                    case 7:
                        _c = _d.sent();
                        _d.label = 8;
                    case 8:
                        M = _c;
                        $t() && console.log("[trueos pixi widgets] input-html chars=".concat(M.length, " sample=\"").concat(hn(M), "\"")), St("main:render-tree"), wi.clear();
                        _ = qo(M), B = ts(), R_1 = ms(B.tree, _.rows), H = fs(R_1, _.rows);
                        if ($t() && (console.log("[trueos pixi widgets] text-fallback source=".concat(_.source, " rows=").concat(_.rows.length, " samples=").concat(zo(_.rows))), console.log("[trueos pixi widgets] render-tree source=".concat(B.source, " nodes=").concat(R_1.length, " trusted_text_applied=").concat(H))), R_1.length === 0)
                            throw new Error("TrueSurfer widget render tree is missing");
                        D_2 = $o(R_1), w_1 = null, T_1 = { nodes: 0, blocks: 0, text: 0, maxDepth: 0, tags: {} }, $_1 = 0, C_1 = function () { var g = Math.max(0, u.scroll.contentHeight - u.scroll.viewportHeight); u.scroll.y = Math.max(0, Math.min(u.scroll.y, g)); }, F_1 = function () { var g = i_1.renderer.width, S = i_1.renderer.height; u.scroll.viewportHeight = S; var X = u.scroll.contentHeight, x = Math.max(0, X - S), I = x > .5; if (a_2.clear(), a_2.visible = I, !I) {
                            u.scroll.track = { x: 0, y: 0, w: u.scroll.track.w, h: 0 }, u.scroll.thumb = { x: 0, y: 0, w: u.scroll.thumb.w, h: 0 };
                            return;
                        } var A = An, O = Pe, L = Math.max(0, g - O - A), K = A, Y = Math.max(0, S - A * 2), z = Math.max(24, S / Math.max(S, X) * Y), V = Math.max(1, Y - z), J = x <= 0 ? 0 : u.scroll.y / x, rt = K + V * J; u.scroll.track = { x: L, y: K, w: O, h: Y }, u.scroll.thumb = { x: L, y: rt, w: O, h: z }, a_2.rect(L, K, O, Y), a_2.fill({ color: 0, alpha: .06 }), a_2.rect(L, rt, O, z), a_2.fill({ color: 0, alpha: .25 }); }, m_2 = function () { if (w_1) {
                            if (St("main:paint:clamp"), C_1(), St("main:paint:render-to-pixi"), bs(i_1, w_1, o_3), St("main:paint:scrollbar"), F_1(), St("main:paint:renderer-render"), i_1.renderer.render(i_1.stage), as(D_2, T_1, Uo(R_1), Xo(w_1)), $t()) {
                                var g = is(w_1);
                                window.__TRUEOS_PIXI_LAYOUT_TEXT_OVERLAYS__ = g, $_1 < 4 && ($_1 += 1, console.log("[trueos pixi widgets] layout-text-overlays count=".concat(g.length, " samples=").concat(os(g))));
                            }
                            St("main:paint:done");
                        } };
                        $t() && (window.__TRUEOS_REPAINT_NOW__ = function () { window.__TRUEOS_PIXI_DIRTY__ = !1, m_2(); });
                        P_2 = function () { St("main:layout-build"); var g = gs(R_1, window.innerWidth, window.innerHeight); St("main:layout-commit"), w_1 = g, $t() && (window.__TRUEOS_PIXI_LAST_LAYOUT__ = g, window.__TRUEOS_PIXI_LAYOUT_TEXT_OVERLAYS__ = []), T_1 = Ho(g), u.scroll.contentHeight = us(g), u.scroll.viewportHeight = window.innerHeight, m_2(); };
                        dn = function () { P_2(); };
                        v_3 = !1, et_1 = !1, q_2 = function () { if ($t()) {
                            window.__TRUEOS_PIXI_DIRTY__ = !0;
                            return;
                        } et_1 || v_3 || (et_1 = !0, requestAnimationFrame(function () { et_1 = !1, i_1.renderer.render(i_1.stage); })); };
                        ot = function () { if (!v_3) {
                            if ($t()) {
                                window.__TRUEOS_PIXI_DIRTY__ = !0;
                                return;
                            }
                            v_3 = !0, requestAnimationFrame(function () { v_3 = !1, m_2(); });
                        } }, St("main:first-rerender"), P_2(), St("main:cursor-setup");
                        nt = 2, Q = 10, j_1 = $t();
                        h(f_1, { half: Q, strokeWidth: nt, color: Jt(Wt) }), h(d_1, { half: Q, strokeWidth: nt, color: Jt(Ht) }), h(b_1, { half: Q, strokeWidth: nt, color: Jt(Xt) });
                        G_4 = 2;
                        if (h(c_1, { half: Q, strokeWidth: nt, color: Jt(G_4) }), u.userCursorPos.set(Wt, { x: i_1.renderer.width * .25, y: i_1.renderer.height * .5 }), u.userCursorPos.set(Ht, { x: i_1.renderer.width * .25 + 40, y: i_1.renderer.height * .5 + 20 }), u.userCursorPos.set(Xt, { x: i_1.renderer.width * .25 + 80, y: i_1.renderer.height * .5 + 40 }), f_1.visible = !j_1, d_1.visible = !j_1, b_1.visible = !j_1, !j_1) {
                            g = u.userCursorPos.get(Wt), S = u.userCursorPos.get(Ht), X = u.userCursorPos.get(Xt);
                            f_1.position.set(g.x, g.y), d_1.position.set(S.x, S.y), b_1.position.set(X.x, X.y);
                        }
                        c_1.visible = !j_1 && u.virtualCursor.enabled;
                        U_1 = function () { if (j_1) {
                            f_1.visible = !1, d_1.visible = !1, b_1.visible = !1, c_1.visible = !1;
                            return;
                        } var g = u.userCursorPos.get(Wt), S = u.userCursorPos.get(Ht), X = u.userCursorPos.get(Xt); g && (f_1.visible = !0, f_1.position.set(g.x, g.y)), S && (d_1.visible = !0, d_1.position.set(S.x, S.y)), X && (b_1.visible = !0, b_1.position.set(X.x, X.y)); var x = function (I, A) { var O = null, L = null; for (var K = u.hoverRects.length - 1; K >= 0; K--) {
                            var Y = u.hoverRects[K];
                            if (I >= Y.x && I <= Y.x + Y.w && A >= Y.y && A <= Y.y + Y.h) {
                                O = Y.key, L = Y.cursor;
                                break;
                            }
                        } return { hitKey: O, hitCursor: L }; }; if (g) {
                            var _a = x(g.x, g.y), I = _a.hitKey, A = _a.hitCursor;
                            u.hoveredKeyByPointer.set(Wt, I), u.hoveredCursorByPointer.set(Wt, A);
                            var O = u.textDrags.has(Wt) || u.sliderDrags.has(Wt) || u.dialogDrags.has(Wt);
                            f_1.rotation = A != null || O ? Math.PI / 4 : 0;
                        } if (S) {
                            var _b = x(S.x, S.y), I = _b.hitKey, A = _b.hitCursor;
                            u.hoveredKeyByPointer.set(Ht, I), u.hoveredCursorByPointer.set(Ht, A);
                            var O = u.textDrags.has(Ht) || u.sliderDrags.has(Ht) || u.dialogDrags.has(Ht);
                            d_1.rotation = A != null || O ? Math.PI / 4 : 0;
                        } if (X) {
                            var _c = x(X.x, X.y), I = _c.hitKey, A = _c.hitCursor;
                            u.hoveredKeyByPointer.set(Xt, I), u.hoveredCursorByPointer.set(Xt, A);
                            var O = u.textDrags.has(Xt) || u.sliderDrags.has(Xt) || u.dialogDrags.has(Xt);
                            b_1.rotation = A != null || O ? Math.PI / 4 : 0;
                        } q_2(); };
                        u.harness.enabled && setInterval(function () {
                            var e_56, _a, e_57, _b;
                            var g = u.harness.activeUserPointerId, S = g === Wt ? Ht : g === Ht ? Xt : Wt;
                            if (u.harness.activeUserPointerId = S, u.lastMouse.has) {
                                var Y = u.userCursorPos.get(g), N = u.userCursorPos.get(S);
                                u.userCursorPos.set(S, { x: u.lastMouse.x, y: u.lastMouse.y }), N ? u.userCursorPos.set(g, { x: N.x, y: N.y }) : Y && u.userCursorPos.set(g, { x: Y.x, y: Y.y });
                            }
                            var X = u.textDrags.size > 0, x = u.sliderDrags.size > 0, I = u.dialogDrags.size > 0, A = u.scroll.draggingPointerId != null, O = u.color.draggingPointerId != null, L = !1;
                            try {
                                for (var _c = __values(u.iframeScroll.values()), _d = _c.next(); !_d.done; _d = _c.next()) {
                                    var Y = _d.value;
                                    if (Y.draggingPointerId != null) {
                                        L = !0;
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
                            var K = X || x || I || A || O || L;
                            u.textDrags.delete(Wt), u.textDrags.delete(Ht), u.textDrags.delete(Xt), u.sliderDrags.delete(Wt), u.sliderDrags.delete(Ht), u.sliderDrags.delete(Xt), u.dialogDrags.delete(Wt), u.dialogDrags.delete(Ht), u.dialogDrags.delete(Xt);
                            try {
                                for (var _f = __values([Wt, Ht, Xt]), _g = _f.next(); !_g.done; _g = _f.next()) {
                                    var Y = _g.value;
                                    var N = u.numberHolds.get(Y);
                                    N && (N.timeoutId != null && window.clearTimeout(N.timeoutId), N.intervalId != null && window.clearInterval(N.intervalId), u.numberHolds.delete(Y));
                                }
                            }
                            catch (e_57_1) { e_57 = { error: e_57_1 }; }
                            finally {
                                try {
                                    if (_g && !_g.done && (_b = _f.return)) _b.call(_f);
                                }
                                finally { if (e_57) throw e_57.error; }
                            }
                            (u.scroll.draggingPointerId === Wt || u.scroll.draggingPointerId === Ht || u.scroll.draggingPointerId === Xt) && (u.scroll.draggingPointerId = null), (u.color.draggingPointerId === Wt || u.color.draggingPointerId === Ht || u.color.draggingPointerId === Xt) && (u.color.draggingPointerId = null), U_1(), K && (ot == null || ot());
                        }, u.harness.periodMs), !j_1 && u.virtualCursor.enabled && i_1.ticker.add(function () { var A, O, L, K, Y; var g = Math.max(0, i_1.ticker.deltaMS) / 1e3; c_1.visible = !0, u.virtualCursor.t += g; var S = i_1.renderer.width * .75, X = i_1.renderer.height * .25, x = u.virtualCursor.t * u.virtualCursor.speed, I = u.virtualCursor.radius; u.virtualCursor.x = S + Math.cos(x) * I, u.virtualCursor.y = X + Math.sin(x) * I, c_1.position.set(u.virtualCursor.x, u.virtualCursor.y); {
                            var N = G_4, z = u.virtualCursor.x, V = u.virtualCursor.y, J = null, rt = null;
                            for (var Tt = u.hoverRects.length - 1; Tt >= 0; Tt--) {
                                var Mt = u.hoverRects[Tt];
                                if (z >= Mt.x && z <= Mt.x + Mt.w && V >= Mt.y && V <= Mt.y + Mt.h) {
                                    J = Mt.key, rt = Mt.cursor;
                                    break;
                                }
                            }
                            var st = (A = u.hoveredKeyByPointer.get(N)) != null ? A : null;
                            st !== J && (st && ((L = (O = u.hoverHandlers.get(st)) == null ? void 0 : O.out) == null || L.call(O)), J && ((Y = (K = u.hoverHandlers.get(J)) == null ? void 0 : K.over) == null || Y.call(K)), u.hoveredKeyByPointer.set(N, J)), u.hoveredCursorByPointer.set(N, rt);
                            var mt = u.textDrags.has(N) || u.sliderDrags.has(N) || u.dialogDrags.has(N);
                            c_1.rotation = rt != null || mt ? Math.PI / 4 : 0;
                        } }), u.virtualCursor.x = i_1.renderer.width * .75 + u.virtualCursor.radius, u.virtualCursor.y = i_1.renderer.height * .25, c_1.position.set(u.virtualCursor.x, u.virtualCursor.y), $t() && m_2(), i_1.stage.on("pointerup", function (g) {
                            var e_58, _a;
                            var x, I, A;
                            var S = Bt(g), X = (I = (x = u.sliderDrags.get(S)) == null ? void 0 : x.key) != null ? I : null;
                            u.textDrags.delete(S), u.sliderDrags.delete(S), u.dialogDrags.delete(S), u.scroll.draggingPointerId === S && (u.scroll.draggingPointerId = null), u.color.draggingPointerId === S && (u.color.draggingPointerId = null);
                            try {
                                for (var _b = __values(u.iframeScroll.values()), _c = _b.next(); !_c.done; _c = _b.next()) {
                                    var O = _c.value;
                                    O.draggingPointerId === S && (O.draggingPointerId = null);
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
                                var O = u.numberHolds.get(S);
                                O && (O.timeoutId != null && window.clearTimeout(O.timeoutId), O.intervalId != null && window.clearInterval(O.intervalId), u.numberHolds.delete(S));
                            }
                            if (X) {
                                var O = (A = u.temporalYearOwners.get(X)) != null ? A : null;
                                if (O) {
                                    var L = u.temporals.get(O);
                                    L && L.openYear && (L.openYear = !1, u.temporals.set(O, L), ot == null || ot());
                                }
                            }
                            q_2();
                        }), i_1.stage.on("pointerupoutside", function (g) {
                            var e_59, _a;
                            var x, I, A;
                            var S = Bt(g), X = (I = (x = u.sliderDrags.get(S)) == null ? void 0 : x.key) != null ? I : null;
                            u.textDrags.delete(S), u.sliderDrags.delete(S), u.dialogDrags.delete(S), u.scroll.draggingPointerId === S && (u.scroll.draggingPointerId = null), u.color.draggingPointerId === S && (u.color.draggingPointerId = null);
                            try {
                                for (var _b = __values(u.iframeScroll.values()), _c = _b.next(); !_c.done; _c = _b.next()) {
                                    var O = _c.value;
                                    O.draggingPointerId === S && (O.draggingPointerId = null);
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
                                var O = u.numberHolds.get(S);
                                O && (O.timeoutId != null && window.clearTimeout(O.timeoutId), O.intervalId != null && window.clearInterval(O.intervalId), u.numberHolds.delete(S));
                            }
                            if (X) {
                                var O = (A = u.temporalYearOwners.get(X)) != null ? A : null;
                                if (O) {
                                    var L = u.temporals.get(O);
                                    L && L.openYear && (L.openYear = !1, u.temporals.set(O, L), ot == null || ot());
                                }
                            }
                            q_2();
                        }), a_2.on("pointerdown", function (g) { var V, J, rt, st, mt, Tt; if ((g == null ? void 0 : g.button) === 2)
                            return; var S = Bt(g); if (S <= 0)
                            return; var X = (J = (V = g.global) == null ? void 0 : V.x) != null ? J : 0, x = (st = (rt = g.global) == null ? void 0 : rt.y) != null ? st : 0, I = u.scroll.track, A = u.scroll.thumb; if (!(X >= I.x && X <= I.x + I.w && x >= I.y && x <= I.y + I.h))
                            return; var L = Math.max(0, u.scroll.contentHeight - u.scroll.viewportHeight); if (L <= .5)
                            return; if (X >= A.x && X <= A.x + A.w && x >= A.y && x <= A.y + A.h) {
                            u.scroll.draggingPointerId = S, u.scroll.dragOffsetY = x - A.y, (mt = g.stopPropagation) == null || mt.call(g);
                            return;
                        } var Y = Math.max(1, I.h - A.h), N = Math.max(I.y, Math.min(I.y + Y, x - A.h / 2)), z = (N - I.y) / Y; u.scroll.y = Math.max(0, Math.min(L, z * L)), u.scroll.draggingPointerId = S, u.scroll.dragOffsetY = x - N, ot == null || ot(), (Tt = g.stopPropagation) == null || Tt.call(g); }), i_1.stage.on("pointermove", function (g) {
                            var e_60, _a;
                            var O, L, K, Y, N, z, V, J, rt, st, mt, Tt, Mt, Gt, vt, lt, yt, It, At, ft, pt, ct, at, Yt, Nt, Ft, se, re;
                            var S = Number((K = (L = g == null ? void 0 : g.pointerId) != null ? L : (O = g == null ? void 0 : g.data) == null ? void 0 : O.pointerId) != null ? K : 1);
                            if (String((z = (N = g == null ? void 0 : g.pointerType) != null ? N : (Y = g == null ? void 0 : g.data) == null ? void 0 : Y.pointerType) != null ? z : "").toLowerCase() === "mouse" || S === 1) {
                                var ut = (J = (V = g.global) == null ? void 0 : V.x) != null ? J : 0, gt = (st = (rt = g.global) == null ? void 0 : rt.y) != null ? st : 0;
                                u.lastMouse.x = ut, u.lastMouse.y = gt, u.lastMouse.has = !0, u.primaryMousePointerId = S;
                                var Et = u.harness.enabled ? u.harness.activeUserPointerId : S;
                                u.userCursorPos.set(Et, { x: ut, y: gt }), U_1();
                            }
                            var I = Bt(g);
                            if (I <= 0)
                                return;
                            var A = !1;
                            {
                                var ut = u.textDrags.get(I);
                                if (ut) {
                                    var gt = ut.key, Et = u.fieldBounds.get(gt), Dt = u.inputs.get(gt);
                                    if (Et && Dt && typeof Dt.value == "string") {
                                        var Lt = Et.isPassword ? "\u2022".repeat(Dt.value.length) : Dt.value, Kt = ue(ce(Lt, Math.max(0, Et.innerWidth), y_1), Et.maxLines), Z = ((Tt = (mt = g.global) == null ? void 0 : mt.x) != null ? Tt : 0) - Et.x - Et.innerLeft, ht = ((Gt = (Mt = g.global) == null ? void 0 : Mt.y) != null ? Gt : 0) - Et.y - Et.innerTop, Vt = Ee({ fullText: Lt, lines: Kt, localX: Z, localY: ht, lineHeight: k_2, measure: y_1 });
                                        Dt.selections || (Dt.selections = new Map), Dt.selections.set(I, { start: ut.anchor, end: Vt }), A = !0;
                                    }
                                }
                            }
                            {
                                var ut = u.sliderDrags.get(I);
                                if (ut) {
                                    var gt = ut.key, Et = u.sliderBounds.get(gt);
                                    if (Et) {
                                        var Lt = ((lt = (vt = g.global) == null ? void 0 : vt.x) != null ? lt : 0) - Et.x, Kt = Math.max(1, Et.w - Et.innerPad * 2), Z = (Lt - Et.innerPad) / Kt, ht = ye(u.sliders, gt, void 0);
                                        ht.value = Math.max(0, Math.min(1, Z)), A = !0;
                                    }
                                }
                            }
                            {
                                var ut = u.color.draggingPointerId;
                                if (ut != null && ut === I) {
                                    var gt = u.color.bounds;
                                    if (gt) {
                                        var Et = (It = (yt = g.global) == null ? void 0 : yt.x) != null ? It : 0, Dt = (ft = (At = g.global) == null ? void 0 : At.y) != null ? ft : 0, Lt = Et - gt.x, Kt = Dt - gt.y, Z = In({ lx: Lt, ly: Kt, w: gt.w, h: gt.h });
                                        Z && (u.color.rgb = Z, u.color.pick = { x: Lt, y: Kt }, A = !0);
                                    }
                                }
                            }
                            {
                                var ut = u.scroll.draggingPointerId;
                                if (ut != null && ut === I) {
                                    var gt = u.scroll.track, Et = u.scroll.thumb, Dt = Math.max(0, u.scroll.contentHeight - u.scroll.viewportHeight);
                                    if (Dt > .5 && gt.h > 0 && Et.h > 0) {
                                        var Lt = (ct = (pt = g.global) == null ? void 0 : pt.y) != null ? ct : 0, Kt = Math.max(1, gt.h - Et.h), ht = (Math.max(gt.y, Math.min(gt.y + Kt, Lt - u.scroll.dragOffsetY)) - gt.y) / Kt;
                                        u.scroll.y = Math.max(0, Math.min(Dt, ht * Dt)), A = !0;
                                    }
                                }
                            }
                            try {
                                for (var _b = __values(u.iframeScroll.values()), _c = _b.next(); !_c.done; _c = _b.next()) {
                                    var ut = _c.value;
                                    if (ut.draggingPointerId == null || ut.draggingPointerId !== I)
                                        continue;
                                    var gt = Math.max(0, ut.contentHeight - ut.viewportHeight);
                                    if (gt <= .5 || ut.track.h <= 0 || ut.thumb.h <= 0)
                                        continue;
                                    var Et = (Yt = (at = g.global) == null ? void 0 : at.y) != null ? Yt : 0, Dt = Math.max(1, ut.track.h - ut.thumb.h), Kt = (Math.max(ut.track.y, Math.min(ut.track.y + Dt, Et - ut.dragOffsetY)) - ut.track.y) / Dt;
                                    ut.y = Math.max(0, Math.min(gt, Kt * gt)), A = !0;
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
                                var ut = u.dialogDrags.get(I);
                                if (ut) {
                                    var gt = Qe(u.dialogs, ut.key), Et = (Ft = (Nt = g.global) == null ? void 0 : Nt.x) != null ? Ft : 0, Dt = (re = (se = g.global) == null ? void 0 : se.y) != null ? re : 0;
                                    gt.x = ut.originX + (Et - ut.startGX), gt.y = ut.originY + (Dt - ut.startGY);
                                    var Lt = u.dialogDragBounds.get(ut.key);
                                    Lt && (gt.x = Math.max(Lt.minX, Math.min(Lt.maxX, gt.x)), gt.y = Math.max(Lt.minY, Math.min(Lt.maxY, gt.y))), A = !0;
                                }
                            }
                            A && (ot == null || ot());
                        }), St("main:input-listeners"), window.addEventListener("keydown", function (g) {
                            var rt, st, mt, Tt, Mt, Gt, vt;
                            var S = u.keyboardOwnerPointerId, X = (rt = u.focusedKeyByPointer.get(S)) != null ? rt : null;
                            if (!X)
                                return;
                            var x = u.inputs.get(X);
                            if (!x || typeof x.value != "string")
                                return;
                            if (x.selections || (x.selections = new Map), !x.selections.has(S)) {
                                var lt = x.value.length;
                                x.selections.set(S, { start: lt, end: lt });
                            }
                            var I = x.selections.get(S), A = x.value.length, O = function (lt) { return Math.max(0, Math.min(A, lt)); }, L = O((st = I.start) != null ? st : A), K = O((mt = I.end) != null ? mt : L);
                            I.start = L, I.end = K;
                            var Y = Math.min(L, K), N = Math.max(L, K), z = Y !== N, V = function (lt) { var yt = Math.max(0, Math.min(x.value.length, lt)); I.start = yt, I.end = yt; }, J = function (lt, yt) { I.start = Math.max(0, Math.min(x.value.length, lt)), I.end = Math.max(0, Math.min(x.value.length, yt)); };
                            if (g.key.toLowerCase() === "a" && (g.ctrlKey || g.metaKey)) {
                                J(0, x.value.length), g.preventDefault(), m_2();
                                return;
                            }
                            if (g.key === "ArrowLeft" || g.key === "ArrowRight") {
                                var lt = g.key === "ArrowLeft" ? -1 : 1;
                                if (g.shiftKey) {
                                    var yt = (Tt = I.start) != null ? Tt : A, It = ((Mt = I.end) != null ? Mt : yt) + lt;
                                    J(yt, It);
                                }
                                else
                                    V((z ? Y : N) + lt);
                                g.preventDefault(), P_2();
                                return;
                            }
                            if (g.key === "Home") {
                                g.shiftKey ? J((Gt = I.start) != null ? Gt : A, 0) : V(0), g.preventDefault(), P_2();
                                return;
                            }
                            if (g.key === "End") {
                                g.shiftKey ? J((vt = I.start) != null ? vt : 0, x.value.length) : V(x.value.length), g.preventDefault(), P_2();
                                return;
                            }
                            if (g.key === "Backspace") {
                                if (z)
                                    x.value = x.value.slice(0, Y) + x.value.slice(N), V(Y);
                                else {
                                    var lt = N;
                                    lt > 0 && (x.value = x.value.slice(0, lt - 1) + x.value.slice(lt), V(lt - 1));
                                }
                                g.preventDefault(), P_2();
                                return;
                            }
                            if (g.key === "Enter") {
                                var lt = "\n";
                                if (z)
                                    x.value = x.value.slice(0, Y) + lt + x.value.slice(N), V(Y + lt.length);
                                else {
                                    var yt = N;
                                    x.value = x.value.slice(0, yt) + lt + x.value.slice(yt), V(yt + lt.length);
                                }
                                g.preventDefault(), P_2();
                                return;
                            }
                            if (g.key === "Delete") {
                                if (z)
                                    x.value = x.value.slice(0, Y) + x.value.slice(N), V(Y);
                                else {
                                    var lt = N;
                                    lt < x.value.length && (x.value = x.value.slice(0, lt) + x.value.slice(lt + 1), V(lt));
                                }
                                g.preventDefault(), P_2();
                                return;
                            }
                            if (g.key === "Escape") {
                                u.focusedKeyByPointer.set(S, null), P_2();
                                return;
                            }
                            if (g.key.length === 1 && !g.ctrlKey && !g.metaKey && !g.altKey) {
                                if (z)
                                    x.value = x.value.slice(0, Y) + g.key + x.value.slice(N), V(Y + 1);
                                else {
                                    var lt = N;
                                    x.value = x.value.slice(0, lt) + g.key + x.value.slice(lt), V(lt + 1);
                                }
                                g.preventDefault(), P_2();
                            }
                        }), window.addEventListener("resize", function () { P_2(), c_1.visible = u.virtualCursor.enabled; }), St("main:done"), r && (window.__TRUEOS_PIXI_APP_READY__ = !0, window.__TRUEOS_PIXI_APP_ERROR__ = "", window.__TRUEOS_PIXI_APP_PHASE__ = "ready");
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
    xs().then(function () { window.__TRUEOS_PIXI_APP_ERROR__ || (window.__TRUEOS_PIXI_APP_READY__ = !0, window.__TRUEOS_PIXI_APP_ERROR__ = "", window.__TRUEOS_PIXI_APP_PHASE__ = "ready"); }).catch(function (t) { var n; window.__TRUEOS_PIXI_APP_READY__ = !1, window.__TRUEOS_PIXI_APP_ERROR__ = Ii(t), console.error(t); var e = document.createElement("pre"); e.textContent = String((n = t == null ? void 0 : t.stack) != null ? n : t), document.body.appendChild(e); });
})();
