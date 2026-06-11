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
    var cr = Object.defineProperty, Qi = Object.defineProperties;
    var Zi = Object.getOwnPropertyDescriptors;
    var lr = Object.getOwnPropertySymbols;
    var qi = Object.prototype.hasOwnProperty, to = Object.prototype.propertyIsEnumerable;
    var Cn = function (t, e, n) { return e in t ? cr(t, e, { enumerable: !0, configurable: !0, writable: !0, value: n }) : t[e] = n; }, ae = function (t, e) {
        var e_1, _a;
        for (var n in e || (e = {}))
            qi.call(e, n) && Cn(t, n, e[n]);
        if (lr)
            try {
                for (var _b = __values(lr(e)), _c = _b.next(); !_c.done; _c = _b.next()) {
                    var n = _c.value;
                    to.call(e, n) && Cn(t, n, e[n]);
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
    }, Se = function (t, e) { return Qi(t, Zi(e)); };
    var eo = function (t, e) { return function () { return (t && (e = t(t = 0)), e); }; };
    var no = function (t, e) { for (var n in e)
        cr(t, n, { get: e[n], enumerable: !0 }); };
    var It = function (t, e, n) { return Cn(t, typeof e != "symbol" ? e + "" : e, n); };
    var rn = function (t, e, n) { return new Promise(function (r, i) { var o = function (a) { try {
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
    var Si = {};
    no(Si, { default: function () { return qo; } });
    var qo, Pi = eo(function () { qo = {}; });
    var Ue = /** @class */ (function () {
        function Ue(e, n) {
            if (e === void 0) { e = 0; }
            if (n === void 0) { n = 0; }
            It(this, "x");
            It(this, "y");
            this.x = Number(e) || 0, this.y = Number(n) || 0;
        }
        Ue.prototype.set = function (e, n) {
            if (e === void 0) { e = 0; }
            if (n === void 0) { n = e; }
            this.x = Number(e) || 0, this.y = Number(n) || 0;
        };
        return Ue;
    }()), Ot = /** @class */ (function () {
        function Ot(e, n, r, i) {
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
        return Ot;
    }()), Nn = /** @class */ (function () {
        function Nn() {
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
            this.parent = null, this.position = new Ue, this.scale = new Ue(1, 1), this.pivot = new Ue, this.visible = !0, this.alpha = 1, this.mask = null, this.rotation = 0, this.zIndex = 0, this.eventMode = null, this.cursor = null, this.hitArea = null, this.listeners = {};
        }
        Object.defineProperty(Nn.prototype, "x", {
            get: function () { return this.position.x; },
            set: function (e) { this.position.x = Number(e) || 0; },
            enumerable: false,
            configurable: true
        });
        Object.defineProperty(Nn.prototype, "y", {
            get: function () { return this.position.y; },
            set: function (e) { this.position.y = Number(e) || 0; },
            enumerable: false,
            configurable: true
        });
        Nn.prototype.on = function (e, n) { return this; };
        Nn.prototype.removeAllListeners = function (e) { return e == null ? this.listeners = {} : delete this.listeners[String(e)], this; };
        Nn.prototype.removeFromParent = function () { var e; return (e = this.parent) == null || e.removeChild(this), this; };
        Nn.prototype.destroy = function (e) { this.removeFromParent(), this.removeAllListeners(); };
        Nn.prototype.toLocal = function (e) { var n = e || {}; return { x: (Number(n.x) || 0) - this.getGlobalX(), y: (Number(n.y) || 0) - this.getGlobalY() }; };
        Nn.prototype.getGlobalPosition = function () { return { x: this.getGlobalX(), y: this.getGlobalY() }; };
        Nn.prototype.getGlobalX = function () { return (this.parent ? this.parent.getGlobalX() : 0) + this.x; };
        Nn.prototype.getGlobalY = function () { return (this.parent ? this.parent.getGlobalY() : 0) + this.y; };
        return Nn;
    }()), Ct = /** @class */ (function (_super) {
        __extends(Ct, _super);
        function Ct() {
            var _this = _super.call(this) || this;
            It(_this, "children");
            It(_this, "sortableChildren");
            _this.children = [], _this.sortableChildren = !1;
            return _this;
        }
        Ct.prototype.addChild = function () {
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
        Ct.prototype.addChildAt = function (n, r) { var o; (o = n.parent) == null || o.removeChild(n), n.parent = this; var i = Math.max(0, Math.min(Number(r) | 0, this.children.length)); return this.children.splice(i, 0, n), n; };
        Ct.prototype.removeChild = function () {
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
        Ct.prototype.removeChildren = function (n, r) {
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
        Ct.prototype.setChildIndex = function (n, r) { var i = this.children.indexOf(n); if (i < 0)
            return; this.children.splice(i, 1); var o = Math.max(0, Math.min(Number(r) | 0, this.children.length)); this.children.splice(o, 0, n); };
        Ct.prototype.getChildIndex = function (n) { return this.children.indexOf(n); };
        Ct.prototype.getChildByLabel = function (n) { for (var r = 0; r < this.children.length; r += 1) {
            var i = this.children[r];
            if (i && i.label === n)
                return i;
        } return null; };
        return Ct;
    }(Nn)), kt = /** @class */ (function (_super) {
        __extends(kt, _super);
        function kt() {
            var _this = _super.call(this) || this;
            It(_this, "commands");
            _this.commands = [];
            return _this;
        }
        kt.prototype.clear = function () { return this.commands.length = 0, this; };
        kt.prototype.rect = function (n, r, i, o) { return this.commands.push(["rect", n, r, i, o]), this; };
        kt.prototype.roundRect = function (n, r, i, o, s) {
            if (s === void 0) { s = 0; }
            return this.commands.push(["roundRect", n, r, i, o, s]), this;
        };
        kt.prototype.circle = function (n, r, i) { return this.commands.push(["circle", n, r, i]), this; };
        kt.prototype.ellipse = function (n, r, i, o) { return this.commands.push(["ellipse", n, r, i, o]), this; };
        kt.prototype.moveTo = function (n, r) { return this.commands.push(["moveTo", n, r]), this; };
        kt.prototype.lineTo = function (n, r) { return this.commands.push(["lineTo", n, r]), this; };
        kt.prototype.closePath = function () { return this.commands.push(["closePath"]), this; };
        kt.prototype.poly = function (n) { return this.commands.push(["poly", n]), this; };
        kt.prototype.fill = function (n) { return this.commands.push(["fill", n]), this; };
        kt.prototype.stroke = function (n) { return this.commands.push(["stroke", n]), this; };
        kt.prototype.image = function (n, r, i, o, s) { return this.commands.push(["image", n, r, i, o, s]), this; };
        kt.prototype.svg = function (n) { return this.commands.push(["svg", n]), this; };
        return kt;
    }(Ct)), re = /** @class */ (function (_super) {
        __extends(re, _super);
        function re(n) {
            if (n === void 0) { n = ""; }
            var r, i;
            var _this = _super.call(this) || this;
            It(_this, "_text");
            It(_this, "_style");
            It(_this, "_resolution");
            _this._text = "", _this._style = {}, _this._resolution = 1, typeof n == "string" ? _this._text = n : (_this._text = String((r = n.text) != null ? r : ""), _this._style = ae({}, (i = n.style) != null ? i : {}));
            return _this;
        }
        Object.defineProperty(re.prototype, "text", {
            get: function () { return this._text; },
            set: function (n) { this._text = String(n != null ? n : ""); },
            enumerable: false,
            configurable: true
        });
        Object.defineProperty(re.prototype, "style", {
            get: function () { return this._style; },
            set: function (n) { this._style = n != null ? n : {}; },
            enumerable: false,
            configurable: true
        });
        Object.defineProperty(re.prototype, "resolution", {
            get: function () { return this._resolution; },
            set: function (n) { this._resolution = Math.max(1, Number(n) || 1); },
            enumerable: false,
            configurable: true
        });
        Object.defineProperty(re.prototype, "width", {
            get: function () { var n = Number(this._style.fontSize) || 16; return this._text.length * n * .58; },
            enumerable: false,
            configurable: true
        });
        Object.defineProperty(re.prototype, "height", {
            get: function () { var n = Number(this._style.fontSize) || 16; return Number(this._style.lineHeight) || n * 1.25; },
            enumerable: false,
            configurable: true
        });
        re.prototype.setSize = function (n, r) { return this; };
        return re;
    }(Ct)), Ce = /** @class */ (function () {
        function Ce(e) {
            if (e === void 0) { e = {}; }
            It(this, "options");
            this.options = e;
        }
        Ce.prototype.addAttribute = function (e, n) { return this; };
        Ce.prototype.destroy = function () { };
        return Ce;
    }()), on = /** @class */ (function (_super) {
        __extends(on, _super);
        function on(n) {
            if (n === void 0) { n = {}; }
            var r, i;
            var _this = _super.call(this) || this;
            It(_this, "geometry");
            It(_this, "shader");
            _this.geometry = (r = n.geometry) != null ? r : new Ce, _this.shader = (i = n.shader) != null ? i : new Be;
            return _this;
        }
        return on;
    }(Ct)), sn = /** @class */ (function () {
        function sn(e) {
            if (e === void 0) { e = {}; }
            It(this, "options");
            this.options = e;
        }
        return sn;
    }()), An = { VERTEX: 1, COPY_DST: 2 }, Be = /** @class */ (function () {
        function Be(e) {
            if (e === void 0) { e = {}; }
            It(this, "options");
            this.options = e;
        }
        return Be;
    }());
    var ur = "", dr = "", hr = "", an = /** @class */ (function () {
        function an() {
            var _this = this;
            It(this, "stage");
            It(this, "screen");
            It(this, "canvas");
            It(this, "renderer");
            It(this, "ticker");
            var e = Math.max(1, Number(globalThis.innerWidth || 1920) | 0), n = Math.max(1, Number(globalThis.innerHeight || 1080) | 0);
            this.stage = new Ct, this.screen = new Ot(0, 0, e, n), this.canvas = document.createElement("canvas"), this.ticker = { stop: function () { }, add: function () { }, remove: function () { } }, this.renderer = { width: e, height: n, screen: this.screen, render: function (r) { return r; }, resize: function (r, i) { var o = Math.max(1, Number(r || e) | 0), s = Math.max(1, Number(i || n) | 0); _this.renderer.width = o, _this.renderer.height = s, _this.screen.width = o, _this.screen.height = s; } };
        }
        an.prototype.init = function (e) { return rn(this, null, function () { return __generator(this, function (_a) {
            return [2 /*return*/];
        }); }); };
        return an;
    }());
    var ee = { fontFamily: "system-ui, -apple-system, Segoe UI, Arial", fontSize: 16, background: 16777215, text: 1118481, mutedText: 6710886, boxBorder: 14540253, hr: 13421772, control: { border: 0, focusBorder: 3900150, background: 16777215, accent: 3900150, radius: 0, button: { fill: 15921906, hoverFill: 15395562, activeFill: 14737632, border: 6710886, text: 1118481, radius: 0 }, progress: { border: 10066329, background: 16777215, fill: 6990335 }, table: { border: 10066329, cellBorder: 11579568, headerFill: 16250871 } } };
    var Ne = 24, Pt = 1;
    function ie(t) { var i, o; var e = t.wrapWidth, n = (i = t.wordWrap) != null ? i : e != null, r = (o = t.wordWrapWidth) != null ? o : e == null ? void 0 : Math.max(0, Math.ceil(e) + Ne); return new re({ text: t.text, style: { fontFamily: t.fontFamily, fontSize: t.fontSize, fill: t.fill, fontWeight: t.bold ? "700" : "400", wordWrap: n, wordWrapWidth: r } }); }
    function Ln(t, e) { var n = t.children; if (!Array.isArray(n))
        return null; for (var r = 0; r < n.length; r += 1) {
        var i = n[r];
        if (i && i.label === e)
            return i;
    } return null; }
    function ue(t, e) { var n = Ln(t, e); if (n)
        return n; var r = new Ct; return r.label = e, t.addChild(r), r; }
    function $t(t, e) { var n = Ln(t, e); if (n)
        return n; var r = new kt; return r.label = e, t.addChild(r), r; }
    function vt(t, e, n) { var r = Ln(t, e); if (r)
        return r; var i = new re({ text: "" }); return i.label = e, n == null || n(i), t.addChild(i), i; }
    function Gt(t) { t.clear(), t.removeAllListeners(), t.hitArea = null; }
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
    function Ae(t) { var e = t.fullText, n = t.lines, r = t.localX, i = t.localY, o = t.lineHeight, s = t.measure; if (n.length === 0)
        return 0; var c = Math.max(0, r), a = Math.max(0, i), h = Math.max(1, o), m = Math.max(0, Math.min(n.length - 1, Math.floor(a / h))), d = n[m], b = d.start, y = Number.POSITIVE_INFINITY; for (var w = d.start; w <= d.end; w++) {
        var u = s(e.slice(d.start, w)), _ = Math.abs(u - c);
        _ < y && (y = _, b = w);
    } return b; }
    function mr(t) { var w, u, _, p; var e = t.node, n = t.graphics, r = t.w, i = t.h, o = t.theme, s = Math.max(0, Math.round(r)), c = Math.max(0, Math.round(i)); n.rect(.5, .5, Math.max(0, s - 1), Math.max(0, c - 1)), n.fill(o.control.progress.background), n.stroke({ width: 1, color: o.control.progress.border }); var a = Number((u = (w = e.attrs) == null ? void 0 : w.value) != null ? u : "0"), h = Number((p = (_ = e.attrs) == null ? void 0 : _.max) != null ? p : "1"), m = h > 0 ? Math.max(0, Math.min(1, a / h)) : 0, d = 3, b = Math.max(0, s - d * 2), y = Math.max(0, c - d * 2); n.rect(d, d, Math.max(0, b * m), y), n.fill(o.control.progress.fill); }
    function fr(t, e) { t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0), t.setHeight(14), t.setMinWidth(240); }
    function Pe(t, e, n) { var c; var r = t.get(e); if (r)
        return r; var i = Number((c = n == null ? void 0 : n.value) != null ? c : "0"), o = Number.isFinite(i) ? i : 0, s = { value: Math.max(0, Math.min(1, o)) }; return t.set(e, s), s; }
    function pr(t, e) { t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0), t.setHeight(14), t.setMinWidth(240); }
    function gr(t) { var e = t.node, n = t.Yoga, r = t.measurer, i = n.Node.create(); return i.setPadding(n.EDGE_LEFT, 0), i.setPadding(n.EDGE_RIGHT, 0), i.setPadding(n.EDGE_TOP, 0), i.setPadding(n.EDGE_BOTTOM, 0), i.setMargin(n.EDGE_RIGHT, 6), i.setMeasureFunc(function () { var o = r.measure("100"); return { width: o.width, height: o.height }; }), { yogaNode: i, buildBox: function () { return ({ kind: "block", key: e.key, tagName: e.tagName, attrs: e.attrs, x: i.getComputedLeft(), y: i.getComputedTop(), width: i.getComputedWidth(), height: i.getComputedHeight(), children: [] }); } }; }
    function br(t) { var h, m; var e = t.node, n = t.container, r = t.theme, i = t.sliderStates, o = (h = e.attrs) == null ? void 0 : h["data-slider-key"], s = null; if (o) {
        var d = i.get(o);
        if (d)
            s = d;
        else {
            var b = (m = e.attrs) == null ? void 0 : m["data-slider-init"];
            s = Pe(i, o, b != null ? { value: String(b) } : void 0);
        }
    } var c = s ? Math.round(s.value * 100) : 0, a = vt(n, "__pct", function (d) { d.style = { fontFamily: r.fontFamily, fontSize: r.fontSize, fill: r.text, fontWeight: "400", wordWrap: !1 }; }); a.text = String(c), a.position.set(0, Pt); }
    function ln(t) { var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.absX, c = t.absY, a = t.theme, h = t.sliderStates, m = t.sliderBounds, d = t.sliderDrags, b = t.requestPaint, y = e.key, w = y ? Pe(h, y, e.attrs) : null, u = Math.max(0, Math.round(i)), _ = Math.max(0, Math.round(o)), p = 3; y && m.set(y, { x: s, y: c, w: u, h: _, innerPad: p }), r.rect(.5, .5, Math.max(0, u - 1), Math.max(0, _ - 1)), r.fill(a.control.progress.background), r.stroke({ width: 1, color: a.control.progress.border }); var R = w ? Math.max(0, Math.min(1, w.value)) : 0, A = Math.max(0, u - p * 2), N = Math.max(0, _ - p * 2); r.rect(p, p, Math.max(0, A * R), N), r.fill(a.control.progress.fill); var D = p + A * R, x = N / 2; r.moveTo(D, p - x), r.lineTo(D, p + N + x), r.stroke({ width: 2, color: a.text }), y && (Jt(n), n.eventMode = "static", n.cursor = "pointer", n.hitArea = new Ot(0, 0, Math.max(0, u), Math.max(0, _)), n.on("pointerdown", function (C) {
        var e_5, _a;
        var W, tt, it, st, z, nt;
        if ((C == null ? void 0 : C.button) === 2)
            return;
        var H = t.getPointerId ? t.getPointerId(C) : Number((it = (tt = C == null ? void 0 : C.pointerId) != null ? tt : (W = C == null ? void 0 : C.data) == null ? void 0 : W.pointerId) != null ? it : 0);
        if (H <= 0)
            return;
        try {
            for (var _b = __values(d.entries()), _c = _b.next(); !_c.done; _c = _b.next()) {
                var _d = __read(_c.value, 2), L = _d[0], V = _d[1];
                V.key === y && L !== H && d.delete(L);
            }
        }
        catch (e_5_1) { e_5 = { error: e_5_1 }; }
        finally {
            try {
                if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
            }
            finally { if (e_5) throw e_5.error; }
        }
        d.set(H, { key: y });
        var k = m.get(y), S = (z = (st = C.global) == null ? void 0 : st.x) != null ? z : 0, X = k ? S - k.x : 0, j = k ? Math.max(1, k.w - k.innerPad * 2) : 1, f = (X - ((nt = k == null ? void 0 : k.innerPad) != null ? nt : 0)) / j, G = Pe(h, y, e.attrs);
        G.value = Math.max(0, Math.min(1, f)), b == null || b();
    })); }
    var Xe = new Map;
    function ro(t) { var n; if (!t || !Object.prototype.hasOwnProperty.call(t, "data-details-open"))
        return !1; var e = String((n = t["data-details-open"]) != null ? n : "").trim().toLowerCase(); return e !== "0" && e !== "false" && e !== "no"; }
    function io(t) { var n; var e = t.key; return e && e.endsWith(":summary") ? "".concat(e.slice(0, -8), ":details") : (n = t.attrs) == null ? void 0 : n["data-details-key"]; }
    function _r(t) { var _; var e = t.node, n = t.container, r = t.w, i = t.h, o = t.theme, s = t.detailsOpen, c = t.requestRerender, a = io(e), h = ro(e.attrs), m = a && s.has(a) ? s.get(a) === !0 : h, d = function (p) { var R, A, N; return Number((N = (A = p == null ? void 0 : p.pointerId) != null ? A : (R = p == null ? void 0 : p.data) == null ? void 0 : R.pointerId) != null ? N : 1) || 1; }, b = function (p) { var N; if (!a || (p == null ? void 0 : p.button) === 2)
        return; var A = !(s.has(a) ? s.get(a) === !0 : h); s.set(a, A), c == null || c(), (N = p == null ? void 0 : p.stopPropagation) == null || N.call(p); }, y = 16, w = (_ = n.children) == null ? void 0 : _.find(function (p) { return (p == null ? void 0 : p.label) === "__arrow"; }); w && (Gt(w), w.visible = !1); var u = vt(n, "__arrowText", function (p) { p.style = { fontFamily: o.fontFamily, fontSize: o.fontSize, fill: o.text, fontWeight: "700" }; }); u.visible = !0, u.text = m ? "v" : ">", u.style.fontFamily = o.fontFamily, u.style.fontSize = o.fontSize, u.style.fill = o.text, u.style.fontWeight = "700", u.position.set(5, Math.max(0, (i - o.fontSize) / 2) + Pt), a && (Jt(n), n.eventMode = "static", n.cursor = "pointer", n.hitArea = new Ot(0, 0, Math.max(0, r), Math.max(0, i)), n.on("pointerdown", function (p) { var R; (p == null ? void 0 : p.button) !== 2 && (Xe.set(d(p), a), (R = p.stopPropagation) == null || R.call(p)); }), n.on("pointerup", function (p) { if ((p == null ? void 0 : p.button) === 2)
        return; var R = d(p), A = Xe.get(R); Xe.delete(R), A === a && b(p); }), n.on("pointerupoutside", function (p) { var R = d(p); Xe.get(R) === a && Xe.delete(R); })); }
    function yr(t, e) { t.setFlexDirection(e.FLEX_DIRECTION_ROW), t.setAlignItems(e.ALIGN_CENTER), t.setJustifyContent(e.JUSTIFY_SPACE_BETWEEN), t.setPadding(e.EDGE_TOP, 6), t.setPadding(e.EDGE_BOTTOM, 6), t.setPadding(e.EDGE_LEFT, 26), t.setPadding(e.EDGE_RIGHT, 12), t.setMinHeight(36); }
    function xr(t, e) { t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0); }
    function wr(t, e) { var i, o, s; if (!t || t.tagName !== "details" || !t.key)
        return (i = t == null ? void 0 : t.children) != null ? i : []; var n = t.attrs ? Object.prototype.hasOwnProperty.call(t.attrs, "open") : !1; return (e.has(t.key) ? e.get(t.key) === !0 : n) ? (o = t.children) != null ? o : [] : ((s = t.children) != null ? s : []).filter(function (c) { return c && c.kind === "block" && c.tagName === "summary"; }); }
    function Tr(t) { var e = t.graphics, n = t.w, r = t.theme; e.rect(0, 0, Math.round(n), 1), e.fill(r.hr); }
    function Er(t, e) { t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0), t.setMargin(e.EDGE_TOP, 2), t.setMargin(e.EDGE_BOTTOM, 2), t.setHeight(1); }
    function Ir(t) { var R, A; var e = t.container, n = t.graphics, r = t.w, i = t.h, o = t.label, s = t.theme, c = t.registerHoverHandlers, a = t.publishFastPath, h = function (N) { n.clear(); var D = 1, x = D / 2; s.control.button.radius > 0 ? n.roundRect(x, x, Math.max(0, r - D), Math.max(0, i - D), s.control.button.radius) : n.rect(x, x, Math.max(0, r - D), Math.max(0, i - D)), n.fill(N), n.stroke({ width: D, color: s.control.button.border }); }, m = function (N) { h(N), a == null || a(N); }; h(s.control.button.fill); var d = vt(e, "__label", function (N) { N.style = { fontFamily: s.fontFamily, fontSize: s.fontSize, fill: s.control.button.text, fontWeight: "400", wordWrap: !1, wordWrapWidth: 0 }; }), b = String(o != null ? o : "").trim(); d.text = b, d.visible = b.length > 0, d.style = Se(ae({}, d.style), { fontFamily: s.fontFamily, fontSize: s.fontSize, fill: s.control.button.text, wordWrap: !1, wordWrapWidth: Math.max(0, Math.ceil(r - 16)) }); var y = Number((R = d.width) != null ? R : 0), w = Number((A = d.height) != null ? A : 0), u = s.fontSize * 1.25; d.position.set(y > 0 ? Math.max(8, Math.floor((r - y) / 2)) : 8, Math.max(0, Math.floor((i - (w > 0 ? w : u)) / 2)) + Pt); var _ = function () { return m(s.control.button.hoverFill); }, p = function () { return m(s.control.button.fill); }; c == null || c({ over: _, out: p }), Jt(e), e.eventMode = "static", e.cursor = "pointer", e.on("pointerover", _), e.on("pointerout", p), e.on("pointerdown", function () { return m(s.control.button.activeFill); }), e.on("pointerup", function () { return m(s.control.button.hoverFill); }); }
    function Mr(t, e) { t.setFlexDirection(e.FLEX_DIRECTION_ROW), t.setPadding(e.EDGE_TOP, 6), t.setPadding(e.EDGE_BOTTOM, 6), t.setMinHeight(36), t.setMinWidth(100), t.setAlignItems(e.ALIGN_CENTER), t.setJustifyContent(e.JUSTIFY_CENTER); }
    function Sr(t) { var e = t.graphics, n = t.w, r = t.h, i = t.boxBorder, o = Math.max(0, Math.round(n)), s = Math.max(0, Math.round(r)); e.rect(0, 0, o, s), e.stroke({ width: 1, color: i, alignment: 0 }); }
    function Pr(t) { var e = t.nodeTag, n = t.graphics, r = t.w, i = t.h, o = t.theme; e === "th" && (n.rect(0, 0, r, i), n.fill(o.control.table.headerFill)), n.rect(0, 0, r, i), n.stroke({ width: 1, color: o.control.table.cellBorder }); }
    function Rr(t, e) { t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0); }
    function kr(t, e) { t.setFlexDirection(e.FLEX_DIRECTION_ROW), t.setFlexWrap(e.WRAP_NO_WRAP), t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0), t.setMargin(e.EDGE_BOTTOM, 0); }
    function Or(t, e) { t.setFlexGrow(1), t.setFlexShrink(1), t.setMinWidth(80), t.setPadding(e.EDGE_LEFT, 8), t.setPadding(e.EDGE_RIGHT, 8), t.setPadding(e.EDGE_TOP, 6), t.setPadding(e.EDGE_BOTTOM, 6), t.setMargin(e.EDGE_BOTTOM, 0); }
    function Dn(t) { var e = String(t != null ? t : "").toLowerCase(); if (e.length !== 2 || e.charAt(0) !== "h")
        return !1; var n = e.charCodeAt(1); return n >= 49 && n <= 54; }
    function Cr(t, e) { t.setPadding(e.EDGE_TOP, 6), t.setPadding(e.EDGE_BOTTOM, 6), t.setMinHeight(36), t.setJustifyContent(e.JUSTIFY_CENTER); }
    function Nr(t, e) {
        var n = Math.max(1, Math.floor(t)), r = Math.max(1, Math.floor(e));
        return "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<svg viewBox=\"0 0 ".concat(n, " ").concat(r, "\" xmlns=\"http://www.w3.org/2000/svg\">\n  <rect x=\"0\" y=\"0\" width=\"").concat(n, "\" height=\"").concat(r, "\" fill=\"#f6f6f6\"/>\n  <rect x=\"0.5\" y=\"0.5\" width=\"").concat(Math.max(0, n - 1), "\" height=\"").concat(Math.max(0, r - 1), "\" fill=\"none\" stroke=\"#999\"/>\n  <path d=\"M2 2 L").concat(Math.max(2, n - 2), " ").concat(Math.max(2, r - 2), "\" stroke=\"#c8c8c8\"/>\n  <path d=\"M").concat(Math.max(2, n - 2), " 2 L2 ").concat(Math.max(2, r - 2), "\" stroke=\"#c8c8c8\"/>\n</svg>");
    }
    function Ar(_a) {
        var _b = _a === void 0 ? {} : _a, _c = _b.ring, t = _c === void 0 ? 34 : _c, _d = _b.core, e = _d === void 0 ? 14 : _d, _f = _b.hueA, n = _f === void 0 ? "#00e5ff" : _f, _g = _b.hueB, r = _g === void 0 ? "#ff2bd6" : _g;
        var i = Math.max(0, t - 10), o = Math.max(0, e * .35);
        return "\n<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 100 100\">\n  <rect width=\"100\" height=\"100\" fill=\"#ffffff\"/>\n  <rect width=\"100\" height=\"100\" fill=\"".concat(n, "\" opacity=\"0.08\"/>\n\n  <circle cx=\"50\" cy=\"50\" r=\"").concat(t, "\" fill=\"none\" stroke=\"").concat(r, "\" stroke-width=\"4\" opacity=\"0.95\"/>\n  <circle cx=\"50\" cy=\"50\" r=\"").concat(i, "\" fill=\"none\" stroke=\"").concat(n, "\" stroke-width=\"1\" opacity=\"0.35\"/>\n\n  <circle cx=\"50\" cy=\"50\" r=\"").concat(e, "\" fill=\"").concat(n, "\" opacity=\"0.9\"/>\n  <circle cx=\"43\" cy=\"43\" r=\"").concat(o, "\" fill=\"#ffffff\" opacity=\"0.55\"/>\n\n  <path d=\"M50 16 L52 22 L58 24 L52 26 L50 32 L48 26 L42 24 L48 22 Z\" fill=\"#ffffff\" opacity=\"0.85\"/>\n  <path d=\"M82 52 L85 56 L90 57 L85 58 L82 62 L79 58 L74 57 L79 56 Z\" fill=\"#ffffff\" opacity=\"0.70\"/>\n  <path d=\"M20 70 L22 74 L27 75 L22 76 L20 80 L18 76 L13 75 L18 74 Z\" fill=\"#ffffff\" opacity=\"0.65\"/>\n</svg>\n");
    }
    var Lr = new Map;
    function Ye() { var t = globalThis; return !0; }
    function Gr(t) { var e = globalThis, n = String(t != null ? t : "").trim(); if (!n)
        return null; var r = e.__TRUEOS_READY_IMAGE_TEXTURE_CACHE__; (!r || typeof r.get != "function" || typeof r.set != "function") && (r = new Map, e.__TRUEOS_READY_IMAGE_TEXTURE_CACHE__ = r); var i = r.get(n); if (i)
        return i; var o = e.__trueosResolveReadyImageTexture; if (typeof o != "function")
        return null; var s = o(n); if (s && typeof s.then == "function") {
        var c = s.then(function (a) { return (r.set(n, a), a); }).catch(function (a) { throw r.delete(n), a; });
        return r.set(n, c), c;
    } return r.set(n, s), s; }
    function oo(t) { var s; var e = globalThis, n = String(t != null ? t : "").trim(); if (!n)
        return null; var r = e.__trueosPeekReadyImageTexture; if (typeof r != "function")
        return null; var i = r(n); return !i || typeof i.then == "function" ? null : Math.max(0, Number((s = i == null ? void 0 : i.texId) != null ? s : 0) | 0) > 0 ? i : null; }
    function so(t, e) { var n = globalThis, r = String(t != null ? t : "").trim(); if (!r || !e)
        return; var i = n.__TRUEOS_READY_IMAGE_TEXTURE_CACHE__; (!i || typeof i.get != "function" || typeof i.set != "function") && (i = new Map, n.__TRUEOS_READY_IMAGE_TEXTURE_CACHE__ = i), i.set(r, e); }
    function Hr(t, e) { var r, i, o, s, c; var n = Math.max(0, Number((r = e == null ? void 0 : e.texId) != null ? r : 0) | 0); if (n <= 0)
        throw new Error("image texture missing"); t.state = "ready", t.texId = n, t.width = Math.max(0, Number((o = (i = e == null ? void 0 : e.pixelWidth) != null ? i : e == null ? void 0 : e.width) != null ? o : 0) | 0), t.height = Math.max(0, Number((c = (s = e == null ? void 0 : e.pixelHeight) != null ? s : e == null ? void 0 : e.height) != null ? c : 0) | 0); }
    function ao(t, e) { var n = oo(t) || Gr(t); return !n || typeof n.then == "function" ? !1 : (Hr(e, n), so(t, n), !0); }
    function Dr(t, e) { var n = String(t != null ? t : "").trim(); if (!n)
        return null; var r = Lr.get(n); if (r) {
        if (Ye() && r.state === "loading")
            try {
                ao(n, r);
            }
            catch (c) {
                r.state = "error";
            }
        return r;
    } if (Ye())
        return null; var i = { state: "loading", texId: 0, width: 0, height: 0 }; Lr.set(n, i); var o = function (c) { Hr(i, c), Ye() || e == null || e(); }, s = function () { i.state = "error", Ye() || e == null || e(); }; try {
        var c = Gr(n);
        if (!c)
            return i;
        if (c && typeof c.then == "function") {
            if (Ye())
                return i;
            c.then(o).catch(s);
        }
        else
            o(c);
    }
    catch (c) {
        s();
    } return i; }
    function lo(t) { var e = String(t != null ? t : ""); if (!e.startsWith("data:image/svg+xml"))
        return null; var n = e.indexOf(","); if (n === -1)
        return null; var r = e.slice(0, n).toLowerCase(), i = e.slice(n + 1); try {
        return r.includes(";base64") ? atob(i) : decodeURIComponent(i);
    }
    catch (o) {
        return null;
    } }
    function co(t) { return vr(vr(String(t), "tspan"), "text"); }
    function uo(t) { return "data:image/svg+xml;charset=utf-8,".concat(encodeURIComponent(t)); }
    function vr(t, e) { var n = "", r = 0, i = t.toLowerCase(), o = "<" + e, s = "</" + e; for (; r < t.length;) {
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
    function Wr(t) { var N, D, x, C; var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.theme, c = t.requestRerender, a = (D = (N = e.attrs) == null ? void 0 : N.alt) != null ? D : "", h = (C = (x = e.attrs) == null ? void 0 : x.src) != null ? C : "", m = h.trim().length > 0, d = a.trim().length > 0 ? a : h.trim().length > 0 ? h : "img", b = r.image, y = m ? Dr(h, c) : null; if ((y == null ? void 0 : y.state) === "ready" && y.texId > 0 && typeof b == "function") {
        b.call(r, y.texId, 0, 0, Math.max(0, i), Math.max(0, o));
        var H = n.getChildByLabel ? n.getChildByLabel("__label") : n.children.find(function (k) { return (k == null ? void 0 : k.label) === "__label"; });
        H && (H.visible = !1);
        return;
    } var w = m ? lo(h) : null, u = co(w != null ? w : m ? Nr(i, o) : Ar({ ring: 34, core: 14 })), _ = $t(n, "__svg"), p = Dr(uo(u), c); if ((p == null ? void 0 : p.state) === "ready" && p.texId > 0 && typeof _.image == "function") {
        var H = "texture:".concat(p.texId, ":").concat(Math.round(i), "x").concat(Math.round(o));
        if (_.__key !== H && (Gt(_), _.image(p.texId, 0, 0, Math.max(0, i), Math.max(0, o)), _.__key = H), _.scale.set(1), _.position.set(0, 0), !m) {
            var k = n.getChildByLabel ? n.getChildByLabel("__label") : n.children.find(function (S) { return (S == null ? void 0 : S.label) === "__label"; });
            k && (k.visible = !1);
            return;
        }
        if (d.trim().length > 0) {
            var k = vt(n, "__label", function (S) { S.style = { fontFamily: s.fontFamily, fontSize: s.fontSize, fill: s.mutedText, fontWeight: "400", wordWrap: !0, wordWrapWidth: 0 }; });
            k.text = d, k.style.wordWrapWidth = Math.max(0, Math.ceil(i - 16) + Ne), k.position.set(8, 8 + Pt), k.visible = !0;
        }
        return;
    }
    else
        Gt(_); var R = _.svg; if (0 && _.__key !== H)
        try { }
        catch (S) { } r.rect(.5, .5, Math.max(0, i - 1), Math.max(0, o - 1)), r.fill(16185078), r.stroke({ width: 1, color: s.control.border }); var A = vt(n, "__label", function (H) { H.style = { fontFamily: s.fontFamily, fontSize: s.fontSize, fill: s.mutedText, fontWeight: "400", wordWrap: !0, wordWrapWidth: 0 }; }); A.text = d, A.style.wordWrapWidth = Math.max(0, Math.ceil(i - 16) + Ne), A.position.set(8, 8 + Pt); }
    function Fr(t, e, n) { var h, m, d, b; t.setPadding(n.EDGE_LEFT, 0), t.setPadding(n.EDGE_RIGHT, 0), t.setPadding(n.EDGE_TOP, 0), t.setPadding(n.EDGE_BOTTOM, 0); var r = Number((m = (h = e.attrs) == null ? void 0 : h.width) != null ? m : "0"), i = Number((b = (d = e.attrs) == null ? void 0 : d.height) != null ? b : "0"), o = Number.isFinite(r) && r > 0, s = Number.isFinite(i) && i > 0, c = o ? r : 240, a = s ? i : 140; (o || s) && (t.setAlignSelf(n.ALIGN_FLEX_START), t.setFlexGrow(0), t.setFlexShrink(0)), t.setWidth(c), t.setHeight(a), t.setMinWidth(120), t.setMinHeight(80); }
    var $r = new Map;
    function Ke() { var t = globalThis; return !0; }
    function Br(t) { var e = globalThis, n = String(t != null ? t : "").trim(); if (!n)
        return null; var r = e.__TRUEOS_READY_IMAGE_TEXTURE_CACHE__; (!r || typeof r.get != "function" || typeof r.set != "function") && (r = new Map, e.__TRUEOS_READY_IMAGE_TEXTURE_CACHE__ = r); var i = r.get(n); if (i)
        return i; var o = e.__trueosResolveReadyImageTexture; if (typeof o != "function")
        return null; var s = o(n); if (s && typeof s.then == "function") {
        var c = s.then(function (a) { return (r.set(n, a), a); }).catch(function (a) { throw r.delete(n), a; });
        return r.set(n, c), c;
    } return r.set(n, s), s; }
    function ho(t) { var s; var e = globalThis, n = String(t != null ? t : "").trim(); if (!n)
        return null; var r = e.__trueosPeekReadyImageTexture; if (typeof r != "function")
        return null; var i = r(n); return !i || typeof i.then == "function" ? null : Math.max(0, Number((s = i == null ? void 0 : i.texId) != null ? s : 0) | 0) > 0 ? i : null; }
    function mo(t, e) { var n = globalThis, r = String(t != null ? t : "").trim(); if (!r || !e)
        return; var i = n.__TRUEOS_READY_IMAGE_TEXTURE_CACHE__; (!i || typeof i.get != "function" || typeof i.set != "function") && (i = new Map, n.__TRUEOS_READY_IMAGE_TEXTURE_CACHE__ = i), i.set(r, e); }
    function Xr(t, e) { var r, i, o, s, c; var n = Math.max(0, Number((r = e == null ? void 0 : e.texId) != null ? r : 0) | 0); if (n <= 0)
        throw new Error("svg texture missing"); t.state = "ready", t.texId = n, t.width = Math.max(0, Number((o = (i = e == null ? void 0 : e.pixelWidth) != null ? i : e == null ? void 0 : e.width) != null ? o : 0) | 0), t.height = Math.max(0, Number((c = (s = e == null ? void 0 : e.pixelHeight) != null ? s : e == null ? void 0 : e.height) != null ? c : 0) | 0); }
    function fo(t, e) { var n = ho(t) || Br(t); return !n || typeof n.then == "function" ? !1 : (Xr(e, n), mo(t, n), !0); }
    function po(t) { return Ur(Ur(String(t), "tspan"), "text"); }
    function Ur(t, e) { var n = "", r = 0, i = t.toLowerCase(), o = "<" + e, s = "</" + e; for (; r < t.length;) {
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
    function Yr(t) { var e = String(t), r = e.toLowerCase().indexOf("viewbox"); if (r < 0)
        return null; var i = e.indexOf("=", r + 7); if (i < 0)
        return null; var o = i + 1; for (; o < e.length;) {
        var y = e.charCodeAt(o);
        if (y !== 32 && y !== 9 && y !== 10 && y !== 13 && y !== 12)
            break;
        o += 1;
    } var s = e.charAt(o); if (s !== '"' && s !== "'")
        return null; var c = e.indexOf(s, o + 1); if (c < 0)
        return null; var a = go(e.slice(o + 1, c)); if (a.length < 4)
        return null; var h = Number(a[0]), m = Number(a[1]), d = Number(a[2]), b = Number(a[3]); return ![h, m, d, b].every(function (y) { return Number.isFinite(y); }) || d <= 0 || b <= 0 ? null : { minX: h, minY: m, w: d, h: b }; }
    function go(t) { var e = [], n = ""; for (var r = 0; r < t.length; r += 1) {
        var i = t.charCodeAt(r);
        i === 32 || i === 9 || i === 10 || i === 13 || i === 12 ? n.length > 0 && (e.push(n), n = "") : n += t.charAt(r);
    } return n.length > 0 && e.push(n), e; }
    function bo(t, e) { var n = String(t != null ? t : ""); if (!n.trim())
        return null; var r = $r.get(n), i = "data:image/svg+xml;charset=utf-8,".concat(encodeURIComponent(n)); if (r) {
        if (Ke() && r.state === "loading")
            try {
                fo(i, r);
            }
            catch (a) {
                r.state = "error";
            }
        return r;
    } if (Ke())
        return null; var o = { state: "loading", texId: 0, width: 0, height: 0 }; $r.set(n, o); var s = function (a) { Xr(o, a), Ke() || e == null || e(); }, c = function () { o.state = "error", Ke() || e == null || e(); }; try {
        var a = Br(i);
        if (!a)
            return o;
        if (a && typeof a.then == "function") {
            if (Ke())
                return o;
            a.then(s).catch(c);
        }
        else
            s(a);
    }
    catch (a) {
        c();
    } return o; }
    function _o(t, e, n) { var r = Math.max(0, e), i = Math.max(0, n), o = Yr(t); if (!o || r <= 0 || i <= 0)
        return { x: 0, y: 0, w: r, h: i }; var s = r / o.w, c = i / o.h, a = Math.min(s, c), h = Math.max(0, o.w * a), m = Math.max(0, o.h * a); return { x: Math.max(0, (r - h) / 2), y: Math.max(0, (i - m) / 2), w: h, h: m }; }
    function Kr(t, e, n) { var h, m, d, b; t.setPadding(n.EDGE_LEFT, 0), t.setPadding(n.EDGE_RIGHT, 0), t.setPadding(n.EDGE_TOP, 0), t.setPadding(n.EDGE_BOTTOM, 0); var r = Number((m = (h = e.attrs) == null ? void 0 : h.width) != null ? m : "0"), i = Number((b = (d = e.attrs) == null ? void 0 : d.height) != null ? b : "0"), o = Number.isFinite(r) && r > 0, s = Number.isFinite(i) && i > 0, c = o ? r : 300, a = s ? i : 150; (o || s) && (t.setAlignSelf(n.ALIGN_FLEX_START), t.setFlexGrow(0), t.setFlexShrink(0)), t.setWidth(c), t.setHeight(a), t.setMinWidth(Math.min(120, c)), t.setMinHeight(Math.min(80, a)); }
    function zr(t) { var e = t.svgMarkup, n = t.container, r = t.w, i = t.h, o = t.requestRerender, s = po(e), c = $t(n, "__svg"), a = c.__svgString, h = c.__w, m = c.__h, d = a !== s, b = bo(s, o); if (c.scale.set(1), c.position.set(0, 0), (b == null ? void 0 : b.state) === "ready" && b.texId > 0 && typeof c.image == "function") {
        if (d || h !== r || m !== i || c.__texId !== b.texId) {
            var w = _o(s, r, i);
            Gt(c), c.image(b.texId, w.x, w.y, w.w, w.h), c.__svgString = s, c.__w = r, c.__h = i, c.__texId = b.texId;
        }
        return;
    } Gt(c); return; if (typeof y == "function") {
        if (d || h !== r || m !== i) {
            Gt(c);
            var u = void 0;
            try {
                u = y.call(c, s);
            }
            catch (_) {
                u = null;
            }
            u && typeof u.then == "function" && u.then(function () { return o == null ? void 0 : o(); }).catch(function () { }), c.__svgString = s, c.__w = r, c.__h = i;
        }
        var w = Yr(s);
        if (w) {
            var u = r / w.w, _ = i / w.h, p = Math.min(u, _), R = w.w * p, A = w.h * p;
            c.scale.set(p), c.position.set(-w.minX * p + (r - R) / 2, -w.minY * p + (i - A) / 2);
        }
        return;
    } }
    function jr(t, e, n) { var h, m, d, b; t.setPadding(n.EDGE_LEFT, 0), t.setPadding(n.EDGE_RIGHT, 0), t.setPadding(n.EDGE_TOP, 0), t.setPadding(n.EDGE_BOTTOM, 0); var r = Number((m = (h = e.attrs) == null ? void 0 : h.width) != null ? m : "0"), i = Number((b = (d = e.attrs) == null ? void 0 : d.height) != null ? b : "0"), o = Number.isFinite(r) && r > 0, s = Number.isFinite(i) && i > 0, c = o ? r : 300, a = s ? i : 150; (o || s) && (t.setAlignSelf(n.ALIGN_FLEX_START), t.setFlexGrow(0), t.setFlexShrink(0)), t.setWidth(c), t.setHeight(a), t.setMinWidth(Math.min(120, c)), t.setMinHeight(Math.min(80, a)); }
    function Vr(t) { var e = t.graphics, n = t.container, r = t.w, i = t.h, o = t.theme, s = 1, c = s / 2; e.rect(c, c, Math.max(0, r - s), Math.max(0, i - s)), e.fill(16777215), e.stroke({ width: s, color: o.control.border, alignment: 0 }), e.moveTo(6, i - 6), e.lineTo(r - 6, 6), e.stroke({ width: 1, color: 0, alpha: .1 }); var a = vt(n, "__label", function (h) { h.style = { fontFamily: o.fontFamily, fontSize: Math.max(10, Math.floor(o.fontSize * .85)), fill: o.mutedText, fontWeight: "400", wordWrap: !1 }; }); a.text = "canvas", a.position.set(8, 8 + Pt); }
    function Jr(t, e, n) { var m, d, b, y, w, u; var r = String((d = (m = e.attrs) == null ? void 0 : m["data-root"]) != null ? d : "") === "1"; if (t.setFlexDirection(n.FLEX_DIRECTION_COLUMN), t.setAlignItems(n.ALIGN_STRETCH), r) {
        t.setPadding(n.EDGE_LEFT, 0), t.setPadding(n.EDGE_RIGHT, 0), t.setPadding(n.EDGE_TOP, 0), t.setPadding(n.EDGE_BOTTOM, 0), t.setAlignSelf(n.ALIGN_STRETCH), t.setFlexGrow(1), t.setFlexShrink(1), t.setMinWidth(0), t.setMinHeight(0);
        return;
    } t.setPadding(n.EDGE_LEFT, 8), t.setPadding(n.EDGE_RIGHT, 8), t.setPadding(n.EDGE_BOTTOM, 8), t.setPadding(n.EDGE_TOP, 34); var i = Number((y = (b = e.attrs) == null ? void 0 : b.width) != null ? y : "0"), o = Number((u = (w = e.attrs) == null ? void 0 : w.height) != null ? u : "0"), s = Number.isFinite(i) && i > 0, c = Number.isFinite(o) && o > 0, a = s ? i : 420, h = c ? o : 240; (s || c) && (t.setAlignSelf(n.ALIGN_FLEX_START), t.setFlexGrow(0), t.setFlexShrink(0)), t.setWidth(a), t.setHeight(h), t.setMinWidth(Math.min(200, a)), t.setMinHeight(Math.min(160, h)); }
    function Qr(t) { var y, w, u, _; var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.theme; if (String((w = (y = e.attrs) == null ? void 0 : y["data-root"]) != null ? w : "") === "1")
        return; var a = 1, h = a / 2; r.rect(h, h, Math.max(0, i - a), Math.max(0, o - a)), r.fill(16777215), r.stroke({ width: a, color: s.control.border, alignment: 0 }), r.rect(h, h, Math.max(0, i - a), 26), r.fill({ color: 0, alpha: .04 }); var d = String((_ = (u = e.attrs) == null ? void 0 : u.srcdoc) != null ? _ : "").trim().length > 0 ? "srcdoc" : "empty", b = vt(n, "__title", function (p) { p.style = { fontFamily: s.fontFamily, fontSize: Math.max(10, Math.floor(s.fontSize * .85)), fill: s.mutedText, fontWeight: "400", wordWrap: !1 }; }); b.text = "iframe (".concat(d, ")"), b.position.set(8, 6 + Pt), n.eventMode = "static", n.cursor = "default", n.hitArea = new Ot(0, 0, Math.max(0, i), Math.max(0, o)); }
    function Zr(t, e, n) { var i, o; var r = ((o = (i = e.attrs) == null ? void 0 : i.type) != null ? o : "text").toLowerCase(); r === "checkbox" || r === "radio" ? (t.setPadding(n.EDGE_LEFT, 0), t.setPadding(n.EDGE_RIGHT, 0), t.setPadding(n.EDGE_TOP, 0), t.setPadding(n.EDGE_BOTTOM, 0), t.setWidth(16), t.setHeight(16), t.setMinWidth(16), t.setMargin(n.EDGE_RIGHT, 6)) : (t.setPadding(n.EDGE_TOP, 6), t.setPadding(n.EDGE_BOTTOM, 6), t.setHeight(36), t.setMinHeight(36), t.setMinWidth(220)); }
    function qr(t) {
        var e_6, _a, e_7, _b;
        var X, j, f, G, W, tt, it, st, z, nt;
        var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.absX, c = t.absY, a = t.theme, h = t.textMeasure, m = t.uiState, d = t.getOrInitInputState, b = t.clamp, y = t.radioGroups, w = t.textDrags, u = t.requestPaint, _ = ((j = (X = e.attrs) == null ? void 0 : X.type) != null ? j : "text").toLowerCase(), p = e.key, R = p ? d(p, e.attrs) : void 0, A = (f = t.showCaret) != null ? f : !1, N = (G = t.caretPointerId) != null ? G : null, D = t.focusColor, x = t.getCursorColor;
        n.sortableChildren = !0;
        try {
            for (var _c = __values(n.children), _d = _c.next(); !_d.done; _d = _c.next()) {
                var L = _d.value;
                var V = L.label;
                V && (V.startsWith("__sel:") || V === "__caret") && (L.visible = !1);
            }
        }
        catch (e_6_1) { e_6 = { error: e_6_1 }; }
        finally {
            try {
                if (_d && !_d.done && (_a = _c.return)) _a.call(_c);
            }
            finally { if (e_6) throw e_6.error; }
        }
        var C = 8, H = 6 + Pt, k = 5, S = a.fontSize * 1.25;
        if (_ === "checkbox")
            r.rect(.5, .5, Math.max(0, i - 1), Math.max(0, o - 1)), r.fill(a.control.background), r.stroke({ width: 1, color: a.control.border }), R != null && R.indeterminate ? (r.moveTo(4, 4), r.lineTo(Math.max(4, i - 4), Math.max(4, o - 4)), r.stroke({ width: 2, color: a.control.accent }), r.moveTo(Math.max(4, i - 4), 4), r.lineTo(4, Math.max(4, o - 4)), r.stroke({ width: 2, color: a.control.accent })) : R != null && R.checked && (r.rect(3, 3, Math.max(0, i - 3 * 2), Math.max(0, o - 3 * 2)), r.fill(a.control.accent));
        else if (_ === "radio") {
            {
                var K = Math.max(0, Math.min(i, o) / 2 - .5);
                r.circle(i / 2, o / 2, K), r.fill(a.control.background), r.stroke({ width: 1, color: a.control.border });
            }
            if (R != null && R.checked) {
                var L = Math.max(0, Math.min(i, o) / 2 - 4.5);
                r.circle(i / 2, o / 2, L), r.fill(a.control.accent);
            }
        }
        else {
            var L = D != null ? 2 : 1, V = L / 2;
            a.control.radius > 0 ? r.roundRect(V, V, Math.max(0, i - L), Math.max(0, o - L), a.control.radius) : r.rect(V, V, Math.max(0, i - L), Math.max(0, o - L)), r.fill(a.control.background), r.stroke({ width: L, color: D != null ? D : a.control.border });
            var K = _ === "password" ? "\u2022".repeat(((W = R == null ? void 0 : R.value) != null ? W : "").length) : (tt = R == null ? void 0 : R.value) != null ? tt : "", U = Math.max(0, i - C * 2);
            p && m.fieldBounds.set(p, { x: s, y: c, w: i, h: o, innerLeft: C, innerTop: H, innerWidth: U, maxLines: k, isPassword: _ === "password" });
            var gt = we(K, U, h), F = Te(gt, k), $ = F.length > 0 ? F[F.length - 1].end : 0;
            if (p && R && typeof R.value == "string") {
                var ot = R.selections;
                if (ot && ot.size > 0)
                    try {
                        for (var _f = __values(ot.entries()), _g = _f.next(); !_g.done; _g = _f.next()) {
                            var _h = __read(_g.value, 2), bt = _h[0], g = _h[1];
                            var T = b((it = g.start) != null ? it : 0, 0, K.length), v = b((st = g.end) != null ? st : T, 0, K.length), M = b(Math.min(T, v), 0, $), E = b(Math.max(T, v), 0, $);
                            if (M === E)
                                continue;
                            var P = $t(n, "__sel:".concat(bt));
                            Gt(P), P.zIndex = 0, P.visible = !0;
                            for (var I = 0; I < F.length; I++) {
                                var O = F[I], J = Math.max(M, O.start), Q = Math.min(E, O.end);
                                if (J >= Q)
                                    continue;
                                var Y = C + h(K.slice(O.start, J)), lt = h(K.slice(J, Q));
                                P.rect(Y, H + I * S, lt, S);
                            }
                            P.fill({ color: x(bt), alpha: .22 });
                        }
                    }
                    catch (e_7_1) { e_7 = { error: e_7_1 }; }
                    finally {
                        try {
                            if (_g && !_g.done && (_b = _f.return)) _b.call(_f);
                        }
                        finally { if (e_7) throw e_7.error; }
                    }
                if (A && N != null) {
                    var bt = (z = R.selections) == null ? void 0 : z.get(N), g = bt ? bt.end : 0, T = b(g, 0, $), v = Math.max(0, F.length - 1);
                    for (var I = 0; I < F.length; I++) {
                        var O = F[I];
                        if (T >= O.start && T <= O.end) {
                            v = I;
                            break;
                        }
                    }
                    var M = (nt = F[v]) != null ? nt : { start: 0, end: 0, text: "" }, E = C + h(K.slice(M.start, T)), P = $t(n, "__caret");
                    Gt(P), P.zIndex = 2, P.visible = !0, P.moveTo(E, H + v * S), P.lineTo(E, H + v * S + S), P.stroke({ width: 1, color: D != null ? D : a.control.focusBorder });
                }
            }
            var ft = F.map(function (ot) { return ot.text; }).join("\n"), q = vt(n, "__valueText", function (ot) { ot.style = { fontFamily: a.fontFamily, fontSize: a.fontSize, fill: a.text, fontWeight: "400", wordWrap: !1 }, ot.zIndex = 1; });
            q.text = ft, q.position.set(C, H);
        }
        p && (Jt(n), n.eventMode = "static", n.cursor = "text", n.on("pointerdown", function (L) {
            var e_8, _a, e_9, _b, e_10, _c;
            var K, U, gt, F, $, ft, q, ot, bt, g, T, v, M;
            if ((L == null ? void 0 : L.button) === 2)
                return;
            var V = t.getPointerId ? t.getPointerId(L) : Number((gt = (U = L == null ? void 0 : L.pointerId) != null ? U : (K = L == null ? void 0 : L.data) == null ? void 0 : K.pointerId) != null ? gt : 0);
            if (!(V <= 0)) {
                if (m.focusedKeyByPointer.set(V, p), m.keyboardOwnerPointerId = V, _ === "checkbox") {
                    var E = d(p, e.attrs), P = E.indeterminate === !0, I = E.checked === !0;
                    !I && !P ? (E.checked = !0, E.indeterminate = !1) : I && !P ? (E.checked = !1, E.indeterminate = !0) : (E.checked = !1, E.indeterminate = !1);
                }
                else if (_ === "radio") {
                    var P = "radio:".concat(($ = (F = e.attrs) == null ? void 0 : F.name) != null ? $ : "__default__"), I = (ft = y.get(P)) != null ? ft : [];
                    try {
                        for (var I_1 = __values(I), I_1_1 = I_1.next(); !I_1_1.done; I_1_1 = I_1.next()) {
                            var O = I_1_1.value;
                            var J = d(O, void 0);
                            J.checked = O === p;
                        }
                    }
                    catch (e_8_1) { e_8 = { error: e_8_1 }; }
                    finally {
                        try {
                            if (I_1_1 && !I_1_1.done && (_a = I_1.return)) _a.call(I_1);
                        }
                        finally { if (e_8) throw e_8.error; }
                    }
                }
                else {
                    var E = d(p, e.attrs);
                    if (typeof E.value == "string") {
                        try {
                            for (var _d = __values(m.inputs.entries()), _f = _d.next(); !_f.done; _f = _d.next()) {
                                var _g = __read(_f.value, 2), Z = _g[0], _t = _g[1];
                                Z !== p && ((q = _t.selections) == null || q.delete(V));
                            }
                        }
                        catch (e_9_1) { e_9 = { error: e_9_1 }; }
                        finally {
                            try {
                                if (_f && !_f.done && (_b = _d.return)) _b.call(_d);
                            }
                            finally { if (e_9) throw e_9.error; }
                        }
                        var P = _ === "password" ? "\u2022".repeat(E.value.length) : E.value, I = m.fieldBounds.get(p), O = (ot = I == null ? void 0 : I.innerWidth) != null ? ot : Math.max(0, i - C * 2), J = Te(we(P, O, h), k), Q = ((g = (bt = L.global) == null ? void 0 : bt.x) != null ? g : 0) - s - C, Y = ((v = (T = L.global) == null ? void 0 : T.y) != null ? v : 0) - c - H, lt = Ae({ fullText: P, lines: J, localX: Q, localY: Y, lineHeight: S, measure: h });
                        E.selections || (E.selections = new Map), E.selections.set(V, { start: lt, end: lt });
                        try {
                            for (var _h = __values(w.entries()), _j = _h.next(); !_j.done; _j = _h.next()) {
                                var _k = __read(_j.value, 2), Z = _k[0], _t = _k[1];
                                _t.key === p && Z !== V && w.delete(Z);
                            }
                        }
                        catch (e_10_1) { e_10 = { error: e_10_1 }; }
                        finally {
                            try {
                                if (_j && !_j.done && (_c = _h.return)) _c.call(_h);
                            }
                            finally { if (e_10) throw e_10.error; }
                        }
                        w.set(V, { key: p, anchor: lt });
                    }
                }
                (_ === "checkbox" || _ === "radio") && ((M = L.stopPropagation) == null || M.call(L)), u == null || u();
            }
        }), (_ === "checkbox" || _ === "radio") && (n.cursor = "pointer"), n.hitArea = new Ot(0, 0, Math.max(0, i), Math.max(0, o)));
    }
    function ti(t, e) { t.setPadding(e.EDGE_TOP, 6), t.setPadding(e.EDGE_BOTTOM, 6), t.setHeight(108), t.setMinHeight(108), t.setMinWidth(220); }
    function ei(t) {
        var e_11, _a, e_12, _b;
        var st, z, nt, L, V, K, U, gt;
        var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.absX, c = t.absY, a = t.theme, h = t.textMeasure, m = t.uiState, d = t.getOrInitInputState, b = t.clamp, y = t.textDrags, w = t.requestPaint, u = e.key, _ = u ? d(u, Se(ae({}, (st = e.attrs) != null ? st : {}), { type: "text" })) : void 0, p = (z = t.showCaret) != null ? z : !1, R = (nt = t.caretPointerId) != null ? nt : null, A = t.focusColor, N = t.getCursorColor;
        n.sortableChildren = !0;
        try {
            for (var _c = __values(n.children), _d = _c.next(); !_d.done; _d = _c.next()) {
                var F = _d.value;
                var $ = F.label;
                $ && ($.startsWith("__sel:") || $ === "__caret") && (F.visible = !1);
            }
        }
        catch (e_11_1) { e_11 = { error: e_11_1 }; }
        finally {
            try {
                if (_d && !_d.done && (_a = _c.return)) _a.call(_c);
            }
            finally { if (e_11) throw e_11.error; }
        }
        var D = 8, x = 6 + Pt, C = 5, H = a.fontSize * 1.25, k = A != null ? 2 : 1, S = k / 2;
        a.control.radius > 0 ? r.roundRect(S, S, Math.max(0, i - k), Math.max(0, o - k), a.control.radius) : r.rect(S, S, Math.max(0, i - k), Math.max(0, o - k)), r.fill(a.control.background), r.stroke({ width: k, color: A != null ? A : a.control.border });
        var X = (L = _ == null ? void 0 : _.value) != null ? L : "", j = Math.max(0, i - D * 2);
        u && m.fieldBounds.set(u, { x: s, y: c, w: i, h: o, innerLeft: D, innerTop: x, innerWidth: j, maxLines: C, isPassword: !1 });
        var f = we(X, j, h), G = Te(f, C), W = G.length > 0 ? G[G.length - 1].end : 0;
        if (u && _ && typeof _.value == "string") {
            var F = _.selections;
            if (F && F.size > 0)
                try {
                    for (var _f = __values(F.entries()), _g = _f.next(); !_g.done; _g = _f.next()) {
                        var _h = __read(_g.value, 2), $ = _h[0], ft = _h[1];
                        var q = b((V = ft.start) != null ? V : 0, 0, X.length), ot = b((K = ft.end) != null ? K : q, 0, X.length), bt = b(Math.min(q, ot), 0, W), g = b(Math.max(q, ot), 0, W);
                        if (bt === g)
                            continue;
                        var T = $t(n, "__sel:".concat($));
                        Gt(T), T.zIndex = 0, T.visible = !0;
                        for (var v = 0; v < G.length; v++) {
                            var M = G[v], E = Math.max(bt, M.start), P = Math.min(g, M.end);
                            if (E >= P)
                                continue;
                            var I = D + h(X.slice(M.start, E)), O = h(X.slice(E, P));
                            T.rect(I, x + v * H, O, H);
                        }
                        T.fill({ color: N($), alpha: .22 });
                    }
                }
                catch (e_12_1) { e_12 = { error: e_12_1 }; }
                finally {
                    try {
                        if (_g && !_g.done && (_b = _f.return)) _b.call(_f);
                    }
                    finally { if (e_12) throw e_12.error; }
                }
            if (p && R != null) {
                var $ = (U = _.selections) == null ? void 0 : U.get(R), ft = $ ? $.end : 0, q = b(ft, 0, W), ot = Math.max(0, G.length - 1);
                for (var v = 0; v < G.length; v++) {
                    var M = G[v];
                    if (q >= M.start && q <= M.end) {
                        ot = v;
                        break;
                    }
                }
                var bt = (gt = G[ot]) != null ? gt : { start: 0, end: 0, text: "" }, g = D + h(X.slice(bt.start, q)), T = $t(n, "__caret");
                Gt(T), T.zIndex = 2, T.visible = !0, T.moveTo(g, x + ot * H), T.lineTo(g, x + ot * H + H), T.stroke({ width: 1, color: A != null ? A : a.control.focusBorder });
            }
        }
        var tt = G.map(function (F) { return F.text; }).join("\n"), it = vt(n, "__valueText", function (F) { F.style = { fontFamily: a.fontFamily, fontSize: a.fontSize, fill: a.text, fontWeight: "400", wordWrap: !1 }, F.zIndex = 1; });
        it.text = tt, it.position.set(D, x), u && (Jt(n), n.eventMode = "static", n.cursor = "text", n.hitArea = new Ot(0, 0, Math.max(0, i), Math.max(0, o)), n.on("pointerdown", function (F) {
            var e_13, _a, e_14, _b;
            var q, ot, bt, g, T, v, M, E, P, I;
            if ((F == null ? void 0 : F.button) === 2)
                return;
            var $ = t.getPointerId ? t.getPointerId(F) : Number((bt = (ot = F == null ? void 0 : F.pointerId) != null ? ot : (q = F == null ? void 0 : F.data) == null ? void 0 : q.pointerId) != null ? bt : 0);
            if ($ <= 0)
                return;
            m.focusedKeyByPointer.set($, u), m.keyboardOwnerPointerId = $;
            var ft = d(u, Se(ae({}, (g = e.attrs) != null ? g : {}), { type: "text" }));
            if (typeof ft.value == "string") {
                try {
                    for (var _c = __values(m.inputs.entries()), _d = _c.next(); !_d.done; _d = _c.next()) {
                        var _f = __read(_d.value, 2), Mt = _f[0], xt = _f[1];
                        Mt !== u && ((T = xt.selections) == null || T.delete($));
                    }
                }
                catch (e_13_1) { e_13 = { error: e_13_1 }; }
                finally {
                    try {
                        if (_d && !_d.done && (_a = _c.return)) _a.call(_c);
                    }
                    finally { if (e_13) throw e_13.error; }
                }
                var O = m.fieldBounds.get(u), J = (v = O == null ? void 0 : O.innerWidth) != null ? v : Math.max(0, i - D * 2), Q = ft.value, Y = Te(we(Q, J, h), C), lt = ((E = (M = F.global) == null ? void 0 : M.x) != null ? E : 0) - s - D, Z = ((I = (P = F.global) == null ? void 0 : P.y) != null ? I : 0) - c - x, _t = Ae({ fullText: Q, lines: Y, localX: lt, localY: Z, lineHeight: H, measure: h });
                ft.selections || (ft.selections = new Map), ft.selections.set($, { start: _t, end: _t });
                try {
                    for (var _g = __values(y.entries()), _h = _g.next(); !_h.done; _h = _g.next()) {
                        var _j = __read(_h.value, 2), Mt = _j[0], xt = _j[1];
                        xt.key === u && Mt !== $ && y.delete(Mt);
                    }
                }
                catch (e_14_1) { e_14 = { error: e_14_1 }; }
                finally {
                    try {
                        if (_h && !_h.done && (_b = _g.return)) _b.call(_g);
                    }
                    finally { if (e_14) throw e_14.error; }
                }
                y.set($, { key: u, anchor: _t });
            }
            w == null || w();
        }));
    }
    function ni(t, e) { t.setFlexDirection(e.FLEX_DIRECTION_ROW), t.setAlignItems(e.ALIGN_CENTER), t.setJustifyContent(e.JUSTIFY_FLEX_START), t.setPadding(e.EDGE_LEFT, 8), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0); }
    function yo(t, e, n, r, i) { t.circle(e, n, r), t.stroke({ width: 2, color: i }); var o = e + r * .65, s = n + r * .65, c = e + r * 1.55, a = n + r * 1.55; t.moveTo(o, s), t.lineTo(c, a), t.stroke({ width: 2, color: i }); }
    function ri(t, e) { t.setFlexDirection(e.FLEX_DIRECTION_ROW), t.setFlexWrap(e.WRAP_NO_WRAP), t.setAlignItems(e.ALIGN_CENTER), t.setJustifyContent(e.JUSTIFY_FLEX_START), t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0); }
    function ii(t, e) { t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0), t.setWidth(36), t.setHeight(36), t.setMinWidth(36), t.setMinHeight(36), t.setFlexGrow(0), t.setFlexShrink(0), t.setMargin(e.EDGE_RIGHT, 6); }
    function oi(t) { var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.theme, c = t.uiState, a = t.getPointerId, h = t.focusInputKey, m = t.requestPaint, d = function (y) { r.clear(); var w = 1, u = w / 2; s.control.button.radius > 0 ? r.roundRect(u, u, Math.max(0, i - w), Math.max(0, o - w), s.control.button.radius) : r.rect(u, u, Math.max(0, i - w), Math.max(0, o - w)), r.fill(y), r.stroke({ width: w, color: s.control.button.border }); var _ = i / 2 - 2, p = o / 2 - 2, R = Math.max(5, Math.min(7, Math.min(i, o) * .22)); yo(r, _, p, R, s.text); }; d(s.control.button.fill), Jt(n), n.eventMode = "static", n.cursor = "pointer", n.hitArea = new Ot(0, 0, Math.max(0, i), Math.max(0, o)), n.on("pointerover", function () { return d(s.control.button.hoverFill); }), n.on("pointerout", function () { return d(s.control.button.fill); }), n.on("pointerdown", function (y) { var w; if ((y == null ? void 0 : y.button) !== 2) {
        if (d(s.control.button.activeFill), h) {
            var u = a(y);
            u > 0 && (c.focusedKeyByPointer.set(u, h), c.keyboardOwnerPointerId = u);
        }
        m == null || m(), (w = y.stopPropagation) == null || w.call(y);
    } }), n.on("pointerup", function () { return d(s.control.button.hoverFill); }); var b = e.attrs; }
    function cn(t, e) { var n = t.get(e); if (n)
        return n; var r = { x: 0, y: 0 }; return t.set(e, r), r; }
    function si(t, e) { t.setPositionType(e.POSITION_TYPE_ABSOLUTE), t.setPosition(e.EDGE_LEFT, 0), t.setPosition(e.EDGE_TOP, 0), t.setAlignSelf(e.ALIGN_FLEX_START), t.setFlexGrow(0), t.setFlexShrink(0), t.setPadding(e.EDGE_LEFT, 12), t.setPadding(e.EDGE_RIGHT, 12), t.setPadding(e.EDGE_TOP, 12), t.setPadding(e.EDGE_BOTTOM, 12), t.setWidth(540), t.setMinWidth(360), t.setMinHeight(148); }
    function ai(t) { var N, D; var e = t.node, n = t.container, r = t.w, i = t.h, o = t.theme, s = t.selectedBy, c = t.getCursorColor, a = t.dialogStates, h = t.dialogDrags, m = t.bringToFront, d = t.requestPaint, b = e.key; if (!b)
        return; var y = s.get(b), w = y == null ? o.boxBorder : c(y), u = Math.max(0, Math.round(r)), _ = Math.max(0, Math.round(i)), p = $t(n, "__dialogBorder"); Gt(p), p.rect(0, 0, u, _), p.fill({ color: 16777215, alpha: .8 }); var R = y == null ? 1 : 2, A = R / 2; p.rect(A, A, Math.max(0, u - R), Math.max(0, _ - R)), p.stroke({ width: R, color: w, alignment: 0 }), p.eventMode = "static", p.cursor = "move", p.hitArea = new Ot(0, 0, u, _), p.on("pointerdown", function (x) {
        var e_15, _a;
        var S, X, j, f, G, W, tt, it;
        var C = function (st) { try {
            typeof console != "undefined" && typeof console.log == "function" && console.log("[dialog pointerdown] ".concat(st));
        }
        catch (z) { } };
        if (C("start"), (x == null ? void 0 : x.button) === 2)
            return;
        C("pointer-id");
        var H = t.getPointerId ? t.getPointerId(x) : Number((j = (X = x == null ? void 0 : x.pointerId) != null ? X : (S = x == null ? void 0 : x.data) == null ? void 0 : S.pointerId) != null ? j : 0);
        if (H <= 0 || H <= 0)
            return;
        C("clear-other-drags");
        try {
            for (var _b = __values(h.entries()), _c = _b.next(); !_c.done; _c = _b.next()) {
                var _d = __read(_c.value, 2), st = _d[0], z = _d[1];
                z.key === b && st !== H && h.delete(st);
            }
        }
        catch (e_15_1) { e_15 = { error: e_15_1 }; }
        finally {
            try {
                if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
            }
            finally { if (e_15) throw e_15.error; }
        }
        C("select"), s.set(b, H), C("bring-to-front"), m == null || m(b), C("state");
        var k = cn(a, b);
        C("set-drag"), h.set(H, { key: b, startGX: (G = (f = x.global) == null ? void 0 : f.x) != null ? G : 0, startGY: (tt = (W = x.global) == null ? void 0 : W.y) != null ? tt : 0, originX: k.x, originY: k.y }), C("request-paint"), d == null || d(), C("stop-propagation"), (it = x.stopPropagation) == null || it.call(x), C("done");
    }); {
        var x = n.getChildByLabel, C = (D = (N = x == null ? void 0 : x.call(n, "__children")) != null ? N : n.children.find(function (H) { return H && H.label === "__children"; })) != null ? D : null;
        if (C && p.parent === n) {
            var H = n.getChildIndex(C), k = Math.max(0, n.children.length - 1), S = Math.max(0, Math.min(H - 1, k));
            n.getChildIndex(p) > S && n.setChildIndex(p, S);
        }
    } }
    function Gn(t, e, n) { var c; var r = t.get(e); if (r)
        return r; var i = Number((c = n == null ? void 0 : n.value) != null ? c : "0"), s = { value: Number.isFinite(i) ? i : 0 }; return t.set(e, s), s; }
    function li(t, e) { t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0), t.setHeight(36), t.setMinHeight(36), t.setMinWidth(140), t.setFlexGrow(0), t.setFlexShrink(0); }
    function xo(t, e, n) { return Math.max(e, Math.min(n, t)); }
    function vn(t, e, n) { var i; var r = Number((i = t == null ? void 0 : t[e]) != null ? i : ""); return Number.isFinite(r) ? r : n; }
    function wo(t, e, n, r, i, o) { var c = e + 3, a = e + r - 3, h = n + 3, m = n + i - 3; t.moveTo(c, m), t.lineTo((c + a) / 2, h), t.lineTo(a, m), t.stroke({ width: 2, color: o }); }
    function To(t, e, n, r, i, o) { var c = e + 3, a = e + r - 3, h = n + 3, m = n + i - 3; t.moveTo(c, h), t.lineTo((c + a) / 2, m), t.lineTo(a, h), t.stroke({ width: 2, color: o }); }
    function ci(t) { var j; var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.theme, c = t.getValue, a = t.setValue, h = t.requestPaint, m = e.key, d = e.attrs, b = vn(d, "min", 0), y = vn(d, "max", 255), w = Math.max(1e-9, vn(d, "step", 1)), u = c(), _ = 1, p = _ / 2; r.rect(p, p, Math.max(0, i - _), Math.max(0, o - _)), r.fill(s.control.background), r.stroke({ width: _, color: s.control.border }); var R = 22, A = Math.max(0, i - R); r.moveTo(A + .5, 0), r.lineTo(A + .5, o), r.stroke({ width: 1, color: s.control.border, alignment: 0 }); var N = $t(n, "__arrows"); Gt(N), wo(N, A, 0, R, o / 2, s.text), To(N, A, o / 2, R, o / 2, s.text); var D = ((j = d == null ? void 0 : d.channel) != null ? j : "").toLowerCase(), x = D === "r" ? "R" : D === "g" ? "G" : D === "b" ? "B" : D === "a" ? "A" : "", C = vt(n, "__valueText", function (f) { f.style = { fontFamily: s.fontFamily, fontSize: s.fontSize, fill: s.text, fontWeight: "400", wordWrap: !1 }; }); if (C.text = x ? "".concat(x, ": ").concat(Math.round(u)) : String(Math.round(u)), C.position.set(8, 9 + Pt), !m)
        return; var H = new Ot(A, 0, R, o / 2), k = new Ot(A, o / 2, R, o / 2), S = function (f) { var G = c(), W = xo(G + f * w, b, y); a(W), h == null || h(); }, X = $t(n, "__hit"); Gt(X), X.eventMode = "static", X.cursor = "default", X.hitArea = new Ot(0, 0, Math.max(0, i), Math.max(0, o)), X.on("pointerdown", function (f) {
        var e_16, _a;
        var nt, L, V, K, U, gt;
        if ((f == null ? void 0 : f.button) === 2)
            return;
        var G = t.getPointerId ? t.getPointerId(f) : Number((V = (L = f == null ? void 0 : f.pointerId) != null ? L : (nt = f == null ? void 0 : f.data) == null ? void 0 : nt.pointerId) != null ? V : 0);
        if (G <= 0)
            return;
        var W = n.toLocal(f.global), tt = (K = W == null ? void 0 : W.x) != null ? K : 0, it = (U = W == null ? void 0 : W.y) != null ? U : 0, st = H.contains(tt, it) ? 1 : k.contains(tt, it) ? -1 : null;
        if (!st)
            return;
        S(st);
        var z = t.numberHolds;
        if (z && m) {
            try {
                for (var _b = __values(z.entries()), _c = _b.next(); !_c.done; _c = _b.next()) {
                    var _d = __read(_c.value, 2), ft = _d[0], q = _d[1];
                    ft !== G && (q.timeoutId != null && window.clearTimeout(q.timeoutId), q.intervalId != null && window.clearInterval(q.intervalId), z.delete(ft));
                }
            }
            catch (e_16_1) { e_16 = { error: e_16_1 }; }
            finally {
                try {
                    if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
                }
                finally { if (e_16) throw e_16.error; }
            }
            var F = z.get(G);
            F && (F.timeoutId != null && window.clearTimeout(F.timeoutId), F.intervalId != null && window.clearInterval(F.intervalId));
            var $_1 = { key: m, timeoutId: null, intervalId: null };
            $_1.timeoutId = window.setTimeout(function () { $_1.timeoutId = null, $_1.intervalId = window.setInterval(function () { S(st); }, 250); }, 500), z.set(G, $_1);
        }
        (gt = f.stopPropagation) == null || gt.call(f);
    }); }
    var un = null;
    function ui() { return un || (un = new sn({ data: de, label: "attribute-color-picker-colors", shrinkToFit: !1, usage: An.VERTEX | An.COPY_DST }), un); }
    function di(t, e, n) { var h, m, d, b; t.setPadding(n.EDGE_LEFT, 0), t.setPadding(n.EDGE_RIGHT, 0), t.setPadding(n.EDGE_TOP, 0), t.setPadding(n.EDGE_BOTTOM, 0); var r = Number((m = (h = e.attrs) == null ? void 0 : h.width) != null ? m : "0"), i = Number((b = (d = e.attrs) == null ? void 0 : d.height) != null ? b : "0"), o = Number.isFinite(r) && r > 0, s = Number.isFinite(i) && i > 0, c = o ? r : 240, a = s ? i : 200; (o || s) && (t.setAlignSelf(n.ALIGN_FLEX_START), t.setFlexGrow(0), t.setFlexShrink(0)), t.setWidth(c), t.setHeight(a), t.setMinWidth(Math.min(240, c)), t.setMinHeight(Math.min(200, a)); }
    function fe(t) { return Number.isFinite(t) ? Math.max(0, Math.min(255, Math.round(t))) : 0; }
    function dn(t) { return fe(t).toString(16).padStart(2, "0"); }
    function Eo(t, e, n, r, i, o, s, c) { var a = s - n, h = c - r, m = i - n, d = o - r, b = t - n, y = e - r, w = a * a + h * h, u = a * m + h * d, _ = a * b + h * y, p = m * m + d * d, R = m * b + d * y, A = 1 / (w * p - u * u), N = (p * _ - u * R) * A, D = (w * R - u * _) * A; return N >= 0 && D >= 0 && N + D <= 1; }
    function Io(t, e, n, r, i, o, s, c) { var a = i - n, h = o - r, m = s - n, d = c - r, b = t - n, y = e - r, w = a * d - m * h; if (Math.abs(w) < 1e-9)
        return { w0: 1, w1: 0, w2: 0 }; var u = (b * d - m * y) / w, _ = (a * y - b * h) / w; return { w0: 1 - u - _, w1: u, w2: _ }; }
    var Mo = { name: "solid-out", fragment: { main: "\n      outColor = vec4(1.0);\n    " } }, hn = null;
    function So() { if (hn)
        return hn; var t = { name: "color-picker-vertex-color", bits: [dr, hr, ur, Mo] }; return hn = new Be({ glProgram: t, resources: {} }), hn; }
    function hi(t, e, n) { var r = new Float32Array(12), i = [-90, -30, 30, 90, 150, 210]; for (var o = 0; o < 6; o++) {
        var s = i[o] * Math.PI / 180;
        r[o * 2 + 0] = t + Math.cos(s) * n, r[o * 2 + 1] = e + Math.sin(s) * n;
    } return r; }
    var de = new Uint8Array([255, 0, 0, 255, 128, 128, 0, 255, 0, 255, 0, 255, 0, 128, 128, 255, 0, 0, 255, 255, 128, 0, 128, 255]), Le = new Uint32Array([0, 1, 2, 0, 2, 3, 0, 3, 4, 0, 4, 5]);
    function Hn(t) { var e = t.lx, n = t.ly, r = t.w, i = t.h, o = 10, s = Math.max(0, r - o * 2), c = Math.max(0, i - o * 2), a = o + s / 2, h = o + c / 2, m = Math.max(0, Math.min(s, c) / 2 - 2), d = hi(a, h, m); for (var b = 0; b < Le.length; b += 3) {
        var y = Le[b + 0], w = Le[b + 1], u = Le[b + 2], _ = d[y * 2 + 0], p = d[y * 2 + 1], R = d[w * 2 + 0], A = d[w * 2 + 1], N = d[u * 2 + 0], D = d[u * 2 + 1];
        if (!Eo(e, n, _, p, R, A, N, D))
            continue;
        var x = Io(e, n, _, p, R, A, N, D), C = y * 4, H = w * 4, k = u * 4, S = x.w0 * de[C + 0] + x.w1 * de[H + 0] + x.w2 * de[k + 0], X = x.w0 * de[C + 1] + x.w1 * de[H + 1] + x.w2 * de[k + 1], j = x.w0 * de[C + 2] + x.w1 * de[H + 2] + x.w2 * de[k + 2];
        return { r: fe(S), g: fe(X), b: fe(j) };
    } return null; }
    function mi(t) { var z, nt; var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.theme, c = t.rgb, a = t.setRgb, h = t.alpha, m = t.setAlpha, d = t.pick, b = t.setPick, y = t.requestPaint, w = t.getPointerId, u = t.setDraggingPointerId, _ = 1, p = _ / 2; r.rect(p, p, Math.max(0, i - _), Math.max(0, o - _)), r.fill(16777215), r.stroke({ width: _, color: s.control.border, alignment: 0 }); var R = 10, A = Math.max(0, i - R * 2), N = Math.max(0, o - R * 2), D = R + A / 2, x = R + N / 2, C = Math.max(0, Math.min(A, N) / 2 - 2), H = hi(D, x, C), k = "".concat(Math.round(i), "x").concat(Math.round(o)), S = n.getChildByLabel, X = S ? S.call(n, "__mesh") : n.children.find(function (L) { return (L == null ? void 0 : L.label) === "__mesh"; }); if (X) {
        if (X.__sizeKey !== k) {
            var L = new Float32Array(H.length), V = new Ce({ positions: H, uvs: L, indices: Le });
            V.addAttribute("aColor", { buffer: ui(), format: "unorm8x4", stride: 4, offset: 0 });
            try {
                (nt = (z = X.geometry) == null ? void 0 : z.destroy) == null || nt.call(z);
            }
            catch (K) { }
            X.geometry = V, X.__sizeKey = k;
        }
    }
    else {
        var L = new Float32Array(H.length), V = new Ce({ positions: H, uvs: L, indices: Le });
        V.addAttribute("aColor", { buffer: ui(), format: "unorm8x4", stride: 4, offset: 0 }), X = new on({ geometry: V, shader: So() }), X.label = "__mesh", n.addChild(X), X.__sizeKey = k;
    } X.removeAllListeners(), X.eventMode = "static", X.cursor = "crosshair", X.hitArea = new Ot(R, R, A, N), X.on("pointerdown", function (L) { var $, ft, q; if ((L == null ? void 0 : L.button) === 2)
        return; var V = w(L); if (V <= 0)
        return; var K = n.toLocal(L.global), U = ($ = K == null ? void 0 : K.x) != null ? $ : 0, gt = (ft = K == null ? void 0 : K.y) != null ? ft : 0, F = Hn({ lx: U, ly: gt, w: i, h: o }); F && (b({ x: U, y: gt }), a(F), u(V), y == null || y(), (q = L.stopPropagation) == null || q.call(L)); }); {
        var L = $t(n, "__border");
        Gt(L), L.moveTo(H[0], H[1]);
        for (var V = 1; V < 6; V++)
            L.lineTo(H[V * 2 + 0], H[V * 2 + 1]);
        L.closePath(), L.stroke({ width: 2, color: 0 });
    } var j = $t(n, "__overlay"); Gt(j); var f = 44, G = 18, W = Math.max(R, i - R - f), tt = R; j.rect(W, tt, f, G), j.fill({ color: fe(c.r) << 16 | fe(c.g) << 8 | fe(c.b), alpha: Math.max(0, Math.min(1, fe(h) / 255)) }), j.rect(W + .5, tt + .5, f - 1, G - 1), j.stroke({ width: 1, color: s.control.border, alignment: 0 }), d && (j.circle(d.x, d.y, 4), j.stroke({ width: 2, color: 16777215 }), j.circle(d.x, d.y, 4), j.stroke({ width: 1, color: 0 })); var it = "#".concat(dn(c.r)).concat(dn(c.g)).concat(dn(c.b)).concat(dn(h)).toUpperCase(), st = vt(n, "__label", function (L) { L.style = { fontFamily: s.fontFamily, fontSize: Math.max(10, Math.floor(s.fontSize * .75)), fill: s.mutedText, fontWeight: "400", wordWrap: !1 }; }); st.text = it, st.position.set(R, Math.max(R, o - R - st.height)), m && m(fe(h)); }
    function Ee(t, e, n) { var r = t.get(e); if (r)
        return r; var i = { selectedIndex: Math.max(0, n | 0), open: !1 }; return t.set(e, i), i; }
    function fi(t, e) { t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0), t.setHeight(36), t.setMinHeight(36), t.setMinWidth(220); }
    function Po(t, e, n, r, i, o) { var c = e + 4, a = e + r - 4, h = n + 4, m = n + i - 4; t.moveTo(c, (h + m) / 2 - 2), t.lineTo((c + a) / 2, (h + m) / 2 + 2), t.lineTo(a, (h + m) / 2 - 2), t.stroke({ width: 2, color: o }); }
    function Wn(t) {
        var r;
        var n = String((r = t == null ? void 0 : t["data-options"]) != null ? r : "").split("\n").map(function (i) { return i.trim(); }).filter(function (i) { return i.length > 0; });
        return n.length > 0 ? n : ["(empty)"];
    }
    function Ro(t) { var n; var e = Number((n = t == null ? void 0 : t["data-selected-index"]) != null ? n : "0"); return Number.isFinite(e) ? Math.max(0, e | 0) : 0; }
    function mn(t) { var j; var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.absX, c = t.absY, a = t.theme, h = t.selectStates, m = t.uiState, d = t.getPointerId, b = t.getCursorColor, y = t.requestPaint, w = t.requestOverlayPaint, u = t.popupSink, _ = e.key; if (!_)
        return; var p = Wn(e.attrs), R = Ro(e.attrs), A = Ee(h, _, R); A.selectedIndex = Math.max(0, Math.min(p.length - 1, A.selectedIndex | 0)); var N = (function () {
        var e_17, _a;
        var f = m.keyboardOwnerPointerId;
        if (m.focusedKeyByPointer.get(f) === _)
            return f;
        try {
            for (var _b = __values(m.focusedKeyByPointer.entries()), _c = _b.next(); !_c.done; _c = _b.next()) {
                var _d = __read(_c.value, 2), G = _d[0], W = _d[1];
                if (W === _)
                    return G;
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
    })(), D = N != null ? b(N) : null, x = D != null ? 2 : 1, C = x / 2; a.control.radius > 0 ? r.roundRect(C, C, Math.max(0, i - x), Math.max(0, o - x), a.control.radius) : r.rect(C, C, Math.max(0, i - x), Math.max(0, o - x)), r.fill(a.control.background), r.stroke({ width: x, color: D != null ? D : a.control.border }); var H = 22, k = Math.max(0, i - H); r.moveTo(k + .5, 0), r.lineTo(k + .5, o), r.stroke({ width: 1, color: a.control.border, alignment: 0 }), Po(r, k, 0, H, o, a.text); var S = (j = p[A.selectedIndex]) != null ? j : "", X = vt(n, "__label", function (f) { f.style = { fontFamily: a.fontFamily, fontSize: a.fontSize, fill: a.text, fontWeight: "400", wordWrap: !1 }; }); X.text = S, X.position.set(8, 9 + Pt), Jt(n), n.eventMode = "static", n.cursor = "pointer", n.hitArea = new Ot(0, 0, Math.max(0, i), Math.max(0, o)), n.on("pointerdown", function (f) { var W, tt; if ((f == null ? void 0 : f.button) === 2)
        return; var G = d(f); G <= 0 || (m.focusedKeyByPointer.set(G, _), m.keyboardOwnerPointerId = G, A.open = !A.open, (W = w != null ? w : y) == null || W(), (tt = f.stopPropagation) == null || tt.call(f)); }), A.open && u.push({ key: _, absX: s, absY: c, w: i, h: o, options: p, selectedIndex: A.selectedIndex }); }
    function Fn(t) { var R; var e = t.popup, n = t.stage, r = t.theme, i = t.selectStates, o = t.uiState, s = t.getPointerId, c = t.requestPaint, a = t.viewportW, h = t.viewportH, m = 30, b = Math.min(7, e.options.length), y = b * m, w = e.absX, u = e.absY + e.h; w = Math.max(0, Math.min(w, Math.max(0, a - e.w))), u + y > h - 4 && (u = e.absY - y), u = Math.max(0, Math.min(u, Math.max(0, h - y))); var _ = new Ct; _.position.set(w, u), n.addChild(_); var p = new kt; p.rect(0, 0, e.w, y), p.fill(16777215), p.rect(.5, .5, Math.max(0, e.w - 1), Math.max(0, y - 1)), p.stroke({ width: 1, color: r.control.border, alignment: 0 }), _.addChild(p), _.eventMode = "static", _.cursor = "pointer", _.hitArea = new Ot(0, 0, e.w, y), _.on("pointerdown", function (A) { var S, X, j; if ((A == null ? void 0 : A.button) === 2)
        return; var N = s(A), D = _.toLocal(A.global), x = (S = D == null ? void 0 : D.x) != null ? S : -1, C = (X = D == null ? void 0 : D.y) != null ? X : -1; if (x < 0 || x > e.w || C < 0 || C > y)
        return; var H = Math.max(0, Math.min(e.options.length - 1, Math.floor(C / m))), k = i.get(e.key); k && (k.selectedIndex = H, k.open = !1), N > 0 && (o.focusedKeyByPointer.set(N, e.key), o.keyboardOwnerPointerId = N), c == null || c(), (j = A.stopPropagation) == null || j.call(A); }); for (var A = 0; A < b; A++) {
        var N = A * m;
        if (A === e.selectedIndex) {
            var x = new kt;
            x.rect(1, N + 1, Math.max(0, e.w - 2), m - 2), x.fill({ color: 0, alpha: .06 }), _.addChild(x);
        }
        var D = ie({ text: (R = e.options[A]) != null ? R : "", fontFamily: r.fontFamily, fontSize: r.fontSize, fill: r.text, wordWrap: !1 });
        D.position.set(8, N + 7 + Pt), _.addChild(D);
    } }
    function Wt(t, e, n) { var r = Number.isFinite(t) ? t | 0 : 0; return Math.max(e, Math.min(n, r)); }
    function Zt(t) { var e = Wt(t, 0, 99); return e < 10 ? "0".concat(e) : String(e); }
    function se(t, e, n) { var r = Number(t); if (!Number.isFinite(r))
        return null; var i = Math.trunc(r); return i < e || i > n ? null : i; }
    function gn(t) { if (t.length !== 4)
        return null; for (var r = 0; r < t.length; r += 1) {
        var i = t.charCodeAt(r);
        if (i < 48 || i > 57)
            return null;
    } var e = Number(t); if (!Number.isFinite(e))
        return null; var n = e - 2e3; return n < 0 || n > 99 ? null : n; }
    function ko(t) { var e = String(t != null ? t : "").trim().split(":"); if (e.length !== 2 && e.length !== 3)
        return null; var n = se(e[0], 0, 23), r = se(e[1], 0, 59), i = e.length === 3 ? se(e[2], 0, 59) : 0; return n == null || r == null || i == null ? null : { hour: n, minute: r, second: i }; }
    function Oo(t) { var e = String(t != null ? t : "").trim().split("-"); if (e.length !== 2)
        return null; var n = gn(e[0]), r = se(e[1], 1, 12); return n == null || r == null ? null : { year2: n, month: r }; }
    function Co(t) { var e = String(t != null ? t : "").trim().split("-"); if (e.length !== 3)
        return null; var n = gn(e[0]), r = se(e[1], 1, 12), i = se(e[2], 1, 31); if (n == null || r == null || i == null)
        return null; var o = Wt(Math.floor((i - 1) / 7) + 1, 1, 4); return { year2: n, month: r, weekIndex: o }; }
    function No(t) { var e = String(t != null ? t : "").trim(), n = e.indexOf("-W"); if (n < 0)
        return null; var r = gn(e.slice(0, n)), i = se(e.slice(n + 2), 1, 48); if (r == null || i == null)
        return null; var o = Wt(Math.floor((i - 1) / 4) + 1, 1, 12), s = Wt((i - 1) % 4 + 1, 1, 4); return { year2: r, month: o, weekIndex: s }; }
    function Ao(t) { var e = String(t != null ? t : "").trim(), n = e.indexOf("T"); if (n < 0 && (n = e.indexOf(" ")), n < 0)
        return null; var r = e.slice(0, n).split("-"), i = e.slice(n + 1).split(":"); if (r.length !== 3 || i.length !== 2 && i.length !== 3)
        return null; var o = gn(r[0]), s = se(r[1], 1, 12), c = se(r[2], 1, 31), a = se(i[0], 0, 23), h = se(i[1], 0, 59), m = i.length === 3 ? se(i[2], 0, 59) : 0; if (o == null || s == null || c == null || a == null || h == null || m == null)
        return null; var d = Wt(Math.floor((c - 1) / 7) + 1, 1, 4); return { year2: o, month: s, weekIndex: d, hour: a, minute: h, second: m }; }
    function fn(t) { return "20".concat(Zt(t.year2), "-").concat(Zt(t.month)); }
    function Lo(t) { return (Wt(t.month, 1, 12) - 1) * 4 + Wt(t.weekIndex, 1, 4); }
    function pn(t) { return "20".concat(Zt(t.year2), "-W").concat(Zt(Lo(t))); }
    function De(t) { var e = (Wt(t.weekIndex, 1, 4) - 1) * 7 + 1; return "20".concat(Zt(t.year2), "-").concat(Zt(t.month), "-").concat(Zt(e)); }
    function Ve(t) { return "".concat(Zt(t.hour), ":").concat(Zt(t.minute), ":").concat(Zt(t.second)); }
    function ze(t) { return "".concat(De(t), "T").concat(Ve(t)); }
    function Do(t) { var m; var e = t.map, n = t.yearSliderOwners, r = t.inputKey, i = t.kind, o = t.attrs, s = e.get(r); if (s)
        return s.kind = i, s; var c = new Date, a = { kind: i, year2: Wt(c.getFullYear() - 2e3, 0, 99), month: Wt(c.getMonth() + 1, 1, 12), weekIndex: 1, hour: Wt(c.getHours(), 0, 23), minute: Wt(c.getMinutes(), 0, 59), second: Wt(c.getSeconds(), 0, 59), openPanel: null, openYear: !1, openMonthGrid: !1, yearSliderKey: "".concat(r, ":year-slider") }, h = String((m = o == null ? void 0 : o.value) != null ? m : ""); if (h.trim().length > 0) {
        if (i === "time") {
            var d = ko(h);
            d && (a.hour = d.hour, a.minute = d.minute, a.second = d.second);
        }
        else if (i === "month") {
            var d = Oo(h);
            d && (a.year2 = d.year2, a.month = d.month);
        }
        else if (i === "week") {
            var d = No(h);
            d && (a.year2 = d.year2, a.month = d.month, a.weekIndex = d.weekIndex);
        }
        else if (i === "date") {
            var d = Co(h);
            d && (a.year2 = d.year2, a.month = d.month, a.weekIndex = d.weekIndex);
        }
        else if (i === "datetime-local") {
            var d = Ao(h);
            d && (a.year2 = d.year2, a.month = d.month, a.weekIndex = d.weekIndex, a.hour = d.hour, a.minute = d.minute, a.second = d.second);
        }
    } return e.set(r, a), n.set(a.yearSliderKey, r), a; }
    function gi(t, e, n) { t.setPadding(e.EDGE_LEFT, 0), t.setPadding(e.EDGE_RIGHT, 0), t.setPadding(e.EDGE_TOP, 0), t.setPadding(e.EDGE_BOTTOM, 0), t.setHeight(36), t.setMinHeight(36), t.setMinWidth(n === "datetime-local" ? 340 : 220); }
    function vo(t, e, n, r, i) { var o = i != null ? 2 : 1, s = o / 2; e.control.radius > 0 ? t.roundRect(s, s, Math.max(0, n - o), Math.max(0, r - o), e.control.radius) : t.rect(s, s, Math.max(0, n - o), Math.max(0, r - o)), t.fill(e.control.background), t.stroke({ width: o, color: i != null ? i : e.control.border }); }
    function pi(t, e, n, r, i) { var o = e + r / 2, s = n + r / 2; t.moveTo(e, s - 2), t.lineTo(o, s + 2), t.lineTo(e + r, s - 2), t.stroke({ width: 2, color: i }); }
    function bi(t) { var k, S; var e = t.node, n = t.container, r = t.graphics, i = t.w, o = t.h, s = t.absX, c = t.absY, a = t.theme, h = t.uiState, m = t.getPointerId, d = t.getCursorColor, b = t.temporalStates, y = t.yearSliderOwners, w = t.getOrInitInputValue, u = t.requestPaint, _ = t.requestOverlayPaint, p = t.popupSink, R = e.key; if (!R || !e.tagName)
        return; var A = e.tagName === "timeinput" ? "time" : e.tagName === "monthinput" ? "month" : e.tagName === "weekinput" ? "week" : e.tagName === "dateinput" ? "date" : "datetime-local", N = Do({ map: b, yearSliderOwners: y, inputKey: R, kind: A, attrs: e.attrs }), D = w(R, Se(ae({}, (k = e.attrs) != null ? k : {}), { type: "text" })); A === "time" ? D.value = Ve(N) : A === "month" ? D.value = fn(N) : A === "week" ? D.value = pn(N) : A === "date" ? D.value = De(N) : D.value = ze(N); var x = (function () {
        var e_18, _a;
        var X = h.keyboardOwnerPointerId;
        if (h.focusedKeyByPointer.get(X) === R)
            return X;
        try {
            for (var _b = __values(h.focusedKeyByPointer.entries()), _c = _b.next(); !_c.done; _c = _b.next()) {
                var _d = __read(_c.value, 2), j = _d[0], f = _d[1];
                if (f === R)
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
    })(), C = x != null ? d(x) : null; vo(r, a, i, o, C); var H = 8; if (A !== "datetime-local") {
        var X = (S = D.value) != null ? S : "", j = vt(n, "__shown", function (W) { W.style = { fontFamily: a.fontFamily, fontSize: a.fontSize, fill: a.text, fontWeight: "400", wordWrap: !1 }; });
        j.text = X, j.visible = !0, j.position.set(H, 9 + Pt);
        var f = n.getChildByLabel ? n.getChildByLabel("__date") : n.children.find(function (W) { return (W == null ? void 0 : W.label) === "__date"; }), G = n.getChildByLabel ? n.getChildByLabel("__time") : n.children.find(function (W) { return (W == null ? void 0 : W.label) === "__time"; });
        f && (f.visible = !1), G && (G.visible = !1), pi(r, Math.max(0, i - 18), 11, 10, a.text);
    }
    else {
        var X = Math.max(0, Math.round(i * .52));
        r.moveTo(X + .5, 0), r.lineTo(X + .5, o), r.stroke({ width: 1, color: a.control.border, alignment: 0 });
        var j = De(N), f = Ve(N), G = vt(n, "__date", function (it) { it.style = { fontFamily: a.fontFamily, fontSize: a.fontSize, fill: a.text, fontWeight: "400", wordWrap: !1 }; });
        G.text = j, G.visible = !0, G.position.set(H, 9 + Pt);
        var W = vt(n, "__time", function (it) { it.style = { fontFamily: a.fontFamily, fontSize: a.fontSize, fill: a.text, fontWeight: "400", wordWrap: !1 }; });
        W.text = f, W.visible = !0, W.position.set(X + H, 9 + Pt);
        var tt = n.getChildByLabel ? n.getChildByLabel("__shown") : n.children.find(function (it) { return (it == null ? void 0 : it.label) === "__shown"; });
        tt && (tt.visible = !1), pi(r, Math.max(X + 0, X + (i - X) - 18), 11, 10, a.text);
    } Jt(n), n.eventMode = "static", n.cursor = "pointer", n.hitArea = new Ot(0, 0, Math.max(0, i), Math.max(0, o)), n.on("pointerdown", function (X) { var f, G, W, tt; if ((X == null ? void 0 : X.button) === 2)
        return; var j = m(X); if (!(j <= 0)) {
        if (h.focusedKeyByPointer.set(j, R), h.keyboardOwnerPointerId = j, A !== "datetime-local")
            N.openPanel = N.openPanel ? null : A === "time" ? "time" : A === "month" ? "month" : "week", N.openYear = !1, N.openMonthGrid = !1;
        else {
            var z = ((G = (f = X.global) == null ? void 0 : f.x) != null ? G : 0) - s <= i * .52;
            N.openPanel = z ? N.openPanel === "week" ? null : "week" : N.openPanel === "time" ? null : "time", N.openYear = !1, N.openMonthGrid = !1;
        }
        b.set(R, N), (W = _ != null ? _ : u) == null || W(), (tt = X.stopPropagation) == null || tt.call(X);
    } }), N.openPanel === "month" ? p.push({ kind: "month-panel", inputKey: R, absX: s, absY: c, anchorW: i, anchorH: o }) : N.openPanel === "week" ? p.push({ kind: "week-panel", inputKey: R, absX: s, absY: c, anchorW: i, anchorH: o }) : N.openPanel === "time" && p.push({ kind: "time-panel", inputKey: R, absX: s, absY: c, anchorW: i, anchorH: o }); }
    function je(t, e, n, r) { t.rect(0, 0, n, r), t.fill(e.control.background), t.rect(.5, .5, Math.max(0, n - 1), Math.max(0, r - 1)), t.stroke({ width: 1, color: e.control.border, alignment: 0 }); }
    function Go(t) { var e = t.stage, n = t.theme, r = t.popup, i = t.st, o = t.viewportW, s = t.viewportH, c = t.getPointerId, a = t.requestPaint, h = t.onPick, m = 4, d = 3, b = 44, y = 34, w = 8, u = w * 2 + m * b, _ = w * 2 + d * y, p = r.absX, R = r.absY + r.anchorH; p = Math.max(0, Math.min(p, Math.max(0, o - u))), R + _ > s - 4 && (R = r.absY - _), R = Math.max(0, Math.min(R, Math.max(0, s - _))); var A = new Ct; A.position.set(p, R), e.addChild(A); var N = new kt; je(N, n, u, _), A.addChild(N); for (var D = 0; D < 12; D++) {
        var x = D + 1, C = w + D % m * b, H = w + Math.floor(D / m) * y;
        if (x === i.month) {
            var S = new kt;
            S.rect(C + 1, H + 1, b - 2, y - 2), S.fill({ color: 0, alpha: .06 }), A.addChild(S);
        }
        var k = ie({ text: String(x), fontFamily: n.fontFamily, fontSize: n.fontSize, fill: n.text, wordWrap: !1 });
        k.position.set(C + 14, H + 8 + Pt), A.addChild(k), N.rect(C, H, b, y), N.stroke({ width: 1, color: n.control.border, alignment: 0 });
    } A.eventMode = "static", A.cursor = "pointer", A.hitArea = new Ot(0, 0, u, _), A.on("pointerdown", function (D) { var tt, it, st; if ((D == null ? void 0 : D.button) === 2 || c(D) <= 0)
        return; var C = A.toLocal(D.global), H = (tt = C == null ? void 0 : C.x) != null ? tt : -1, k = (it = C == null ? void 0 : C.y) != null ? it : -1, S = H - w, X = k - w; if (S < 0 || X < 0)
        return; var j = Math.floor(S / b), f = Math.floor(X / y); if (j < 0 || j >= m || f < 0 || f >= d)
        return; var W = f * m + j + 1; W < 1 || W > 12 || (h(W), a == null || a(), (st = D.stopPropagation) == null || st.call(D)); }); }
    function Ho(t) {
        var e_19, _a;
        var e = t.stage, n = t.theme, r = t.popup, i = t.st, o = t.viewportW, s = t.viewportH, c = t.sliders, a = t.sliderBounds, h = t.sliderDrags, m = t.getPointerId, d = t.requestPaint, b = t.onChange, y = 10, w = 250, u = 78, _ = r.absX, p = r.absY;
        _ = r.absX + r.anchorW + 6, p = r.absY, _ = Math.max(0, Math.min(_, Math.max(0, o - w))), p = Math.max(0, Math.min(p, Math.max(0, s - u)));
        var R = new Ct;
        R.position.set(_, p), e.addChild(R);
        var A = new kt;
        je(A, n, w, u), R.addChild(A);
        var N = ie({ text: "20".concat(Zt(i.year2)), fontFamily: n.fontFamily, fontSize: n.fontSize, fill: n.text, wordWrap: !1 });
        N.position.set(y, 8 + Pt), R.addChild(N);
        var D = i.yearSliderKey, x = Math.max(0, Math.min(1, Wt(i.year2, 0, 99) / 99)), C = Pe(c, D, { value: String(x) }), H = !1;
        try {
            for (var _b = __values(h.values()), _c = _b.next(); !_c.done; _c = _b.next()) {
                var j = _c.value;
                if (j.key === D) {
                    H = !0;
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
        H || (C.value = x);
        var k = new Ct;
        k.position.set(y, 40), R.addChild(k);
        var S = new kt;
        k.addChild(S), ln({ node: { key: D, attrs: { value: String(C.value) } }, container: k, graphics: S, w: w - y * 2, h: 14, absX: _ + y, absY: p + 40, theme: { text: n.text, control: { progress: n.control.progress } }, sliderStates: c, sliderBounds: a, sliderDrags: h, requestPaint: d, getPointerId: m });
        var X = Wt(Math.round(C.value * 99), 0, 99);
        X !== i.year2 && b(X), R.eventMode = "static", R.hitArea = new Ot(0, 0, w, u), R.on("pointerdown", function (j) { var f; (f = j.stopPropagation) == null || f.call(j); });
    }
    function Wo(t) { var e = t.panel, n = t.theme, r = t.x, i = t.y, o = t.w, s = t.st, c = t.onPick, a = 30, h = 6, m = []; for (var d = 0; d < 4; d++) {
        var b = d + 1, y = i + d * (a + h), w = new kt;
        w.rect(r, y, o, a), w.fill({ color: 0, alpha: b === s.weekIndex ? .06 : .03 }), w.rect(r + .5, y + .5, Math.max(0, o - 1), Math.max(0, a - 1)), w.stroke({ width: 1, color: n.control.border, alignment: 0 }), e.addChild(w);
        var u = (Wt(s.month, 1, 12) - 1) * 4 + b, _ = ie({ text: "".concat(b, " [").concat(Zt(u), "]"), fontFamily: n.fontFamily, fontSize: n.fontSize, fill: n.text, wordWrap: !1 });
        _.position.set(r + 10, y + 7 + Pt), e.addChild(_), m.push({ x: r, y: y, w: o, h: a, weekIndex: b });
    } return { hitRects: m }; }
    function $n(t) {
        var e_20, _a, e_21, _b;
        var A, N, D, x, C, H;
        var e = t.popups, n = t.stage, r = t.theme, i = t.viewportW, o = t.viewportH, s = t.temporalStates, c = t.getOrInitInputValue, a = t.sliders, h = t.sliderBounds, m = t.sliderDrags, d = t.selects, b = t.selectPopups, y = t.getCursorColor, w = t.uiFocus, u = t.getPointerId, _ = t.requestPaint, p = t.requestOverlayPaint, R = [];
        var _loop_1 = function (k) {
            var S = s.get(k.inputKey);
            if (S) {
                if (k.kind === "month-panel") {
                    var nt = k.absX, L = k.absY + k.anchorH;
                    nt = Math.max(0, Math.min(nt, Math.max(0, i - 196))), L + 156 > o - 4 && (L = k.absY - 156), L = Math.max(0, Math.min(L, Math.max(0, o - 156)));
                    var V_1 = new Ct;
                    V_1.position.set(nt, L), n.addChild(V_1);
                    var K = new kt;
                    je(K, r, 196, 156), V_1.addChild(K);
                    var U_1 = { x: 10, y: 10, w: 132, h: 24 };
                    {
                        var $ = new kt;
                        $.rect(U_1.x, U_1.y, U_1.w, U_1.h), $.fill({ color: 0, alpha: .03 }), $.rect(U_1.x + .5, U_1.y + .5, Math.max(0, U_1.w - 1), Math.max(0, U_1.h - 1)), $.stroke({ width: 1, color: r.control.border, alignment: 0 }), V_1.addChild($);
                        var ft = ie({ text: "Year 20".concat(Zt(S.year2)), fontFamily: r.fontFamily, fontSize: r.fontSize, fill: r.text, wordWrap: !1 });
                        ft.position.set(U_1.x + 8, U_1.y + 4 + Pt), V_1.addChild(ft);
                    }
                    var gt_1 = 10, F_1 = 44;
                    for (var $ = 0; $ < 12; $++) {
                        var ft = $ + 1, q = gt_1 + $ % 4 * 44, ot = F_1 + Math.floor($ / 4) * 34;
                        if (ft === S.month) {
                            var g = new kt;
                            g.rect(q + 1, ot + 1, 42, 32), g.fill({ color: 0, alpha: .06 }), V_1.addChild(g);
                        }
                        var bt = ie({ text: String(ft), fontFamily: r.fontFamily, fontSize: r.fontSize, fill: r.text, wordWrap: !1 });
                        bt.position.set(q + 14, ot + 8 + Pt), V_1.addChild(bt), K.rect(q, ot, 44, 34), K.stroke({ width: 1, color: r.control.border, alignment: 0 });
                    }
                    V_1.eventMode = "static", V_1.cursor = "pointer", V_1.hitArea = new Ot(0, 0, 196, 156), V_1.on("pointerdown", function ($) { var J, Q, Y, lt, Z; if (($ == null ? void 0 : $.button) === 2)
                        return; var ft = u($); if (ft <= 0)
                        return; w.focusedKeyByPointer.set(ft, k.inputKey), w.keyboardOwnerPointerId = ft; var q = V_1.toLocal($.global), ot = (J = q == null ? void 0 : q.x) != null ? J : -1, bt = (Q = q == null ? void 0 : q.y) != null ? Q : -1; if (ot >= U_1.x && ot <= U_1.x + U_1.w && bt >= U_1.y && bt <= U_1.y + U_1.h) {
                        S.openYear = !0, s.set(k.inputKey, S), (Y = p != null ? p : _) == null || Y(), (lt = $.stopPropagation) == null || lt.call($);
                        return;
                    } var T = ot - gt_1, v = bt - F_1; if (T < 0 || v < 0)
                        return; var M = Math.floor(T / 44), E = Math.floor(v / 34); if (M < 0 || M >= 4 || E < 0 || E >= 3)
                        return; var I = E * 4 + M + 1; if (I < 1 || I > 12)
                        return; S.month = I, S.openPanel = null, S.openYear = !1, S.openMonthGrid = !1, s.set(k.inputKey, S); var O = c(k.inputKey, { type: "text" }); O.value = fn(S), _ == null || _(), (Z = $.stopPropagation) == null || Z.call($); }), V_1.on("pointerdown", function ($) { var ft; (ft = $.stopPropagation) == null || ft.call($); }), S.openYear && R.push({ kind: "year-panel", inputKey: k.inputKey, absX: nt, absY: L, anchorW: 196, anchorH: 0 });
                }
                if (k.kind === "week-panel") {
                    var G = k.absX, W = k.absY + k.anchorH;
                    G = Math.max(0, Math.min(G, Math.max(0, i - 280))), W + 192 > o - 4 && (W = k.absY - 192), W = Math.max(0, Math.min(W, Math.max(0, o - 192)));
                    var tt_1 = new Ct;
                    tt_1.position.set(G, W), n.addChild(tt_1);
                    var it = new kt;
                    je(it, r, 280, 192), tt_1.addChild(it);
                    var st_1 = { x: 10, y: 10, w: 104, h: 24 }, z_1 = { x: 10 + st_1.w + 10, y: 10, w: 120, h: 24 }, nt = function (K, U) { var gt = new kt; gt.rect(K.x, K.y, K.w, K.h), gt.fill({ color: 0, alpha: .03 }), gt.rect(K.x + .5, K.y + .5, Math.max(0, K.w - 1), Math.max(0, K.h - 1)), gt.stroke({ width: 1, color: r.control.border, alignment: 0 }), tt_1.addChild(gt); var F = ie({ text: U, fontFamily: r.fontFamily, fontSize: r.fontSize, fill: r.text, wordWrap: !1 }); F.position.set(K.x + 8, K.y + 4 + Pt), tt_1.addChild(F); };
                    nt(st_1, "Month ".concat(S.month)), nt(z_1, "Year 20".concat(Zt(S.year2)));
                    var L = 44, V_2 = Wo({ panel: tt_1, theme: r, x: 10, y: L, w: 280 - 10 * 2, st: S, onPick: function () { } }).hitRects;
                    tt_1.eventMode = "static", tt_1.cursor = "pointer", tt_1.hitArea = new Ot(0, 0, 280, 192), tt_1.on("pointerdown", function (K) {
                        var e_23, _a;
                        var q, ot, bt, g, T, v, M;
                        if ((K == null ? void 0 : K.button) === 2)
                            return;
                        var U = u(K);
                        if (U <= 0)
                            return;
                        w.focusedKeyByPointer.set(U, k.inputKey), w.keyboardOwnerPointerId = U;
                        var gt = tt_1.toLocal(K.global), F = (q = gt == null ? void 0 : gt.x) != null ? q : -1, $ = (ot = gt == null ? void 0 : gt.y) != null ? ot : -1, ft = function (E) { return F >= E.x && F <= E.x + E.w && $ >= E.y && $ <= E.y + E.h; };
                        if (ft(st_1)) {
                            S.openMonthGrid = !S.openMonthGrid, s.set(k.inputKey, S), (bt = p != null ? p : _) == null || bt(), (g = K.stopPropagation) == null || g.call(K);
                            return;
                        }
                        if (ft(z_1)) {
                            S.openYear = !0, s.set(k.inputKey, S), (T = p != null ? p : _) == null || T(), (v = K.stopPropagation) == null || v.call(K);
                            return;
                        }
                        try {
                            for (var V_3 = (e_23 = void 0, __values(V_2)), V_3_1 = V_3.next(); !V_3_1.done; V_3_1 = V_3.next()) {
                                var E = V_3_1.value;
                                if (ft(E)) {
                                    S.weekIndex = E.weekIndex;
                                    var P = c(k.inputKey, { type: "text" });
                                    S.kind === "week" ? P.value = pn(S) : S.kind === "date" ? P.value = De(S) : P.value = ze(S), S.openPanel = null, S.openYear = !1, S.openMonthGrid = !1, s.set(k.inputKey, S), _ == null || _(), (M = K.stopPropagation) == null || M.call(K);
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
                    }), S.openMonthGrid && R.push({ kind: "month-grid", inputKey: k.inputKey, absX: G, absY: W + st_1.y + st_1.h + 4, anchorW: 0, anchorH: 0 }), S.openYear && R.push({ kind: "year-panel", inputKey: k.inputKey, absX: G + z_1.x, absY: W + z_1.y, anchorW: z_1.w, anchorH: 0 });
                }
                if (k.kind === "time-panel") {
                    var G_1 = k.absX, W_1 = k.absY + k.anchorH;
                    G_1 = Math.max(0, Math.min(G_1, Math.max(0, i - 330))), W_1 + 80 > o - 4 && (W_1 = k.absY - 80), W_1 = Math.max(0, Math.min(W_1, Math.max(0, o - 80)));
                    var tt_2 = new Ct;
                    tt_2.position.set(G_1, W_1), n.addChild(tt_2);
                    var it = new kt;
                    je(it, r, 330, 80), tt_2.addChild(it);
                    var st = ie({ text: "Time", fontFamily: r.fontFamily, fontSize: r.fontSize, fill: r.text, wordWrap: !1 });
                    st.position.set(10, 8 + Pt), tt_2.addChild(st);
                    var z_2 = function (E) { return Array.from({ length: E }, function (P, I) { return Zt(I); }).join("\n"); }, nt = k.inputKey, L = "".concat(nt, ":time-h"), V = "".concat(nt, ":time-m"), K = "".concat(nt, ":time-s"), U = Ee(d, L, Wt(S.hour, 0, 23)), gt = Ee(d, V, Wt(S.minute, 0, 59)), F = Ee(d, K, Wt(S.second, 0, 59));
                    U.selectedIndex = Wt(S.hour, 0, 23), gt.selectedIndex = Wt(S.minute, 0, 59), F.selectedIndex = Wt(S.second, 0, 59);
                    var $_2 = 96, ft_1 = 36, q_1 = 32, ot = 8, bt = function (E, P, I) { var O = new Ct; O.position.set(P, q_1), tt_2.addChild(O); var J = new kt; O.addChild(J), mn({ node: { key: E, attrs: { "data-options": z_2(I), "data-selected-index": String(Ee(d, E, 0).selectedIndex) } }, container: O, graphics: J, w: $_2, h: ft_1, absX: G_1 + P, absY: W_1 + q_1, theme: r, selectStates: d, uiState: w, getPointerId: u, getCursorColor: y, requestPaint: _, requestOverlayPaint: p, popupSink: b }); };
                    bt(L, 10, 24), bt(V, 10 + $_2 + ot, 60), bt(K, 10 + ($_2 + ot) * 2, 60);
                    var g = Wt((N = (A = d.get(L)) == null ? void 0 : A.selectedIndex) != null ? N : S.hour, 0, 23), T = Wt((x = (D = d.get(V)) == null ? void 0 : D.selectedIndex) != null ? x : S.minute, 0, 59), v = Wt((H = (C = d.get(K)) == null ? void 0 : C.selectedIndex) != null ? H : S.second, 0, 59);
                    S.hour = g, S.minute = T, S.second = v, s.set(k.inputKey, S);
                    var M = c(k.inputKey, { type: "text" });
                    S.kind === "time" ? M.value = Ve(S) : M.value = ze(S), tt_2.eventMode = "static", tt_2.hitArea = new Ot(0, 0, 330, 80), tt_2.on("pointerdown", function (E) { var P; (P = E.stopPropagation) == null || P.call(E); });
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
            var S = s.get(k.inputKey);
            S && (k.kind === "month-grid" && Go({ stage: n, theme: r, popup: k, st: S, viewportW: i, viewportH: o, getPointerId: u, requestPaint: _, onPick: function (X) { S.month = X, S.openMonthGrid = !1, s.set(k.inputKey, S); var j = c(k.inputKey, { type: "text" }); S.kind === "month" ? j.value = fn(S) : S.kind === "week" ? j.value = pn(S) : S.kind === "date" ? j.value = De(S) : j.value = ze(S); } }), k.kind === "year-panel" && Ho({ stage: n, theme: r, popup: k, st: S, viewportW: i, viewportH: o, sliders: a, sliderBounds: h, sliderDrags: m, getPointerId: u, requestPaint: _, onChange: function (X) { S.year2 = X, s.set(k.inputKey, S); var j = c(k.inputKey, { type: "text" }); S.kind === "month" ? j.value = fn(S) : S.kind === "week" ? j.value = pn(S) : S.kind === "date" ? j.value = De(S) : S.kind === "time" ? j.value = Ve(S) : j.value = ze(S); } }));
        };
        try {
            for (var R_1 = __values(R), R_1_1 = R_1.next(); !R_1_1.done; R_1_1 = R_1.next()) {
                var k = R_1_1.value;
                _loop_2(k);
            }
        }
        catch (e_21_1) { e_21 = { error: e_21_1 }; }
        finally {
            try {
                if (R_1_1 && !R_1_1.done && (_b = R_1.return)) _b.call(R_1);
            }
            finally { if (e_21) throw e_21.error; }
        }
    }
    function _i(t) {
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
    var yi = 5e4, Je = new WeakMap, wi = new Map, Fo = 1, Ti = 0, $o = 0, xi = !1, Re = [], Un = null;
    function He(t) { return t instanceof kt ? "Graphics" : t instanceof re ? "Text" : t instanceof Ct ? "Container" : "Object"; }
    function Uo(t) { var e = t && typeof t == "object" ? t.label : void 0, n = t && typeof t == "object" ? He(t) : "Object"; return e ? "".concat(n, ":").concat(String(e).slice(0, 80)) : n; }
    function pe(t) { var e = Je.get(t); return e || (e = Fo++, Je.set(t, e)), wi.set(e, t), e; }
    function bn(t) { var e, n, r, i, o, s; if (t == null || typeof t == "number" || typeof t == "string" || typeof t == "boolean")
        return t; if (Array.isArray(t))
        return t.slice(0, 16).map(bn); if (typeof t == "object") {
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
    function Xn(t) { if (t != null)
        return typeof t == "symbol" ? t.toString() : String(t); }
    function Ei(t) { if (t != null)
        return typeof t == "function" ? { type: "function", name: t.name || void 0, arity: t.length } : typeof t == "object" ? { id: pe(t), type: He(t) } : { type: typeof t }; }
    function Bo(t) { if (t != null)
        return typeof t == "object" ? { id: pe(t), type: He(t) } : typeof t == "function" ? { type: "function" } : { type: typeof t }; }
    function Xo(t) { var e = { event: Xn(t[0]), listener: Ei(t[1]) }; return t.length > 2 && (e.context = Bo(t[2])), [e]; }
    function Yo(t) { return String(t != null ? t : "").slice(0, 240); }
    function Ko(t) {
        var e_26, _a;
        var r, i;
        if (!t || typeof t != "object")
            return bn(t);
        var e = t, n = { type: (i = (r = t.constructor) == null ? void 0 : r.name) != null ? i : "object" };
        try {
            for (var _b = __values(["fontFamily", "fontSize", "fontStyle", "fontWeight", "fill", "align", "lineHeight", "letterSpacing", "wordWrap", "wordWrapWidth", "padding"]), _c = _b.next(); !_c.done; _c = _b.next()) {
                var o = _c.value;
                var s = e[o];
                s !== void 0 && (n[o] = bn(s));
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
    function zo(t) { var s, c, a, h, m, d; if (!t || typeof t != "object")
        return; var e = t, n = Number((s = e.x) != null ? s : 0), r = Number((c = e.y) != null ? c : 0), i = Number((h = (a = e.width) != null ? a : e.w) != null ? h : 0), o = Number((d = (m = e.height) != null ? m : e.h) != null ? d : 0); if (!(!Number.isFinite(n) || !Number.isFinite(r) || !Number.isFinite(i) || !Number.isFinite(o)) && !(i <= 0 || o <= 0))
        return { x: n, y: r, w: i, h: o }; }
    function jo(t, e) { if (e) {
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
        return t === "on" ? Xo(e) : t === "snapshot" ? e : t === "text.text.set" ? e.length ? [Yo(e[0])] : [] : t === "text.style.set" ? e.length ? [Ko(e[0])] : [] : e.map(bn);
    } }
    function _n(t, e, n) { var r, i; try {
        window.__TRUEOS_PIXI_CAPTURE_STEP__ = "record:".concat(e, ":begin");
        var o = window.__pixiCapture;
        if (!(o != null && o.enabled))
            return;
        o.counts[e] = ((r = o.counts[e]) != null ? r : 0) + 1;
        var s = { frame: Ti, seq: ++$o, op: e, id: t && typeof t == "object" ? pe(t) : void 0, target: Uo(t), event: e === "on" && (n != null && n.length) ? Xn(n[0]) : void 0, listener: e === "on" && (n != null && n.length) ? Ei(n[1]) : void 0, args: jo(e, n) };
        window.__TRUEOS_PIXI_CAPTURE_STEP__ = "record:".concat(e, ":push"), o.commands.push(s), o.persist && Vo(s), o.commands.length > yi && o.commands.splice(0, o.commands.length - yi), window.__TRUEOS_PIXI_CAPTURE_STEP__ = "record:".concat(e, ":done");
    }
    catch (o) {
        try {
            window.__TRUEOS_PIXI_CAPTURE_ERROR__ = "record:".concat(e, ":").concat(String((i = o == null ? void 0 : o.message) != null ? i : o));
        }
        catch (s) { }
    } }
    function Vo(t) { if (Re.push(t), t.op === "snapshot") {
        Qe();
        return;
    } if (Re.length >= 512) {
        Qe();
        return;
    } Un == null && (Un = window.setTimeout(function () { Un = null, Qe(); }, 50)); }
    function Qe() {
        if (Re.length === 0)
            return;
        var t = Re;
        Re = [];
        var e = t.map(function (n) { return JSON.stringify(n); }).join("\n") + "\n";
        navigator.sendBeacon && navigator.sendBeacon("/__pixi_capture", new Blob([e], { type: "application/x-ndjson" })) || fetch("/__pixi_capture", { method: "POST", headers: { "Content-Type": "application/x-ndjson" }, body: e, keepalive: !0 }).catch(function () { Re = t.concat(Re); });
    }
    function Jo(t, e, n) {
        var e_27, _a, e_28, _b, e_29, _c;
        var r, i;
        if (e === "on") {
            var o = Xn(n[0]), s = n[1];
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
    function Qo() { var t = function () { var n; return Number((n = window.__TRUEOS_PIXI_RENDER_SERIAL__) != null ? n : 0) || 0; }, e = function () { return !!(window.__TRUEOS_PIXI_REPAINT_REQUIRED__ || window.__TRUEOS_PIXI_SCROLL_REPAINT_REQUIRED__ || window.__TRUEOS_PIXI_OVERLAY_REPAINT_REQUIRED__); }; window.__TRUEOS_DISPATCH_PIXI_POINTER__ = function (n, r, i, o, s, c, a) {
        var e_30, _a;
        if (a === void 0) { a = 0; }
        var H, k, S, X, j, f, G, W, tt, it, st, z, nt, L, V, K, U, gt, F, $, ft;
        var h = function (q) { try {
            window.__TRUEOS_PIXI_POINTER_DISPATCH_STEP__ = q, typeof console != "undefined" && typeof console.log == "function" && console.log("[trueos pointer dispatch] ".concat(q));
        }
        catch (ot) { } };
        h("start node=".concat(Number(n) || 0, " event=").concat(String(r || "")));
        var m = window.__TRUEOS_PIXI_APP;
        if (String(r || "") === "wheel") {
            var q = m == null ? void 0 : m.canvas;
            if (!q || typeof q.dispatchEvent != "function")
                return h("wheel-canvas-missing"), { handled: 0, listenerCount: 0, painted: 0, targetFound: 0 };
            var ot = (S = (k = (H = window.__pixiCapture) == null ? void 0 : H.commands) == null ? void 0 : k.length) != null ? S : 0, bt = { type: "wheel", deltaX: 0, deltaY: Number(a) || 0, deltaMode: 0, offsetX: Number(i) || 0, offsetY: Number(o) || 0, clientX: Number(i) || 0, clientY: Number(o) || 0, pointerId: Number(s) || 1, buttons: Number(c) || 0, defaultPrevented: !1, propagationStopped: !1, preventDefault: function () { this.defaultPrevented = !0; }, stopPropagation: function () { this.propagationStopped = !0; } };
            h("wheel-dispatch deltaY=".concat(bt.deltaY));
            var g = t();
            q.dispatchEvent(bt);
            var T = 0;
            if (window.__TRUEOS_CAPTURE_ONLY__) {
                var J = window.__TRUEOS_REPAINT_NOW__;
                e() && typeof J == "function" && (h("wheel-repaint-call"), J(), h("wheel-repaint-return"), T = 1);
            }
            else
                (X = m == null ? void 0 : m.renderer) != null && X.render && (m != null && m.stage) && (m.renderer.render(m.stage), T = 1);
            var v = (G = (f = (j = window.__pixiCapture) == null ? void 0 : j.commands) == null ? void 0 : f.length) != null ? G : ot, M = t() !== g, E = (W = q.listeners) == null ? void 0 : W.wheel, P = Array.isArray(E) ? E.length : typeof E == "function" ? 1 : 0, I = bt.defaultPrevented || P > 0 ? 1 : 0;
            h("wheel-done handled=".concat(I, " listeners=").concat(P, " painted=").concat(T));
            var O = window.__TRUEOS_PIXI_LAST_SCROLL_FAST_PATH__;
            return (O == null ? void 0 : O.owner) === "root" || (O == null ? void 0 : O.owner) === "iframe" ? { handled: I, listenerCount: P, painted: 1, targetFound: 1, scrollFastPath: 1, rootNode: Number(O.rootNode) || 0, contentNode: Number(O.contentNode) || 0, contentY: Number(O.contentY) || 0, scrollbarNode: Number(O.scrollbarNode) || 0, scrollbarVisible: Number(O.scrollbarVisible) || 0, trackX: Number(O.trackX) || 0, trackY: Number(O.trackY) || 0, trackW: Number(O.trackW) || 0, trackH: Number(O.trackH) || 0, thumbX: Number(O.thumbX) || 0, thumbY: Number(O.thumbY) || 0, thumbW: Number(O.thumbW) || 0, thumbH: Number(O.thumbH) || 0 } : { handled: I, listenerCount: P, painted: v > ot || M || T ? 1 : 0, targetFound: 1 };
        }
        var d = wi.get(Number(n) || 0), b = 0, y = 0, w = 0;
        if (!d)
            return h("target-missing"), { handled: b, listenerCount: y, painted: w, targetFound: 0 };
        window.__TRUEOS_PIXI_LAST_GRAPHICS_FAST_PATH__ = null, window.__TRUEOS_PIXI_LAST_OVERLAY_FAST_PATH__ = null, window.__TRUEOS_PIXI_LAST_SCROLL_FAST_PATH__ = null;
        var u = { type: String(r || ""), button: Number(c) & 2 ? 2 : 0, buttons: Number(c) || 0, pointerId: Number(s) || 1, pointerType: "mouse", global: { x: Number(i) || 0, y: Number(o) || 0 }, data: { pointerId: Number(s) || 1, pointerType: "mouse", global: { x: Number(i) || 0, y: Number(o) || 0 } }, target: d, currentTarget: d, defaultPrevented: !1, propagationStopped: !1, preventDefault: function () { this.defaultPrevented = !0; }, stopPropagation: function () { this.propagationStopped = !0; } }, _ = (st = (it = (tt = window.__pixiCapture) == null ? void 0 : tt.commands) == null ? void 0 : it.length) != null ? st : 0, p = t();
        h("target-found label=".concat(String((z = d.label) != null ? z : "")));
        for (var q = d; q; q = q.parent) {
            u.currentTarget = q;
            var ot = (nt = q.listeners) == null ? void 0 : nt[u.type];
            if (!(!Array.isArray(ot) || ot.length === 0)) {
                y += ot.length, h("listeners node=".concat((L = Je.get(q)) != null ? L : 0, " count=").concat(ot.length));
                try {
                    for (var _b = (e_30 = void 0, __values(ot.slice())), _c = _b.next(); !_c.done; _c = _b.next()) {
                        var bt = _c.value;
                        if (typeof bt == "function" && (b = 1, h("listener-call node=".concat((V = Je.get(q)) != null ? V : 0)), bt.call(q, u), h("listener-return node=".concat((K = Je.get(q)) != null ? K : 0)), u.propagationStopped))
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
                if (u.propagationStopped)
                    break;
            }
        }
        if (window.__TRUEOS_CAPTURE_ONLY__) {
            var q = window.__TRUEOS_REPAINT_NOW__;
            e() && typeof q == "function" && (h("capture-repaint-call"), q(), h("capture-repaint-return"), w = 1);
        }
        else
            (U = m == null ? void 0 : m.renderer) != null && U.render && (m != null && m.stage) && (h("paint-call"), m.renderer.render(m.stage), h("paint-return"), w = 1);
        var R = ($ = (F = (gt = window.__pixiCapture) == null ? void 0 : gt.commands) == null ? void 0 : F.length) != null ? $ : _, A = t() !== p, N = window.__TRUEOS_PIXI_LAST_SCROLL_FAST_PATH__;
        if ((N == null ? void 0 : N.owner) === "root" || (N == null ? void 0 : N.owner) === "iframe")
            return h("scroll-fast owner=".concat(N.owner)), { handled: b, listenerCount: y, painted: 1, targetFound: 1, scrollFastPath: 1, rootNode: Number(N.rootNode) || 0, contentNode: Number(N.contentNode) || 0, contentY: Number(N.contentY) || 0, scrollbarNode: Number(N.scrollbarNode) || 0, scrollbarVisible: Number(N.scrollbarVisible) || 0, trackX: Number(N.trackX) || 0, trackY: Number(N.trackY) || 0, trackW: Number(N.trackW) || 0, trackH: Number(N.trackH) || 0, thumbX: Number(N.thumbX) || 0, thumbY: Number(N.thumbY) || 0, thumbW: Number(N.thumbW) || 0, thumbH: Number(N.thumbH) || 0 };
        var D = window.__TRUEOS_PIXI_LAST_GRAPHICS_FAST_PATH__, x = (D == null ? void 0 : D.owner) === "button-hover" ? u.type === "pointerover" || u.type === "pointerout" || u.type === "pointerdown" || u.type === "pointerup" : u.type === "pointerover" || u.type === "pointerout";
        if (((D == null ? void 0 : D.owner) === "context-menu-hover" || (D == null ? void 0 : D.owner) === "button-hover") && x && R > _)
            return (ft = window.__pixiCapture) != null && ft.commands && window.__pixiCapture.commands.splice(_, R - _), h("graphics-fast owner=".concat(D.owner)), { handled: b, listenerCount: y, painted: 1, targetFound: 1, graphicsFastPath: 1, rootNode: Number(D.rootNode) || 0, graphicsNode: Number(D.graphicsNode) || 0, rectX: Number(D.x) || 0, rectY: Number(D.y) || 0, rectW: Number(D.w) || 0, rectH: Number(D.h) || 0, damageX: Number(D.worldX) + Number(D.x) || 0, damageY: Number(D.worldY) + Number(D.y) || 0, damageW: Number(D.w) || 0, damageH: Number(D.h) || 0, fillColor: Number(D.fillColor) || 0, fillAlpha: Number(D.fillAlpha) || 0, strokeColor: Number(D.strokeColor) || 0, strokeAlpha: Number(D.strokeAlpha) || 0, strokeWidth: Number(D.strokeWidth) || 0 };
        var C = window.__TRUEOS_PIXI_LAST_OVERLAY_FAST_PATH__;
        return C && Number(C.rootNode) > 0 && Number(C.damageW) > 0 && Number(C.damageH) > 0 ? (h("overlay-fast"), { handled: b, listenerCount: y, painted: 1, targetFound: 1, overlayFastPath: 1, rootNode: Number(C.rootNode) || 0, damageX: Number(C.damageX) || 0, damageY: Number(C.damageY) || 0, damageW: Number(C.damageW) || 0, damageH: Number(C.damageH) || 0 }) : (w = R > _ || A || w ? 1 : 0, h("done handled=".concat(b, " listeners=").concat(y, " painted=").concat(w)), { handled: b, listenerCount: y, painted: w, targetFound: 1 });
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
            if (_n(this, n, s), !window.__TRUEOS_CAPTURE_ONLY__)
                return r.apply(this, s);
            try {
                window.__TRUEOS_PIXI_CAPTURE_STEP__ = "invoke:".concat(n, ":begin");
                var a = Jo(this, n, s);
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
    function Zo(t, e) { var n = t; for (; n;) {
        var r = Object.getOwnPropertyDescriptor(n, e);
        if (r)
            return r;
        n = Object.getPrototypeOf(n);
    } }
    function ve(t, e, n) { var o, s; if (!(t != null && t.constructor) || t.constructor["__pixiCapturePatched_".concat(n)])
        return; var r = Zo(t, e); if ((r == null ? void 0 : r.configurable) === !1 || r && !r.set && !r.writable)
        return; var i = typeof Symbol == "function" ? Symbol("pixiCapture:".concat(n)) : "__pixiCaptureValue_".concat(n); Object.defineProperty(t, e, { configurable: (o = r == null ? void 0 : r.configurable) != null ? o : !0, enumerable: (s = r == null ? void 0 : r.enumerable) != null ? s : !0, get: r != null && r.get ? function () { var a; return (a = r.get) == null ? void 0 : a.call(this); } : function () { var a = this; return Object.prototype.hasOwnProperty.call(a, i) ? a[i] : r && "value" in r ? r.value : void 0; }, set: function (a) { if (_n(this, n, [a]), !window.__TRUEOS_CAPTURE_ONLY__) {
            r != null && r.set ? r.set.call(this, a) : Object.defineProperty(this, i, { configurable: !0, enumerable: !1, writable: !0, value: a });
            return;
        } var h = this; n === "text.text.set" ? h._text = String(a != null ? a : "") : n === "text.style.set" ? h._style = a != null ? a : {} : n === "text.resolution.set" ? h._resolution = Math.max(1, Number(a) || 1) : Object.defineProperty(h, i, { configurable: !0, enumerable: !1, writable: !0, value: a }); } }), t.constructor["__pixiCapturePatched_".concat(n)] = !0; }
    function Yn(t, e) {
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
        var i = { id: pe(t), type: He(t), label: (s = t.label) != null ? s : void 0, x: (h = (a = (c = t.position) == null ? void 0 : c.x) != null ? a : t.x) != null ? h : 0, y: (b = (d = (m = t.position) == null ? void 0 : m.y) != null ? d : t.y) != null ? b : 0, globalX: n, globalY: r, scaleX: Number.isFinite(Number((y = t.scale) == null ? void 0 : y.x)) ? Number(t.scale.x) : 1, scaleY: Number.isFinite(Number((w = t.scale) == null ? void 0 : w.y)) ? Number(t.scale.y) : 1, visible: t.visible, alpha: Number.isFinite(Number(t.alpha)) ? Number(t.alpha) : 1, maskId: t.mask ? pe(t.mask) : 0, zIndex: Number(t.zIndex) || 0, sortableChildren: t.sortableChildren === !0 }, o = zo(t.hitArea);
        if (o && (i.hitArea = o), t.listeners && typeof t.listeners == "object") {
            var u = Object.keys(t.listeners).filter(function (_) { var R; var p = (R = t.listeners) == null ? void 0 : R[_]; return Array.isArray(p) && p.length > 0; });
            u.length > 0 && (i.listeners = u.slice(0, 16));
        }
        if (t instanceof kt && Array.isArray(t.commands) && t.commands.length > 0 && (i.commands = t.commands.slice(-256).map(function (u) { return Ge(u, 0); })), typeof t.text == "string" && (i.text = t.text.slice(0, 120), t instanceof re && t.style && typeof t.style == "object")) {
            var u = {}, _ = t.style;
            typeof _.fontSize != "undefined" && (u.fontSize = Ge(_.fontSize, 0)), typeof _.fontWeight != "undefined" && (u.fontWeight = Ge(_.fontWeight, 0)), typeof _.fill != "undefined" && (u.fill = Ge(_.fill, 0)), Object.keys(u).length > 0 && (i.textStyle = u);
        }
        return Array.isArray(t.children) && t.children.length && (i.children = t.children.map(function (u) { return Yn(u, e + 1); })), i;
    }
    function Ii() {
        var e_31, _a, e_32, _b;
        if (window.__pixiCapture)
            return window.__pixiCapture;
        var t = { enabled: !0, persist: !window.__TRUEOS_CAPTURE_ONLY__, commands: [], counts: Object.create(null), objectId: function (e) { return pe(e); }, snapshotNode: function (e) { return Yn(e); }, clear: function () { this.commands.length = 0, this.counts = Object.create(null); }, dump: function (e) {
                if (e === void 0) { e = 200; }
                return this.commands.slice(-e);
            }, flush: function () { Qe(); }, summary: function () { return ae({}, this.counts); } };
        if (window.__pixiCapture = t, Qo(), window.addEventListener("beforeunload", function () { return Qe(); }), !xi) {
            xi = !0, typeof kt.prototype.image != "function" && (kt.prototype.image = function () { return this; });
            try {
                for (var _c = __values(["clear", "rect", "roundRect", "circle", "ellipse", "moveTo", "lineTo", "closePath", "poly", "fill", "stroke", "image", "svg"]), _d = _c.next(); !_d.done; _d = _c.next()) {
                    var e = _d.value;
                    Bn(kt.prototype, e);
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
                    Bn(Ct.prototype, e);
                }
            }
            catch (e_32_1) { e_32 = { error: e_32_1 }; }
            finally {
                try {
                    if (_g && !_g.done && (_b = _f.return)) _b.call(_f);
                }
                finally { if (e_32) throw e_32.error; }
            }
            ve(re.prototype, "text", "text.text.set"), ve(re.prototype, "style", "text.style.set"), ve(re.prototype, "resolution", "text.resolution.set"), Bn(re.prototype, "setSize", "text.setSize"), ve(Ct.prototype, "visible", "visible"), ve(Ct.prototype, "alpha", "alpha"), ve(Ct.prototype, "mask", "mask");
        }
        return t;
    }
    function Mi(t) { var e = t.renderer, n = e == null ? void 0 : e.render; if (typeof n != "function" || n.__pixiCapturePatched)
        return; var r = function (o) { var c, a; var s = o && typeof o == "object" && "container" in o ? o.container : o || t.stage; return Ti++, window.__TRUEOS_PIXI_RENDER_SERIAL__ = (Number((c = window.__TRUEOS_PIXI_RENDER_SERIAL__) != null ? c : 0) || 0) + 1, window.__TRUEOS_CAPTURE_ONLY__ && ((a = window.__pixiCapture) == null || a.clear()), _n(s, "render", []), _n(s, "snapshot", [Yn(s)]), window.__TRUEOS_CAPTURE_ONLY__ ? s : n.call(this, o); }; r.__pixiCapturePatched = !0, e.render = r; }
    Ii();
    var pt = null, wn = 6, Oe = 10, zt = 1, Vt = 3, Qt = 4, We = 512, vi = new Map;
    var l = { focusedKeyByPointer: new Map, keyboardOwnerPointerId: 1, inputs: new Map, sliders: new Map, sliderDrags: new Map, sliderBounds: new Map, dialogs: new Map, dialogDrags: new Map, dialogSelectedBy: new Map, dialogZ: new Map, dialogZCounter: 1, numbers: new Map, numberHolds: new Map, selects: new Map, temporals: new Map, temporalYearOwners: new Map, color: { rgb: { r: 255, g: 0, b: 0 }, a: 255, pick: null, draggingPointerId: null, bounds: null }, cursorColors: new Map, primaryMousePointerId: 1, harness: { enabled: !0, activeUserPointerId: zt, periodMs: 3e3 }, userCursorPos: new Map, lastMouse: { x: 0, y: 0, has: !1 }, scroll: { y: 0, contentHeight: 0, viewportHeight: 0, draggingPointerId: null, dragOffsetY: 0, track: { x: 0, y: 0, w: Oe, h: 0 }, thumb: { x: 0, y: 0, w: Oe, h: 0 } }, iframeScroll: new Map, iframeScrollRoots: new Map, iframeScrollbarGraphics: new Map, iframeRects: [], hoverRects: [], hoverHandlers: new Map, hoveredKeyByPointer: new Map, hoveredCursorByPointer: new Map, virtualCursor: { enabled: !1, x: 0, y: 0, t: 0, radius: 120, speed: .9 }, textDrags: new Map, fieldBounds: new Map, dialogDragBounds: new Map, detailsOpen: new Map, contextMenus: new Map, clipboards: new Map }, yn = null, Vn = 0;
    function ts(t) { if (!yn) {
        var n = document.createElement("canvas").getContext("2d");
        if (!n)
            throw new Error("2D canvas not available");
        yn = n;
    } return yn.font = "".concat(t.fontSize, "px ").concat(t.fontFamily), function (e) { return (Vn += 1, yn.measureText(e).width); }; }
    function zn(t, e) {
        if (e === void 0) { e = 16; }
        return Object.entries(t).sort(function (n, r) { return r[1] - n[1] || (n[0] < r[0] ? -1 : n[0] > r[0] ? 1 : 0); }).slice(0, e).map(function (_a) {
            var _b = __read(_a, 2), n = _b[0], r = _b[1];
            return "".concat(n, ":").concat(r);
        }).join(",");
    }
    function Kn(t) { var e = (2166136261 ^ t.length) >>> 0, n = function (o, s) { for (var c = o; c < s; c += 1) {
        var a = t.charCodeAt(c);
        e = e + (a & 65535) >>> 0, e = e + (e << 10) >>> 0, e ^= e >>> 6;
    } }, r = t.length, i = 4096; if (r <= i * 3)
        n(0, r);
    else {
        n(0, i);
        var o = Math.max(i, Math.floor((r - i) / 2));
        n(o, Math.min(r, o + i)), n(Math.max(0, r - i), r);
    } return e = e + (e << 3) >>> 0, e ^= e >>> 11, e = e + (e << 15) >>> 0, "0x".concat(e.toString(16).padStart(8, "0")); }
    function Gi(t) {
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
    function Hi(t) { return t.kind === "text" ? { kind: "text", text: t.text } : { kind: "block", key: t.key, tagName: t.tagName, attrs: Gi(t.attrs), children: t.children.map(Hi) }; }
    function Wi(t) { var e, n, r; return t.kind === "text" ? { kind: "text", text: (e = t.text) != null ? e : "", x: t.x, y: t.y, width: t.width, height: t.height, children: [] } : { kind: "block", key: (n = t.key) != null ? n : "", tagName: (r = t.tagName) != null ? r : "", attrs: Gi(t.attrs), x: t.x, y: t.y, width: t.width, height: t.height, children: t.children.map(Wi) }; }
    function es(t, e, n, r, i) {
        Dt("[trueos pixi widgets] prepixi stage=canonical-render begin");
        var o = e.map(Hi);
        Dt("[trueos pixi widgets] prepixi stage=canonical-render done"), Dt("[trueos pixi widgets] prepixi stage=canonical-layout begin");
        var s = Wi(n);
        Dt("[trueos pixi widgets] prepixi stage=canonical-layout done"), Dt("[trueos pixi widgets] prepixi stage=stringify begin");
        var c = JSON.stringify(o), a = JSON.stringify(s);
        Dt("[trueos pixi widgets] prepixi stage=stringify done render_bytes=".concat(c.length, " layout_bytes=").concat(a.length)), Dt("[trueos pixi widgets] prepixi stage=hash begin");
        var h = Kn(c), m = Kn(a), d = Kn("".concat(c, "\n").concat(a));
        Dt("[trueos pixi widgets] prepixi stage=hash done"), Dt("[trueos pixi widgets] prepixi stage=trace-stringify begin");
        var b = JSON.stringify({ version: 1, source: t, viewport: { width: r, height: i }, renderHash: h, layoutHash: m, hash: d, renderNodes: o, layout: s });
        return Dt("[trueos pixi widgets] prepixi stage=trace-stringify done bytes=".concat(b.length)), window.__TRUEOS_PIXI_PREPIX_TRACE__ = b, window.__TRUEOS_PIXI_PREPIX_HASH__ = d, window.__TRUEOS_PIXI_PREPIX_RENDER_HASH__ = h, window.__TRUEOS_PIXI_PREPIX_LAYOUT_HASH__ = m, Lt() && console.log("[trueos pixi widgets] prepixi source=".concat(t, " hash=").concat(d, " render_hash=").concat(h, " layout_hash=").concat(m, " bytes=").concat(b.length)), { hash: d, renderHash: h, layoutHash: m, bytes: b.length };
    }
    function Fe(t) { var e = typeof t == "string" ? t : ""; return e.indexOf("<truesurfer-") >= 0 && (e = e.replace(/<truesurfer-[A-Za-z0-9._-]+>/g, "")), e; }
    function ns(t, e) { if (e >= t.length)
        return !0; var n = t.charCodeAt(e); return n === 95 || n === 40 || n === 91 || n === 123 || n === 34 || n === 39 || n >= 48 && n <= 57 || n >= 65 && n <= 90; }
    function Fi(t) { var e = t, n = !0; for (; n;) {
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
        r >= 2 && ns(e, r) && (e = e.slice(r), n = !0);
    } return e; }
    function rs(t) { var e = Fe(t), n = e.indexOf("__trueos") >= 0 || e.indexOf("tsNu") >= 0 || e.indexOf("tsNum") >= 0; return e.indexOf("__TRUEOS_HOST_READY__") >= 0 && (e = e.replace(/__TRUEOS_HOST_READY__/g, "")), e.indexOf("__trueos") >= 0 && (e = is(e), e = e.replace(/__trueosNumberValue/g, "").replace(/__trueosHostNum/g, "").replace(/__trueosNum/g, "").replace(/__trueosNu/g, "").replace(/__trueos/g, "")), (e.indexOf("tsNu") >= 0 || e.indexOf("tsNum") >= 0) && (e = e.replace(/tsNum/g, "").replace(/tsNutsNutsNutsNu/g, "").replace(/tsNutsNutsNu/g, "").replace(/tsNutsNu/g, "").replace(/tsNu/g, "")), n && (e = Fi(e.trimStart())), e; }
    function is(t) { var e = "__trueosN", n = t, r = 0; for (; r < n.length;) {
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
    function $i(t) { return rs(t); }
    function Jn(t) { return Fi($i(t).trimStart()); }
    function os(t) { var e = ge(Jn(t)); return !(e.length === 0 || e === "true" || e === "false" || e === "N" || e === "Nu" || e === "Num" || e.startsWith("<truesurfer-") || e.startsWith("__trueo")); }
    function Ui(t, e) { var r; var n = Fe(e) || "block"; t[n] = ((r = t[n]) != null ? r : 0) + 1; }
    function ss(t) {
        var e_34, _a;
        var e = { nodes: 0, blocks: 0, text: 0, maxDepth: 0, tags: {} }, n = function (r, i) {
            var e_35, _a;
            if (e.nodes += 1, e.maxDepth = Math.max(e.maxDepth, i), r.kind === "text") {
                e.text += 1;
                return;
            }
            e.blocks += 1, Ui(e.tags, r.tagName);
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
    function as(t) { var e = { nodes: 0, blocks: 0, text: 0, maxDepth: 0, tags: {} }, n = function (r, i) {
        var e_36, _a;
        var o;
        e.nodes += 1, e.maxDepth = Math.max(e.maxDepth, i), r.kind === "text" ? e.text += 1 : (e.blocks += 1, Ui(e.tags, (o = r.tagName) != null ? o : "block"));
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
    function Sn(t, e) {
        if (e === void 0) { e = 64; }
        var n = ge($i(t)), r = "";
        for (var i = 0; i < n.length && r.length < e; i += 1) {
            var o = n.charAt(i);
            r += o === "|" || o === '"' || o === "\\" ? "_" : o;
        }
        return r;
    }
    function Tn(t, e) {
        if (e === void 0) { e = 120; }
        var n = "";
        for (var r = 0; r < t.length && n.length < e; r += 1) {
            var i = t.charAt(r);
            n += i === "\r" || i === "\n" || i === "	" || i === "|" || i === '"' || i === "\\" ? "_" : i;
        }
        return n;
    }
    function ls(t) { if (t.length <= 0 || t.length > 1e6 || t.indexOf("\0") >= 0)
        return !1; var e = t.slice(0, 256).trimStart().toLowerCase(); return e.startsWith("<!doctype") || e.startsWith("<html") || e.startsWith("<body") || e.startsWith("<"); }
    function Ri(t, e) {
        if (e === void 0) { e = 12; }
        var n = [], r = function (i, o, s) { if (n.length >= e)
            return; if (i.kind === "text") {
            n.push("#".concat(n.length, "@").concat(o, ":").concat(s, " chars=").concat(i.text.length, " sample=\"").concat(Sn(i.text), "\""));
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
            n.push("#".concat(n.length, "@").concat(o, ":").concat(s, " chars=").concat(m.length, " box=").concat(Math.round(i.x), ",").concat(Math.round(i.y), ",").concat(Math.round(i.width), ",").concat(Math.round(i.height), " sample=\"").concat(Sn(m), "\""));
            return;
        } var c = Fe(i.tagName || "block") || "block", a = i.key || ""; for (var m = 0; m < i.children.length; m += 1)
            r(i.children[m], c, a); };
        return r(t, "root", ""), n.join("|");
    }
    function Oi(t, e) {
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
                    var y = Sn(Pn(o), 36);
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
    function En(t) { return (typeof t == "string" ? t : "").replace(/&quot;/g, '"').replace(/&#34;/g, '"').replace(/&#39;/g, "'").replace(/&apos;/g, "'").replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&amp;/g, "&"); }
    function jn(t) { return ge(En((typeof t == "string" ? t : "").replace(/<[^>]*>/g, " "))); }
    function cs(t) { var e = 0, n = String(t != null ? t : ""); for (; e < n.length && n.charCodeAt(e) <= 32;)
        e += 1; for (n.charAt(e) === "/" && (e += 1); e < n.length && n.charCodeAt(e) <= 32;)
        e += 1; var r = e; for (; e < n.length;) {
        var i = n.charCodeAt(e);
        if (!(i >= 48 && i <= 57 || i >= 65 && i <= 90 || i >= 97 && i <= 122 || i === 45 || i === 58))
            break;
        e += 1;
    } return n.slice(r, e).toLowerCase(); }
    function us(t) { return t === "h1" || t === "h2" || t === "h3" || t === "h4" || t === "h5" || t === "h6" || t === "summary" || t === "p" || t === "button" || t === "label" || t === "legend" || t === "option"; }
    function Bi(t) { var e = typeof t == "string" ? t : "", n = [], r = function (m) { var d = jn(m); d.length !== 0 && (d.startsWith("<truesurfer-") || d.startsWith("__trueo") || n.push(d)); }, i = [], o = e.toLowerCase(), s = o.indexOf("<body"); if (s >= 0) {
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
        var d = En(h);
        if (d.length > 0) {
            for (var A = i.length - 1; A >= 0; A -= 1)
                if (i[A].wanted) {
                    i[A].text += " ".concat(d);
                    break;
                }
        }
        h = "";
        var b = e.indexOf(">", s + 1);
        if (b < 0)
            break;
        var y = e.slice(s, b + 1), w = e.slice(s + 1, b), u = cs(w);
        if (w.trimStart().charAt(0) === "/") {
            for (var A = i.length - 1; A >= 0; A -= 1) {
                var N = i.pop();
                if (N != null && N.wanted && r(N.text), (N == null ? void 0 : N.tag) === u)
                    break;
            }
            s = b + 1;
            continue;
        }
        if (u === "script" || u === "style" || u === "template") {
            var A = "</".concat(u, ">"), N = o.indexOf(A, b + 1);
            s = N >= 0 ? N + A.length : b + 1;
            continue;
        }
        if (u === "input") {
            var A = Ci(y, "type").toLowerCase();
            (A === "button" || A === "submit" || A === "reset") && r(Ci(y, "value"));
        }
        var p = y.length - 1;
        for (; p >= 0 && y.charCodeAt(p) <= 32;)
            p -= 1;
        p >= 1 && y.charAt(p) === ">" && y.charAt(p - 1) === "/" || u === "input" || u === "br" || u === "hr" || u === "img" || i.push({ tag: u, wanted: us(u), text: "" }), s = b + 1;
    } if (h.length > 0) {
        var m = En(h);
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
    function In(t) { var e = window == null ? void 0 : window[t]; return e !== void 0 ? e : globalThis == null ? void 0 : globalThis[t]; }
    function ds(t, e) {
        if (e === void 0) { e = 8; }
        var n = [];
        for (var r = 0; r < t.length && n.length < e; r += 1)
            n.push("#".concat(r, "=\"").concat(Tn(t[r], 48), "\""));
        return n.join("|");
    }
    function Ci(t, e) { var i, o, s; var r = new RegExp("".concat(e, "[ \\t\\r\\n\\f]*=[ \\t\\r\\n\\f]*(\"([^\"]*)\"|'([^']*)'|([^ \\t\\r\\n\\f>]+))"), "i").exec(t); return En((s = (o = (i = r == null ? void 0 : r[2]) != null ? i : r == null ? void 0 : r[3]) != null ? o : r == null ? void 0 : r[4]) != null ? s : ""); }
    function tn(t) { var e = []; for (var n = 0; n < t.length; n += 1) {
        var r = t[n];
        typeof r == "string" && Mn(e, r);
    } return e; }
    function hs(t) { var e = "", n = !1; for (var r = 0; r < t.length; r += 1) {
        var i = t.charCodeAt(r);
        if (i === 32 || i === 9 || i === 10 || i === 13 || i === 12) {
            n = !0;
            continue;
        }
        n && e.length > 0 && (e += " "), e += t.charAt(r), n = !1;
    } return e; }
    function Mn(t, e) { var n = hs(e); if (n.length !== 0 && !(n.indexOf("<truesurfer-") === 0 || n.indexOf("__trueo") === 0)) {
        for (var r = 0; r < t.length; r += 1)
            if (t[r] === n)
                return;
        t.push(n);
    } }
    function ms(t) {
        if (typeof t != "string" || t.length === 0)
            return [];
        var e = [], n = "";
        for (var r = 0; r < t.length; r += 1) {
            var i = t.charAt(r);
            if (i === "\r" || i === "\n") {
                Mn(e, n), n = "", i === "\r" && t.charAt(r + 1) === "\n" && (r += 1);
                continue;
            }
            n += i;
        }
        return Mn(e, n), e;
    }
    function fs(t) { var e = []; if (!Array.isArray(t))
        return e; for (var n = 0; n < t.length; n += 1) {
        var r = t[n];
        typeof r == "string" && Mn(e, r);
    } return e; }
    function ps(t) { var e = []; if (!Array.isArray(t))
        return e; for (var n = 0; n < t.length; n += 1) {
        var r = t[n];
        typeof r != "string" || r.length === 0 || r.indexOf("<truesurfer-") === 0 || r.indexOf("__trueo") === 0 || (e[e.length] = r);
    } return e; }
    function gs(t) {
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
    function bs(t) { var e = In("__TRUEOS_WIDGET_TEXT_ROWS_TEXT__"), n = In("__TRUEOS_WIDGET_TEXT_ROWS__"), r = ps(n); if (r.length > 0)
        return { source: "array-trusted", rows: r }; var i = gs(e); if (i.length > 0)
        return { source: "text-trusted", rows: i }; var o = ms(e); if (o.length > 0)
        return { source: "text", rows: o }; var s = fs(n); if (s.length > 0)
        return { source: "array", rows: s }; var c = Bi(t); if (Lt()) {
        var a = Array.isArray(n) && typeof n[0] == "string" ? Tn(n[0], 72) : "", h = typeof e == "string" ? Tn(e, 72) : "";
        console.log("[trueos pixi widgets] text-fallback-globals text_type=".concat(typeof e, " text_len=").concat(typeof e == "string" ? e.length : 0, " text_rows=").concat(o.length, " text_sample=\"").concat(h, "\" array=").concat(Array.isArray(n) ? n.length : -1, " array_rows=").concat(s.length, " array0=\"").concat(a, "\" html_len=").concat(t.length, " html_rows=").concat(c.length));
    } return { source: "html", rows: c }; }
    function _s() { var e; var t = In("__TRUEOS_WIDGET_RENDER_TREE_JSON__"); if (typeof t == "string" && t.length > 0)
        try {
            return { source: "json", tree: JSON.parse(t) };
        }
        catch (n) {
            Lt() && console.log("[trueos pixi widgets] render-tree-json parse failed err=".concat(String((e = n == null ? void 0 : n.message) != null ? e : n)));
        } return { source: "window", tree: In("__TRUEOS_WIDGET_RENDER_TREE__") }; }
    function ys(t) { var o, s, c, a; var e = [], n = String(t != null ? t : "").replace(/<script[^]*?<\/script>/gi, " ").replace(/<style[^]*?<\/style>/gi, " "), r = /<(h[1-6]|p|label|button)\b[^>]*>([^]*?)<\/\1>|<input\b[^>]*>/gi, i; for (; (i = r.exec(n)) && e.length < We;) {
        var h = (o = i[0]) != null ? o : "", m = String((s = i[1]) != null ? s : "").toLowerCase();
        if (h.toLowerCase().startsWith("<input"))
            continue;
        var d = jn(m === "p" || m === "label" ? (c = i[2]) != null ? c : "" : (a = i[2]) != null ? a : "");
        d.length > 0 && e.push(d);
    } return e; }
    function xs(t) { var e = ys(t), n = tn(e); return tn(n); }
    function ws(t, e, n, r) {
        var e_38, _a;
        var a, h, m, d, b, y;
        var i = tn((h = vi.get(String((a = t.key) != null ? a : ""))) != null ? h : []), o = tn(String((d = (m = t.attrs) == null ? void 0 : m["data-trueos-srcdoc-text"]) != null ? d : "").split("\n").map(function (w) { return ge(w); })), s = i.length > 0 ? i : o.length > 0 ? o : xs(String((y = (b = t.attrs) == null ? void 0 : b.srcdoc) != null ? y : "")), c = n + 48;
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
    function Pn(t) { var e; return t.kind === "text" ? (e = t.text) != null ? e : "" : t.children.map(Pn).join(" "); }
    function Xi(t) { var e; return t.kind === "text" ? (e = t.text) != null ? e : "" : t.children.map(Xi).join(" "); }
    function Ts(t) { var e = [], n = function (r, i, o, s) {
        var e_39, _a;
        var u, _, p;
        if (e.length >= We)
            return;
        var c = i + r.x, a = o + r.y, h = r.kind === "block" && r.tagName === "iframe" && String((_ = (u = r.attrs) == null ? void 0 : u["data-root"]) != null ? _ : "") !== "1", m = s + (h ? 1 : 0), d = r.kind === "block" && r.tagName === "button", b = r.kind === "text" ? (p = r.text) != null ? p : "" : d ? Pn(r) : "", y = ge(Jn(b)), w = e.length;
        if (os(y)) {
            var R = d ? c + 8 : c, A = d ? a + Math.max(0, Math.floor((r.height - ee.fontSize * 1.25) / 2)) : a;
            e.push({ x: R, y: A, text: y });
        }
        if (!d) {
            try {
                for (var _b = __values(r.children), _c = _b.next(); !_c.done; _c = _b.next()) {
                    var R = _c.value;
                    n(R, c, a, m);
                }
            }
            catch (e_39_1) { e_39 = { error: e_39_1 }; }
            finally {
                try {
                    if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
                }
                finally { if (e_39) throw e_39.error; }
            }
            h && e.length === w && ws(r, c, a, e);
        }
    }; return n(t, 0, 0, 0), e; }
    function Es(t, e) {
        if (e === void 0) { e = 8; }
        var n = [];
        for (var r = 0; r < t.length && n.length < e; r += 1) {
            var i = t[r];
            n.push("#".concat(n.length, " x=").concat(Math.round(i.x), " y=").concat(Math.round(i.y), " text=\"").concat(Sn(i.text), "\""));
        }
        return n.join("|");
    }
    function Is() {
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
        return { total: t.length, ops: zn(e, 24), unsupported: zn(n, 24) };
    }
    function Ni(t, e, n, r, i, o) {
        if (i === void 0) { i = ""; }
        if (o === void 0) { o = { hash: "", renderHash: "", layoutHash: "", bytes: 0 }; }
        if (!Lt())
            return;
        var s = Is();
        window.__TRUEOS_PIXI_BRIDGE_STATS__ = { renderNodes: t.nodes, renderBlocks: t.blocks, renderText: t.text, renderTags: zn(t.tags, 24), renderTextSamples: n, layoutBoxes: e.nodes, layoutBlocks: e.blocks, layoutText: e.text, layoutMaxDepth: e.maxDepth, layoutTextSamples: r, layoutWidgetSamples: i, prePixiHash: o.hash, prePixiRenderHash: o.renderHash, prePixiLayoutHash: o.layoutHash, prePixiTraceBytes: o.bytes, measureTextCalls: Vn, scrollbarVisible: l.scroll.track.h > 0 ? 1 : 0, scrollbarTrack: "".concat(Math.round(l.scroll.track.x), ",").concat(Math.round(l.scroll.track.y), ",").concat(Math.round(l.scroll.track.w), ",").concat(Math.round(l.scroll.track.h)), scrollbarThumb: "".concat(Math.round(l.scroll.thumb.x), ",").concat(Math.round(l.scroll.thumb.y), ",").concat(Math.round(l.scroll.thumb.w), ",").concat(Math.round(l.scroll.thumb.h)), pixiCommands: s.total, pixiOps: s.ops, pixiUnsupported: s.unsupported };
    }
    var Ai = new WeakMap;
    function Qn(t, e) { var n = t; for (; n;) {
        if (n === e)
            return !0;
        n = n.parent;
    } return !1; }
    function Yi(t) { return Array.isArray(t.children) || (t.children = []), t.children; }
    function qt(t, e, n) { var r = Number(e) || 0, i = Number(n) || 0; (!t.position || typeof t.position != "object") && (t.position = { x: 0, y: 0 }), t.position.x = r, t.position.y = i; }
    function Ze(t, e, n) { if (e === t || Qn(t, e))
        return; var r = Yi(t); if (e.parent !== t) {
        var c = Math.max(0, Math.min(n, r.length));
        t.addChildAt(e, c);
        return;
    } var i = Math.max(0, r.length - 1), o = Math.max(0, Math.min(n, i)); t.getChildIndex(e) !== o && t.setChildIndex(e, o); }
    function Li(t, e, n) { if (e === t || Qn(t, e))
        return; var r = Yi(t); if (e.parent !== t) {
        var c = Math.max(0, Math.min(n, r.length));
        t.addChildAt(e, c);
        return;
    } var i = Math.max(0, r.length - 1), o = Math.max(0, Math.min(n, i)); t.getChildIndex(e) !== o && t.setChildIndex(e, o); }
    var xn = null, wt = null, Xt = null;
    function te(t) { var e = l.cursorColors.get(t); if (e != null)
        return e; var n = [1118481, 2450411, 1483594, 14427686, 8141549, 959977, 16096779], r = Math.abs(Number(t) || 0) % n.length, i = n[r]; return l.cursorColors.set(t, i), i; }
    function Bt(t) { var i, o, s, c, a, h; var e = Number((s = (o = t == null ? void 0 : t.pointerId) != null ? o : (i = t == null ? void 0 : t.data) == null ? void 0 : i.pointerId) != null ? s : 0), r = String((h = (a = t == null ? void 0 : t.pointerType) != null ? a : (c = t == null ? void 0 : t.data) == null ? void 0 : c.pointerType) != null ? h : "").toLowerCase() === "mouse" || e === 1 || e === l.primaryMousePointerId; return l.harness.enabled && r ? l.harness.activeUserPointerId : e; }
    function Lt() { return !!globalThis.__TRUEOS_CAPTURE_ONLY__; }
    function ke(t, e) { if (!t)
        return e; if (!e)
        return t; var n = Math.min(t.x, e.x), r = Math.min(t.y, e.y), i = Math.max(t.x + t.w, e.x + e.w), o = Math.max(t.y + t.h, e.y + e.h); return { x: n, y: r, w: Math.max(0, i - n), h: Math.max(0, o - r) }; }
    function Ms(t, e) { return e ? t ? e.x >= t.x && e.y >= t.y && e.x + e.w <= t.x + t.w && e.y + e.h <= t.y + t.h : !1 : !0; }
    function Ss(t, e, n) { return { x: t.x + e, y: t.y + n, w: t.w, h: t.h }; }
    function Ps(t) {
        var e_41, _a;
        var o, s, c, a, h;
        var e = null, n = t == null ? void 0 : t.hitArea;
        n && Number.isFinite(Number(n.x)) && Number.isFinite(Number(n.y)) && Number((o = n.width) != null ? o : n.w) > 0 && Number((s = n.height) != null ? s : n.h) > 0 && (e = ke(e, { x: Number(n.x) || 0, y: Number(n.y) || 0, w: Number((c = n.width) != null ? c : n.w) || 0, h: Number((a = n.height) != null ? a : n.h) || 0 }));
        var r = Array.isArray(t == null ? void 0 : t.commands) ? t.commands : [];
        try {
            for (var r_1 = __values(r), r_1_1 = r_1.next(); !r_1_1.done; r_1_1 = r_1.next()) {
                var m = r_1_1.value;
                if (!Array.isArray(m))
                    continue;
                var d = String((h = m[0]) != null ? h : "");
                if (d === "rect" || d === "roundRect") {
                    var b = { x: Number(m[1]) || 0, y: Number(m[2]) || 0, w: Math.max(0, Number(m[3]) || 0), h: Math.max(0, Number(m[4]) || 0) };
                    e = ke(e, b);
                }
                else if (d === "circle") {
                    var b = Number(m[1]) || 0, y = Number(m[2]) || 0, w = Math.max(0, Number(m[3]) || 0);
                    e = ke(e, { x: b - w, y: y - w, w: w * 2, h: w * 2 });
                }
                else if (d === "ellipse") {
                    var b = Number(m[1]) || 0, y = Number(m[2]) || 0, w = Math.max(0, Number(m[3]) || 0), u = Math.max(0, Number(m[4]) || 0);
                    e = ke(e, { x: b - w, y: y - u, w: w * 2, h: u * 2 });
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
            var m = Math.max(1, Number(t == null ? void 0 : t.width) || i.length * ee.fontSize * .7), d = Math.max(1, Number(t == null ? void 0 : t.height) || ee.fontSize * 1.25);
            e = ke(e, { x: 0, y: 0, w: m, h: d });
        }
        return e;
    }
    function Di(t) { var e = function (i, o, s) {
        var e_42, _a;
        var d, b, y, w;
        var c = o + (Number((b = (d = i == null ? void 0 : i.position) == null ? void 0 : d.x) != null ? b : i == null ? void 0 : i.x) || 0), a = s + (Number((w = (y = i == null ? void 0 : i.position) == null ? void 0 : y.y) != null ? w : i == null ? void 0 : i.y) || 0), h = Ps(i);
        h && (h = Ss(h, c, a));
        var m = Array.isArray(i == null ? void 0 : i.children) ? i.children : [];
        try {
            for (var m_1 = __values(m), m_1_1 = m_1.next(); !m_1_1.done; m_1_1 = m_1.next()) {
                var u = m_1_1.value;
                h = ke(h, e(u, c, a));
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
    function Nt(t) { var i; if (!Lt() || (window.__TRUEOS_PIXI_APP_PHASE__ = t, !{ "main:start": !0, "main:yoga": !0, "main:create-app": !0, "main:attach-capture": !0, "main:append-canvas": !0, "main:capture-flags": !0, "main:canvas-listeners": !0, "main:stage:done": !0, "main:roots": !0, "main:text-measure": !0, "main:html": !0, "main:render-tree": !0, "main:first-rerender": !0, "main:layout-build": !0, "main:layout-commit": !0, "main:paint:clamp": !0, "main:paint:render-to-pixi": !0, "main:paint:scrollbar": !0, "main:paint:renderer-render": !0, "main:paint:done": !0, "main:cursor-setup": !0, "main:input-listeners": !0, "main:done": !0 }[t]))
        return; var n = window, r = (i = n.__TRUEOS_PIXI_PHASE_TRACE_SEEN__) != null ? i : n.__TRUEOS_PIXI_PHASE_TRACE_SEEN__ = {}; r[t] || (r[t] = 1, console.log("[Trace] [pixi] phase=".concat(t))); }
    function B(t) { Lt() && (window.__TRUEOS_PIXI_LAYOUT_STEP__ = t); }
    function Dt(t) { Lt() && console.log(t); }
    function he(t, e, n) { var o; if (!Lt())
        return; var r = "__TRUEOS_".concat(t, "_LOG_COUNT__"), i = Number((o = window[r]) != null ? o : 0) || 0; i >= e || (window[r] = i + 1, console.log(n)); }
    function Ki(t) { var c, a, h, m, d; var e = (c = window.__TRUEOS_PIXI_APP_PHASE__) != null ? c : "unknown", n = (a = window.__TRUEOS_PIXI_LAYOUT_STEP__) != null ? a : "", r = t, i = String((h = r == null ? void 0 : r.name) != null ? h : "Error"), o = String((m = r == null ? void 0 : r.message) != null ? m : t), s = String((d = r == null ? void 0 : r.stack) != null ? d : ""); return "phase=".concat(e, " layout=").concat(n, " name=").concat(i, " message=").concat(o, " stack=").concat(s); }
    function Rs() { var t = Math.max(1, Number(window.innerWidth || 1920) | 0), e = Math.max(1, Number(window.innerHeight || 1080) | 0), n = new Ot(0, 0, t, e), r = document.createElement("canvas"), i = { width: t, height: e, screen: n, render: function (o) { return o; }, resize: function (o, s) { var c = Math.max(1, Number(o || t) | 0), a = Math.max(1, Number(s || e) | 0); this.width = c, this.height = a, n.width = c, n.height = a; } }; return { stage: new Ct, screen: n, canvas: r, renderer: i, ticker: { stop: function () { }, add: function () { }, remove: function () { } } }; }
    function ks() { var p = 0, R = 0, A = 2e4; return { Node: { create: function () { return ({ children: [], measureFunc: null, paddingLeft: 0, paddingTop: 0, paddingRight: 0, paddingBottom: 0, marginLeft: 0, marginTop: 0, marginRight: 0, marginBottom: 0, width: 0, height: 0, minWidth: 0, minHeight: 0, flexDirection: 0, alignItems: 0, justifyContent: 1, flexWrap: 0, positionType: 0, positionLeft: null, positionTop: null, positionRight: null, positionBottom: null, computed: { left: 0, top: 0, width: 0, height: 0 }, debugLabel: "node", setMeasureFunc: function (x) { this.measureFunc = x; }, setMargin: function (x, C) { var H = Number(C) || 0; x === 0 ? this.marginLeft = H : x === 1 ? this.marginTop = H : x === 2 ? this.marginRight = H : x === 3 && (this.marginBottom = H); }, setPadding: function (x, C) { var H = Number(C) || 0; x === 0 ? this.paddingLeft = H : x === 1 ? this.paddingTop = H : x === 2 ? this.paddingRight = H : x === 3 && (this.paddingBottom = H); }, setFlexDirection: function (x) { this.flexDirection = x; }, setAlignItems: function (x) { this.alignItems = Number(x) || 0; }, setJustifyContent: function (x) { this.justifyContent = Number(x) || 0; }, setFlexWrap: function (x) { this.flexWrap = Number(x) === 1 ? 1 : 0; }, setFlexGrow: function (x) { }, setFlexShrink: function (x) { }, setAlignSelf: function (x) { }, setPositionType: function (x) { this.positionType = Number(x) === 1 ? 1 : 0; }, setPosition: function (x, C) { var H = Number(C) || 0; x === 0 ? this.positionLeft = H : x === 1 ? this.positionTop = H : x === 2 ? this.positionRight = H : x === 3 && (this.positionBottom = H); }, setWidth: function (x) { this.width = Math.max(0, Number(x) || 0); }, setHeight: function (x) { this.height = Math.max(0, Number(x) || 0); }, setMinWidth: function (x) { this.minWidth = Math.max(0, Number(x) || 0); }, setMinHeight: function (x) { this.minHeight = Math.max(0, Number(x) || 0); }, insertChild: function (x, C) { this.children.splice(Math.max(0, Math.min(C, this.children.length)), 0, x); }, getChildCount: function () { return this.children.length; }, getComputedLeft: function () { return this.computed.left; }, getComputedTop: function () { return this.computed.top; }, getComputedWidth: function () { return this.computed.width; }, getComputedHeight: function () { return this.computed.height; }, freeRecursive: function () { }, calculateLayout: function (x, C) {
                    if (x === void 0) { x = this.width; }
                    if (C === void 0) { C = this.height; }
                    this.layout(0, 0, Math.max(1, Number(x) || this.width || 1), Math.max(1, Number(C) || this.height || 1));
                }, layout: function (x, C, H, k) {
                    var e_43, _a, e_44, _b, e_45, _c;
                    var G, W, tt, it;
                    if (p += 1, (p <= 80 || p % 500 === 0) && (R += 1, R <= 140 && Dt("[trueos pixi widgets] yoga-layout-call #".concat(p, " label=\"").concat(this.debugLabel, "\" children=").concat(this.children.length, " flex=").concat(this.flexDirection, " pos=").concat(this.positionType, " xy=").concat(Math.round(x), ",").concat(Math.round(C), " avail=").concat(Math.round(H), "x").concat(Math.round(k), " own=").concat(Math.round(this.width), "x").concat(Math.round(this.height), " min=").concat(Math.round(this.minWidth), "x").concat(Math.round(this.minHeight)))), p > A)
                        throw new Error("capture yoga layout budget exceeded count=".concat(p, " label=\"").concat(this.debugLabel, "\" children=").concat(this.children.length, " flex=").concat(this.flexDirection, " pos=").concat(this.positionType, " avail=").concat(Math.round(H), "x").concat(Math.round(k)));
                    var S = this.paddingLeft + this.paddingRight, X = this.paddingTop + this.paddingBottom, j = Math.max(this.minWidth, this.width || H), f = Math.max(this.minHeight, this.height || 0);
                    if (this.computed.left = x, this.computed.top = C, this.computed.width = j, this.measureFunc) {
                        var st = this.measureFunc(Math.max(0, j - S), 0);
                        this.width <= 0 && this.minWidth <= 0 && (this.computed.width = Math.ceil(Math.max(0, Number(st.width) || 0)) + S), f = Math.max(f, Math.ceil(Number(st.height) || 0) + X), this.computed.height = f;
                        return;
                    }
                    if (this.flexDirection === 1) {
                        var st = this.paddingLeft, z = 0, nt = Math.max(1, this.children.length);
                        try {
                            for (var _d = __values(this.children), _f = _d.next(); !_f.done; _f = _d.next()) {
                                var L = _f.value;
                                if (L.positionType === 1)
                                    continue;
                                var V = L.width || L.minWidth || Math.max(24, (j - S) / nt);
                                L.layout(st + L.marginLeft, this.paddingTop + L.marginTop, V, k), st += L.computed.width + L.marginLeft + L.marginRight, z = Math.max(z, L.computed.height + L.marginTop + L.marginBottom);
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
                                var L = _h.value;
                                if (L.positionType === 1) {
                                    var V = L.width || L.minWidth || Math.max(0, j - S - L.marginLeft - L.marginRight), K = L.height || L.minHeight || k, U = L.positionLeft != null ? this.paddingLeft + L.positionLeft : Math.max(0, j - this.paddingRight - ((G = L.positionRight) != null ? G : 0) - V), gt = L.positionTop != null ? this.paddingTop + L.positionTop : Math.max(0, f - this.paddingBottom - ((W = L.positionBottom) != null ? W : 0) - K);
                                    L.layout(U + L.marginLeft, gt + L.marginTop, V, K);
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
                        f = Math.max(f, z + X);
                    }
                    else {
                        var st = this.paddingTop;
                        try {
                            for (var _j = __values(this.children), _k = _j.next(); !_k.done; _k = _j.next()) {
                                var z = _k.value;
                                if (z.positionType === 1) {
                                    var L = z.width || z.minWidth || Math.max(0, j - S - z.marginLeft - z.marginRight), V = z.height || z.minHeight || k, K = z.positionLeft != null ? this.paddingLeft + z.positionLeft : Math.max(0, j - this.paddingRight - ((tt = z.positionRight) != null ? tt : 0) - L), U = z.positionTop != null ? this.paddingTop + z.positionTop : Math.max(0, f - this.paddingBottom - ((it = z.positionBottom) != null ? it : 0) - V);
                                    z.layout(K + z.marginLeft, U + z.marginTop, L, V);
                                    continue;
                                }
                                var nt = Math.max(0, j - S - z.marginLeft - z.marginRight);
                                z.layout(this.paddingLeft + z.marginLeft, st + z.marginTop, nt, k), st += z.computed.height + z.marginTop + z.marginBottom;
                            }
                        }
                        catch (e_45_1) { e_45 = { error: e_45_1 }; }
                        finally {
                            try {
                                if (_k && !_k.done && (_c = _j.return)) _c.call(_j);
                            }
                            finally { if (e_45) throw e_45.error; }
                        }
                        f = Math.max(f, st + this.paddingBottom);
                    }
                    this.computed.height = Math.max(this.minHeight, f);
                } }); } }, EDGE_LEFT: 0, EDGE_TOP: 1, EDGE_RIGHT: 2, EDGE_BOTTOM: 3, FLEX_DIRECTION_COLUMN: 0, FLEX_DIRECTION_ROW: 1, FLEX_DIRECTION_ROW_REVERSE: 1, ALIGN_STRETCH: 0, ALIGN_CENTER: 1, ALIGN_FLEX_START: 2, JUSTIFY_CENTER: 0, JUSTIFY_FLEX_START: 1, JUSTIFY_SPACE_BETWEEN: 2, WRAP_WRAP: 1, WRAP_NO_WRAP: 0, POSITION_TYPE_RELATIVE: 0, POSITION_TYPE_ABSOLUTE: 1, DIRECTION_LTR: 0, MEASURE_MODE_UNDEFINED: 0 }; }
    function Os(t) {
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
    function qe(t, e) { var o, s, c, a; var n = l.inputs.get(t); if (n)
        return n; var r = {}, i = ((o = e == null ? void 0 : e.type) != null ? o : "text").toLowerCase(); if (i === "checkbox" || i === "radio") {
        if (r.checked = e ? Object.prototype.hasOwnProperty.call(e, "checked") : !1, i === "checkbox") {
            var h = ((s = e == null ? void 0 : e["aria-checked"]) != null ? s : "").toLowerCase(), m = ((c = e == null ? void 0 : e["data-indeterminate"]) != null ? c : "").toLowerCase();
            r.indeterminate = (e ? Object.prototype.hasOwnProperty.call(e, "indeterminate") : !1) || h === "mixed" || m === "true" || m === "1" || m === "yes";
        }
    }
    else
        r.value = (a = e == null ? void 0 : e.value) != null ? a : ""; return l.inputs.set(t, r), r; }
    function Cs(t) { var e = new Map; function n(r) {
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
    function Ns(t) {
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
    function zi(t, e, n) { var h, m; if (!t || typeof t != "object")
        return null; var r = t, i = typeof r.kind == "string" ? r.kind : ""; if (i === "text") {
        var d = typeof r.text == "string" ? r.text : "", b = "", y = (h = n == null ? void 0 : n.rows[n.index]) != null ? h : "", w = !1;
        if (n && n.index < n.rows.length ? (n.index += 1, b = y, w = !0) : b = ge(Jn(d)), !w && (d.indexOf("<truesurfer-") >= 0 || d.indexOf("__trueo") >= 0) || b.startsWith("<truesurfer-") || b.startsWith("__trueo"))
            b = "";
        else if (b.length === 0) {
            var _ = (m = n == null ? void 0 : n.rows[n.index]) != null ? m : "";
            n && _ && (n.index += 1), _ && (b = _);
        }
        return b.length > 0 ? { kind: "text", text: b } : null;
    } if (i !== "block")
        return null; var o = typeof r.tagName == "string" ? r.tagName.toLowerCase() : ""; if (o.length === 0)
        return null; var s = typeof r.key == "string" ? r.key : "".concat(e, ":").concat(o), c = [], a = Array.isArray(r.children) ? r.children : []; for (var d = 0; d < a.length; d += 1) {
        var b = zi(a[d], "".concat(e, ".").concat(d), n);
        b && c.push(b);
    } return { kind: "block", key: s, tagName: o, attrs: Ns(r.attrs), children: c }; }
    function As(t, e) { var n = Array.isArray(t) ? t : t && typeof t == "object" && Array.isArray(t.widgetRenderTree) ? t.widgetRenderTree : [], i = { rows: Array.isArray(e) ? tn(e) : Bi(e), index: 0 }, o = []; for (var s = 0; s < n.length; s += 1) {
        var c = zi(n[s], "0.".concat(s), i);
        c && o.push(c);
    } return o; }
    function Ls(t, e) { if (!Array.isArray(e) || e.length === 0)
        return 0; var n = 0, r = 0, i = function (o) { if (o.kind === "text") {
        if (n < e.length) {
            var s = e[n];
            n += 1, typeof s == "string" && s.length > 0 && s.indexOf("<truesurfer-") !== 0 && s.indexOf("__trueo") !== 0 && (o.text = s, r += 1);
        }
        return;
    } for (var s = 0; s < o.children.length; s += 1)
        i(o.children[s]); }; for (var o = 0; o < t.length; o += 1)
        i(t[o]); return r; }
    function Ds(t) { var n = document.createElement("canvas").getContext("2d"); if (!n)
        throw new Error("2D canvas not available"); n.font = t; var r = t.indexOf("px"), i = r; for (; i > 0;) {
        var c = t.charCodeAt(i - 1);
        if (c < 48 || c > 57)
            break;
        i -= 1;
    } var o = r > i ? Number(t.slice(i, r)) : 16, s = Math.ceil(o * 1.25); return { measure: function (c, a) {
            var e_50, _a;
            Vn += 1;
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
    function vs(t, e, n) { var w; B("build:start nodes=".concat(t.length, " viewport=").concat(e, "x").concat(n)), window.__TRUEOS_PIXI_LAYOUT_BUILD_COUNT__ = 0, Dt("[trueos pixi widgets] layout-build begin nodes=".concat(t.length, " viewport=").concat(e, "x").concat(n)); var r = 12, i = 8, o = ee; B("build:measurer"); var s = Ds("".concat(o.fontSize, "px ").concat(o.fontFamily)); function c(u) { return u.kind !== "block" || u.tagName === "hr" || u.tagName === "tr" || u.tagName === "td" || u.tagName === "th" ? 0 : i; } var a = 0; function h(u) { if (a += 1, (a <= 140 || a % 250 === 0) && Dt("[trueos pixi widgets] layout-box-build #".concat(a, " label=\"").concat(u, "\"")), a > 5e3)
        throw new Error("layout box build budget exceeded count=".concat(a, " label=\"").concat(u, "\"")); } function m(u) { var D; var _ = u.kind === "text" ? "text:".concat(u.text.slice(0, 24)) : "".concat(u.tagName, ":").concat(u.key); if (B("node:".concat(_, ":start")), u.kind === "text") {
        var x_1 = pt.Node.create();
        return x_1.debugLabel = _, B("node:".concat(_, ":measure-func")), x_1.setMeasureFunc(function (C, H) { B("node:".concat(_, ":measure-call")); var k = H === pt.MEASURE_MODE_UNDEFINED ? void 0 : Math.max(0, C), S = s.measure(u.text, k); return { width: S.width, height: S.height }; }), x_1.setMargin(pt.EDGE_RIGHT, 6), x_1.setMargin(pt.EDGE_BOTTOM, 0), { yogaNode: x_1, buildBox: function () { return (h(_), { kind: "text", text: u.text, x: x_1.getComputedLeft(), y: x_1.getComputedTop(), width: x_1.getComputedWidth(), height: x_1.getComputedHeight(), children: [] }); } };
    } if (u.tagName === "sliderlabel")
        return B("node:".concat(u.tagName, ":").concat(u.key, ":sliderlabel")), gr({ node: u, Yoga: pt, measurer: s }); B("node:".concat(u.tagName, ":").concat(u.key, ":create")); var p = pt.Node.create(); if (p.debugLabel = _, B("node:".concat(u.tagName, ":").concat(u.key, ":base-defaults")), p.setFlexDirection(pt.FLEX_DIRECTION_COLUMN), p.setAlignItems(pt.ALIGN_STRETCH), p.setPadding(pt.EDGE_LEFT, r), p.setPadding(pt.EDGE_RIGHT, r), p.setPadding(pt.EDGE_TOP, r), p.setPadding(pt.EDGE_BOTTOM, r), p.setMargin(pt.EDGE_BOTTOM, 0), Dn(u.tagName) && (B("node:".concat(u.tagName, ":").concat(u.key, ":heading-defaults")), Cr(p, pt)), u.tagName === "hr" && (B("node:".concat(u.tagName, ":").concat(u.key, ":hr-defaults")), Er(p, pt)), (u.tagName === "p" || u.tagName === "label") && (B("node:".concat(u.tagName, ":").concat(u.key, ":inline-scan")), u.children.some(function (C) { return C.kind === "block" && (C.tagName === "input" || C.tagName === "button" || C.tagName === "select" || C.tagName === "textarea" || C.tagName === "timeinput" || C.tagName === "dateinput" || C.tagName === "monthinput" || C.tagName === "weekinput" || C.tagName === "datetimelocalinput" || C.tagName === "progress" || C.tagName === "meter" || C.tagName === "slider" || C.tagName === "number" || C.tagName === "color"); }) && (p.setFlexDirection(pt.FLEX_DIRECTION_ROW), p.setFlexWrap(pt.WRAP_WRAP), p.setAlignItems(pt.ALIGN_CENTER)), p.setPadding(pt.EDGE_TOP, 4), p.setPadding(pt.EDGE_BOTTOM, 4), p.setPadding(pt.EDGE_LEFT, 4), p.setPadding(pt.EDGE_RIGHT, 4)), u.tagName === "table" && (B("node:".concat(u.tagName, ":").concat(u.key, ":table-defaults")), Rr(p, pt)), u.tagName === "tr" && (B("node:".concat(u.tagName, ":").concat(u.key, ":tr-defaults")), kr(p, pt)), (u.tagName === "td" || u.tagName === "th") && (B("node:".concat(u.tagName, ":").concat(u.key, ":cell-defaults")), Or(p, pt)), u.tagName === "input" && (B("node:".concat(u.tagName, ":").concat(u.key, ":input-defaults")), Zr(p, u, pt)), u.tagName === "textarea" && (B("node:".concat(u.tagName, ":").concat(u.key, ":textarea-defaults")), ti(p, pt)), u.tagName === "select" && (B("node:".concat(u.tagName, ":").concat(u.key, ":select-defaults")), fi(p, pt)), u.tagName === "timeinput" || u.tagName === "dateinput" || u.tagName === "monthinput" || u.tagName === "weekinput" || u.tagName === "datetimelocalinput") {
        var x = u.tagName === "timeinput" ? "time" : u.tagName === "monthinput" ? "month" : u.tagName === "weekinput" ? "week" : u.tagName === "dateinput" ? "date" : "datetime-local";
        B("node:".concat(u.tagName, ":").concat(u.key, ":temporal-defaults")), gi(p, pt, x);
    } if (u.tagName === "img" && (B("node:".concat(u.tagName, ":").concat(u.key, ":img-defaults")), Fr(p, u, pt)), u.tagName === "svg" && (B("node:".concat(u.tagName, ":").concat(u.key, ":svg-defaults")), Kr(p, u, pt)), u.tagName === "canvas" && (B("node:".concat(u.tagName, ":").concat(u.key, ":canvas-defaults")), jr(p, u, pt)), u.tagName === "iframe" && (B("node:".concat(u.tagName, ":").concat(u.key, ":iframe-defaults")), Jr(p, u, pt)), u.tagName === "button") {
        B("node:".concat(u.tagName, ":").concat(u.key, ":button-defaults")), Mr(p, pt);
        var x = ge(Xi(u));
        if (x.length > 0) {
            var C = s.measure(x);
            p.setMinWidth(Math.max(100, Math.ceil(C.width) + 28));
        }
    } u.tagName === "dialog" && (B("node:".concat(u.tagName, ":").concat(u.key, ":dialog-defaults")), si(p, pt)), u.tagName === "number" && (B("node:".concat(u.tagName, ":").concat(u.key, ":number-defaults")), li(p, pt)), u.tagName === "color" && (B("node:".concat(u.tagName, ":").concat(u.key, ":color-defaults")), di(p, u, pt)), u.tagName === "searchrow" && (B("node:".concat(u.tagName, ":").concat(u.key, ":searchrow-defaults")), ri(p, pt)), u.tagName === "searchbutton" && (B("node:".concat(u.tagName, ":").concat(u.key, ":searchbutton-defaults")), ii(p, pt)), u.tagName === "summary" && (B("node:".concat(u.tagName, ":").concat(u.key, ":summary-defaults")), yr(p, pt)), u.tagName === "details" && (B("node:".concat(u.tagName, ":").concat(u.key, ":details-defaults")), xr(p, pt)), u.tagName === "barrow" && (B("node:".concat(u.tagName, ":").concat(u.key, ":barrow-defaults")), ni(p, pt)), (u.tagName === "progress" || u.tagName === "meter") && (B("node:".concat(u.tagName, ":").concat(u.key, ":progress-defaults")), fr(p, pt)), u.tagName === "slider" && (B("node:".concat(u.tagName, ":").concat(u.key, ":slider-defaults")), pr(p, pt)), B("node:".concat(u.tagName, ":").concat(u.key, ":children-effective")); var R = wr(u, l.detailsOpen), A = Number((D = window.__TRUEOS_PIXI_LAYOUT_BUILD_COUNT__) != null ? D : 0) + 1; window.__TRUEOS_PIXI_LAYOUT_BUILD_COUNT__ = A, (A <= 120 || A % 50 === 0) && Dt("[trueos pixi widgets] layout-build-node #".concat(A, " label=\"").concat(_, "\" children=").concat(u.children.length, " effective=").concat(R.length)), B("node:".concat(u.tagName, ":").concat(u.key, ":children-map count=").concat(R.length)); var N = R.map(m); (A <= 120 || A % 50 === 0) && Dt("[trueos pixi widgets] layout-build-node-mapped #".concat(A, " label=\"").concat(_, "\" pairs=").concat(N.length)), B("node:".concat(u.tagName, ":").concat(u.key, ":children-insert")); for (var x = 0; x < N.length; x++) {
        var C = R[x], H = N[x];
        if (C && C.kind === "block") {
            var k = x === N.length - 1 ? 0 : c(C);
            H.yogaNode.setMargin(pt.EDGE_BOTTOM, k);
        }
        p.insertChild(H.yogaNode, p.getChildCount());
    } return { yogaNode: p, buildBox: function () { return (h(_), { kind: "block", key: u.key, tagName: u.tagName, attrs: u.attrs, x: p.getComputedLeft(), y: p.getComputedTop(), width: p.getComputedWidth(), height: p.getComputedHeight(), children: N.map(function (x) { return x.buildBox(); }) }); } }; } var d = pt.Node.create(); d.debugLabel = "root", B("root:flex-direction"), d.setFlexDirection(pt.FLEX_DIRECTION_COLUMN), B("root:align-items"), d.setAlignItems(pt.ALIGN_STRETCH), B("root:width"), d.setWidth(e), B("root:height"), d.setHeight(n), B("root:padding-left"), d.setPadding(pt.EDGE_LEFT, 16), B("root:padding-top"), d.setPadding(pt.EDGE_TOP, 16), B("root:padding-right"), d.setPadding(pt.EDGE_RIGHT, 16 + wn), B("root:padding-bottom"), d.setPadding(pt.EDGE_BOTTOM, 16), B("root:children-map count=".concat(t.length)), Dt("[trueos pixi widgets] layout-root children-map count=".concat(t.length)); var b = t.map(m); B("root:children-insert"), Dt("[trueos pixi widgets] layout-root children-insert pairs=".concat(b.length)); for (var u = 0; u < b.length; u++) {
        var _ = t[u], p = b[u];
        if (_ && _.kind === "block") {
            var R = u === b.length - 1 ? 0 : c(_);
            p.yogaNode.setMargin(pt.EDGE_BOTTOM, R);
        }
        d.insertChild(p.yogaNode, d.getChildCount());
    } B("root:calculate"), Dt("[trueos pixi widgets] layout-root calculate begin"), d.calculateLayout(e, n, pt.DIRECTION_LTR), Dt("[trueos pixi widgets] layout-root calculate done"), B("root:build-box"), Dt("[trueos pixi widgets] layout-root build-box begin"), h("root"); var y = { kind: "block", tagName: "root", x: 0, y: 0, width: d.getComputedWidth(), height: d.getComputedHeight(), children: b.map(function (u) { return u.buildBox(); }) }; return Dt("[trueos pixi widgets] layout-root build-box done boxes=".concat(a)), B("root:free"), (w = d.freeRecursive) == null || w.call(d), B("build:done"), y; }
    function Gs(t, e, n) {
        var e_51, _a, e_52, _b, e_53, _c, e_54, _d, e_55, _f;
        var X, j;
        B("render:start");
        var r = ee, i = n != null ? n : t.stage;
        B("render:get-background");
        var o = $t(i, "__background");
        B("render:get-content-root");
        var s = ue(i, "__contentRoot");
        B("render:get-dialog-root");
        var c = ue(i, "__dialogRoot");
        B("render:get-overlay-root");
        var a = ue(i, "__overlayRoot");
        B("render:ensure-background"), Li(i, o, 0), B("render:ensure-content-root"), Ze(i, s, 1), B("render:ensure-dialog-root"), Ze(i, c, 2), B("render:ensure-overlay-root"), Ze(i, a, 3), B("render:overlay-remove-children"), a.removeChildren(), B("render:overlay-removed");
        var h = [], m = [], d = Cs(e);
        B("render:clear-ui-state"), l.fieldBounds.clear(), l.sliderBounds.clear(), l.dialogDragBounds.clear(), l.hoverRects.length = 0, l.hoverHandlers.clear(), l.iframeRects.length = 0, l.iframeScrollRoots.clear(), l.iframeScrollbarGraphics.clear(), B("render:node-cache");
        var b = (X = Ai.get(i)) != null ? X : new Map;
        Ai.set(i, b);
        var y = new Set, w = function (f) {
            var e_56, _a;
            var tt;
            var G = 0, W = function (it, st, z) {
                var e_57, _a;
                var V;
                if (it.kind === "block" && it.tagName === "dialog")
                    return;
                var nt = st + it.x, L = z + it.y;
                G = Math.max(G, L + it.height);
                try {
                    for (var _b = __values((V = it.children) != null ? V : []), _c = _b.next(); !_c.done; _c = _b.next()) {
                        var K = _c.value;
                        W(K, nt, L);
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
                for (var _b = __values((tt = f.children) != null ? tt : []), _c = _b.next(); !_c.done; _c = _b.next()) {
                    var it = _c.value;
                    W(it, 0, 0);
                }
            }
            catch (e_56_1) { e_56 = { error: e_56_1 }; }
            finally {
                try {
                    if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
                }
                finally { if (e_56) throw e_56.error; }
            }
            return G;
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
        var _ = ts(r);
        function p(f, G, W) { return Math.max(G, Math.min(W, f)); }
        var R = function (f) {
            var e_58, _a;
            try {
                for (var _b = __values(l.textDrags.entries()), _c = _b.next(); !_c.done; _c = _b.next()) {
                    var _d = __read(_c.value, 2), G = _d[0], W = _d[1];
                    if (W.key === f)
                        return G;
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
        }, A = function (f) {
            var e_59, _a;
            var G = l.keyboardOwnerPointerId;
            if (l.focusedKeyByPointer.get(G) === f)
                return G;
            try {
                for (var _b = __values(l.focusedKeyByPointer.entries()), _c = _b.next(); !_c.done; _c = _b.next()) {
                    var _d = __read(_c.value, 2), W = _d[0], tt = _d[1];
                    if (tt === f)
                        return W;
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
        B("render:background-clear"), Gt(o), B("render:background-rect"), o.rect(0, 0, t.renderer.width, t.renderer.height), B("render:background-fill"), o.fill(r.background), B("render:content-position");
        {
            var f = l.scroll, G = f && Number(f.y || 0) || 0, W = s.position;
            W && (W.x = 0, W.y = -G);
        }
        B("render:content-position-done");
        function N(f, G, W, tt, it, st, z, nt, L) {
            var e_60, _a;
            if (tt === void 0) { tt = 0; }
            if (it === void 0) { it = 0; }
            var ft, q, ot, bt, g, T, v, M, E, P, I, O, J, Q, Y, lt, Z, _t, Mt, xt;
            B("render:draw:".concat(nt, ":").concat(f.kind, ":").concat(f.kind === "block" ? f.tagName : "text", ":start"));
            var V = f.kind === "block" ? f.key && f.key.length > 0 ? f.key : "".concat(nt, ":").concat((ft = f.tagName) != null ? ft : "block") : "", K = f.kind === "block" ? "b:".concat(V) : "t:".concat(nt);
            B("render:draw:".concat(nt, ":cache"));
            var U = b.get(K);
            (!U || Qn(G, U)) && (B("render:draw:".concat(nt, ":new-container")), U = new Ct, U.label = K, b.set(K, U)), B("render:draw:".concat(nt, ":ensure-child")), y.add(K), Ze(G, U, L), B("render:draw:".concat(nt, ":children-root"));
            var gt = ue(U, "__children");
            if (B("render:draw:".concat(nt, ":ensure-children-root")), Ze(U, gt, 1), B("render:draw:".concat(nt, ":position")), qt(U, f.x, f.y), f.kind === "block" && f.tagName === "hr" && qt(U, Math.round(f.x), Math.round(f.y)), f.kind === "block" && f.tagName === "dialog" && f.key) {
                var yt = cn(l.dialogs, f.key), at = Math.max(0, f.width), et = Math.max(0, f.height), dt = z.x, Ft = z.y, ht = Math.max(dt, z.x + z.w - at), Rt = Math.max(Ft, z.y + z.h - et);
                if (l.dialogDragBounds.set(f.key, { minX: dt, minY: Ft, maxX: ht, maxY: Rt }), Lt() && !yt.__trueosInitialPositionSeeded) {
                    var jt = z.w <= 760 && z.h <= 800, ne = dt + Math.max(12, Math.floor((z.w - at) / 2)), le = Ft + Math.max(jt ? 190 : 40, Math.floor((z.h - et) / 2));
                    yt.x = Math.max(dt, Math.min(ht, ne)), yt.y = Math.max(Ft, Math.min(Rt, le)), yt.__trueosInitialPositionSeeded = !0;
                }
                yt.x = Math.max(dt, Math.min(ht, yt.x)), yt.y = Math.max(Ft, Math.min(Rt, yt.y)), qt(U, yt.x, yt.y);
            }
            var F = tt + U.position.x, $ = it + U.position.y;
            if (f.kind === "block") {
                B("render:draw:".concat(nt, ":block:").concat(f.tagName, ":begin"));
                var yt = W;
                (f.tagName === "h1" || f.tagName === "h2" || f.tagName === "h3" || f.tagName === "summary" || f.tagName === "th") && (yt = { bold: !0 }), B("render:draw:".concat(nt, ":graphics"));
                var at_1 = $t(U, "__g");
                B("render:draw:".concat(nt, ":graphics-clear")), Gt(at_1), B("render:draw:".concat(nt, ":graphics-ensure")), Li(U, at_1, 0), at_1.zIndex = -10;
                var et_1 = Math.max(0, f.width), dt_1 = Math.max(0, f.height), Ft = null;
                if ((f.tagName === "h1" || f.tagName === "h2" || f.tagName === "h3") && (qt(U, Math.round(f.x), Math.round(f.y)), et_1 = Math.round(et_1), dt_1 = Math.round(dt_1)), B("render:draw:".concat(nt, ":widget:").concat(f.tagName)), f.tagName === "hr")
                    Tr({ graphics: at_1, w: et_1, theme: r });
                else if (f.tagName !== "barrow") {
                    if (f.tagName !== "searchrow") {
                        if (f.tagName === "searchbutton")
                            oi({ node: f, container: U, graphics: at_1, w: et_1, h: dt_1, theme: r, uiState: l, getPointerId: Bt, focusInputKey: (q = f.attrs) == null ? void 0 : q["data-focus-key"], requestPaint: wt });
                        else if (f.tagName === "progress" || f.tagName === "meter")
                            mr({ node: f, graphics: at_1, w: et_1, h: dt_1, theme: r });
                        else if (f.tagName === "sliderlabel")
                            br({ node: f, container: U, theme: r, sliderStates: l.sliders });
                        else if (f.tagName === "slider")
                            ln({ node: f, container: U, graphics: at_1, w: et_1, h: dt_1, absX: F, absY: $, theme: r, sliderStates: l.sliders, sliderBounds: l.sliderBounds, sliderDrags: l.sliderDrags, requestPaint: wt, getPointerId: Bt });
                        else if (f.tagName === "timeinput" || f.tagName === "dateinput" || f.tagName === "monthinput" || f.tagName === "weekinput" || f.tagName === "datetimelocalinput")
                            bi({ node: f, container: U, graphics: at_1, w: et_1, h: dt_1, absX: F, absY: $, theme: r, uiState: l, getPointerId: Bt, getCursorColor: te, temporalStates: l.temporals, yearSliderOwners: l.temporalYearOwners, getOrInitInputValue: function (ct, Tt) { return qe(ct, Tt); }, requestPaint: wt, requestOverlayPaint: Xt, popupSink: m });
                        else if (f.tagName === "input") {
                            var ct = f.key, Tt = ct != null ? A(ct) : null, Ut = ct != null && l.focusedKeyByPointer.get(l.keyboardOwnerPointerId) === ct, St = ct == null ? null : Ut ? l.keyboardOwnerPointerId : u.has(ct) ? R(ct) : null, ut = St != null, rt = Tt != null ? te(Tt) : null;
                            qr({ node: f, container: U, graphics: at_1, w: et_1, h: dt_1, absX: F, absY: $, theme: r, textMeasure: _, uiState: l, getOrInitInputState: qe, clamp: p, radioGroups: d, textDrags: l.textDrags, requestPaint: wt, showCaret: ut, caretPointerId: St, focusColor: rt != null ? rt : void 0, getCursorColor: te, getPointerId: Bt });
                        }
                        else if (f.tagName === "textarea") {
                            var ct = f.key, Tt = ct != null ? A(ct) : null, Ut = ct != null && l.focusedKeyByPointer.get(l.keyboardOwnerPointerId) === ct, St = ct == null ? null : Ut ? l.keyboardOwnerPointerId : u.has(ct) ? R(ct) : null, ut = St != null, rt = Tt != null ? te(Tt) : null;
                            ei({ node: f, container: U, graphics: at_1, w: et_1, h: dt_1, absX: F, absY: $, theme: r, textMeasure: _, uiState: l, getOrInitInputState: qe, clamp: p, textDrags: l.textDrags, requestPaint: wt, showCaret: ut, caretPointerId: St, focusColor: rt != null ? rt : void 0, getCursorColor: te, getPointerId: Bt });
                        }
                        else if (f.tagName === "select") {
                            if (f.key) {
                                var ct = Number((bt = (ot = f.attrs) == null ? void 0 : ot["data-selected-index"]) != null ? bt : "0");
                                Ee(l.selects, f.key, Number.isFinite(ct) ? ct : 0);
                            }
                            mn({ node: f, container: U, graphics: at_1, w: et_1, h: dt_1, absX: F, absY: $, theme: r, selectStates: l.selects, uiState: l, getPointerId: Bt, getCursorColor: te, requestPaint: wt, requestOverlayPaint: Xt, popupSink: h });
                        }
                        else if (f.tagName === "summary")
                            f.key && l.hoverRects.push({ key: f.key, kind: "summary", cursor: "pointer", x: F, y: $, w: et_1, h: dt_1 }), _r({ node: f, container: U, w: et_1, h: dt_1, theme: r, detailsOpen: l.detailsOpen, requestRerender: xn });
                        else if (f.tagName === "dialog")
                            ai({ node: f, container: U, w: et_1, h: dt_1, theme: r, selectedBy: l.dialogSelectedBy, getCursorColor: te, dialogStates: l.dialogs, dialogDrags: l.dialogDrags, bringToFront: function (ct) { l.dialogZ.set(ct, l.dialogZCounter++); }, requestPaint: wt, getPointerId: Bt });
                        else if (f.tagName === "img")
                            Wr({ node: f, container: U, graphics: at_1, w: et_1, h: dt_1, theme: r, requestRerender: xn });
                        else if (f.tagName === "svg") {
                            var ct = (T = (g = f.attrs) == null ? void 0 : g["data-svg"]) != null ? T : "";
                            zr({ svgMarkup: ct, container: U, w: et_1, h: dt_1, requestRerender: xn });
                        }
                        else if (f.tagName === "canvas")
                            Vr({ node: f, container: U, graphics: at_1, w: et_1, h: dt_1, theme: r });
                        else if (f.tagName === "iframe")
                            Qr({ node: f, container: U, graphics: at_1, w: et_1, h: dt_1, theme: r });
                        else if (f.tagName === "color")
                            l.color.bounds = { x: F, y: $, w: Math.max(0, et_1), h: Math.max(0, dt_1) }, mi({ node: f, container: U, graphics: at_1, w: et_1, h: dt_1, theme: r, rgb: l.color.rgb, setRgb: function (ct) { l.color.rgb = ct; }, alpha: l.color.a, setAlpha: function (ct) { l.color.a = Math.max(0, Math.min(255, Math.round(ct))); }, pick: l.color.pick, setPick: function (ct) { l.color.pick = ct; }, requestPaint: wt, getPointerId: Bt, setDraggingPointerId: function (ct) { l.color.draggingPointerId = ct; } });
                        else if (f.tagName === "number") {
                            var ct_1 = f.key, Tt_1 = String((M = (v = f.attrs) == null ? void 0 : v.channel) != null ? M : "").toLowerCase(), Ut_1 = Tt_1 === "r" || Tt_1 === "g" || Tt_1 === "b" || Tt_1 === "a";
                            ct_1 && ci({ node: f, container: U, graphics: at_1, w: et_1, h: dt_1, theme: r, getValue: function () { var St, ut; return Ut_1 ? Tt_1 === "a" ? (St = l.color.a) != null ? St : 255 : (ut = l.color.rgb[Tt_1]) != null ? ut : 0 : Gn(l.numbers, ct_1, f.attrs).value; }, setValue: function (St) { Ut_1 ? Tt_1 === "a" ? l.color.a = Math.max(0, Math.min(255, Math.round(St))) : l.color.rgb[Tt_1] = Math.max(0, Math.min(255, Math.round(St))) : Gn(l.numbers, ct_1, f.attrs).value = St; }, requestPaint: wt, numberHolds: l.numberHolds, getPointerId: Bt });
                        }
                        else if (f.tagName === "button") {
                            var ct = ge(Pn(f));
                            f.key && l.hoverRects.push({ key: f.key, kind: "button", cursor: "pointer", x: F, y: $, w: et_1, h: dt_1 }), Ir({ container: U, graphics: at_1, w: et_1, h: dt_1, label: ct, theme: r, publishFastPath: function (Tt) { if (!Lt())
                                    return; var Ut = window.__pixiCapture, St = Ut && typeof Ut.objectId == "function" ? Ut.objectId.bind(Ut) : null; if (!St)
                                    return; var ut = typeof at_1.getGlobalPosition == "function" ? at_1.getGlobalPosition() : { x: F, y: $ }; window.__TRUEOS_PIXI_LAST_GRAPHICS_FAST_PATH__ = { owner: "button-hover", rootNode: St(t.stage), graphicsNode: St(at_1), x: .5, y: .5, w: Math.max(0, et_1 - 1), h: Math.max(0, dt_1 - 1), worldX: Number(ut == null ? void 0 : ut.x) || F, worldY: Number(ut == null ? void 0 : ut.y) || $, fillColor: Tt, fillAlpha: 1, strokeColor: r.control.button.border, strokeAlpha: 1, strokeWidth: 1 }; }, registerHoverHandlers: f.key ? function (Tt) { l.hoverHandlers.set(f.key, Tt); } : void 0 });
                        }
                        else if (!Dn(f.tagName))
                            if (f.tagName === "table")
                                Sr({ graphics: at_1, w: et_1, h: dt_1, boxBorder: r.boxBorder });
                            else if (f.tagName === "td" || f.tagName === "th")
                                Pr({ nodeTag: f.tagName, graphics: at_1, w: et_1, h: dt_1, theme: r });
                            else {
                                var ct = Math.max(0, Math.round(et_1)), Tt = Math.max(0, Math.round(dt_1));
                                at_1.rect(0, 0, ct, Tt), at_1.stroke({ width: 1, color: r.boxBorder, alignment: 0 });
                            }
                    }
                }
                B("render:draw:".concat(nt, ":overlay-label")), Ft && U.addChild(Ft);
                var ht = null, Rt = null, jt = f.tagName === "iframe" && String((P = (E = f.attrs) == null ? void 0 : E["data-root"]) != null ? P : "") === "1";
                if (f.tagName === "iframe" && !jt) {
                    f.key && l.iframeRects.push({ key: f.key, x: F, y: $, w: Math.max(0, et_1), h: Math.max(0, dt_1) }), ht = ue(U, "__iframeContentRoot"), qt(ht, 0, 0);
                    var St = $t(U, "__iframeContentMask");
                    Gt(St);
                    var ut = 0, rt = 34, Et = Math.max(0, et_1), Ht = Math.max(0, dt_1 - 34);
                    St.rect(ut, rt, Et, Ht), St.fill(16777215), St.alpha = 0, ht.mask = St;
                    var At_1 = (I = f.key) != null ? I : "", mt_1 = (O = l.iframeScroll.get(At_1)) != null ? O : { y: 0, contentHeight: 0, viewportHeight: 0, draggingPointerId: null, dragOffsetY: 0, track: { x: 0, y: 0, w: Oe, h: 0 }, thumb: { x: 0, y: 0, w: Oe, h: 0 }, rect: { x: F, y: $, w: Math.max(0, et_1), h: Math.max(0, dt_1) } };
                    mt_1.rect = { x: F, y: $, w: Math.max(0, et_1), h: Math.max(0, dt_1) }, mt_1.contentHeight = w(f), mt_1.viewportHeight = Math.max(0, dt_1 - 34 - 8);
                    var Yt_1 = Math.max(0, mt_1.contentHeight - mt_1.viewportHeight);
                    mt_1.y = Math.max(0, Math.min(mt_1.y, Yt_1)), Rt = ue(ht, "__iframeScrollRoot"), qt(Rt, 0, -mt_1.y), At_1 && l.iframeScrollRoots.set(At_1, Rt);
                    var Kt = $t(U, "__iframeScrollbar");
                    At_1 && l.iframeScrollbarGraphics.set(At_1, Kt), Gt(Kt), Kt.eventMode = "static";
                    var xe = wn, Ie = Oe, en = Math.max(0, et_1 - Ie - xe), Rn = 34 + xe, $e = Math.max(0, dt_1 - 34 - xe * 2), Zn = Yt_1 > .5 && $e > 1;
                    if (Kt.visible = Zn, Zn) {
                        var kn = Math.max(24, (mt_1.viewportHeight || 1) / Math.max(1, mt_1.contentHeight) * $e), ji = Math.max(1, $e - kn), Vi = Yt_1 <= 0 ? 0 : mt_1.y / Yt_1, qn = Rn + ji * Vi;
                        mt_1.track = { x: F + en, y: $ + Rn, w: Ie, h: $e }, mt_1.thumb = { x: F + en, y: $ + qn, w: Ie, h: kn }, Kt.rect(en, Rn, Ie, $e), Kt.fill({ color: 0, alpha: .06 }), Kt.rect(en, qn, Ie, kn), Kt.fill({ color: 0, alpha: .25 }), Kt.on("pointerdown", function (oe) { var nr, rr, ir, or, sr, ar; if ((oe == null ? void 0 : oe.button) === 2)
                            return; var On = Bt(oe); if (On <= 0)
                            return; var nn = (rr = (nr = oe.global) == null ? void 0 : nr.x) != null ? rr : 0, Me = (or = (ir = oe.global) == null ? void 0 : ir.y) != null ? or : 0; if (!(nn >= mt_1.track.x && nn <= mt_1.track.x + mt_1.track.w && Me >= mt_1.track.y && Me <= mt_1.track.y + mt_1.track.h))
                            return; if (nn >= mt_1.thumb.x && nn <= mt_1.thumb.x + mt_1.thumb.w && Me >= mt_1.thumb.y && Me <= mt_1.thumb.y + mt_1.thumb.h) {
                            mt_1.draggingPointerId = On, mt_1.dragOffsetY = Me - mt_1.thumb.y, l.iframeScroll.set(At_1, mt_1), (sr = oe.stopPropagation) == null || sr.call(oe);
                            return;
                        } var tr = Math.max(1, mt_1.track.h - mt_1.thumb.h), er = Math.max(mt_1.track.y, Math.min(mt_1.track.y + tr, Me - mt_1.thumb.h / 2)), Ji = (er - mt_1.track.y) / tr; mt_1.y = Math.max(0, Math.min(Yt_1, Ji * Yt_1)), mt_1.draggingPointerId = On, mt_1.dragOffsetY = Me - er, l.iframeScroll.set(At_1, mt_1), Lt() && (window.__TRUEOS_PIXI_SCROLL_REPAINT_REQUIRED__ = !0, window.__TRUEOS_PIXI_SCROLL_REPAINT_OWNER__ = "iframe", window.__TRUEOS_PIXI_SCROLL_REPAINT_IFRAME_KEY__ = At_1), wt == null || wt(), (ar = oe.stopPropagation) == null || ar.call(oe); });
                    }
                    else
                        mt_1.track = { x: 0, y: 0, w: Ie, h: 0 }, mt_1.thumb = { x: 0, y: 0, w: Ie, h: 0 };
                    l.iframeScroll.set(At_1, mt_1);
                }
                var ne = [], le = f.tagName === "dialog" || f.tagName === "iframe" && !jt ? ne : st, ce = z;
                if (f.tagName === "dialog")
                    ce = { x: 0, y: 0, w: Math.max(0, et_1), h: Math.max(0, dt_1) };
                else if (f.tagName === "iframe" && !jt) {
                    var ct = (J = f.key) != null ? J : "", Tt = l.iframeScroll.get(ct), Ut = Tt ? Tt.y : 0, St = 34;
                    ce = { x: 0, y: St + Ut, w: Math.max(0, et_1), h: Math.max(0, dt_1 - St) };
                }
                var be = (Q = Rt != null ? Rt : ht) != null ? Q : gt, _e = F + ((Y = ht == null ? void 0 : ht.position.x) != null ? Y : 0), ye = $ + ((lt = ht == null ? void 0 : ht.position.y) != null ? lt : 0) + ((Z = Rt == null ? void 0 : Rt.position.y) != null ? Z : 0);
                B("render:draw:".concat(nt, ":children"));
                var me = 0;
                for (var ct = 0; ct < ((_t = f.children) != null ? _t : []).length; ct++) {
                    var Tt = ((Mt = f.children) != null ? Mt : [])[ct];
                    if (Tt.kind === "block" && Tt.tagName === "dialog")
                        le.push(Tt);
                    else {
                        if (f.tagName === "button" && Tt.kind === "text")
                            continue;
                        N(Tt, be, yt, _e, ye, le, ce, "".concat(nt, ".").concat(ct), me++);
                    }
                }
                if ((f.tagName === "dialog" || f.tagName === "iframe" && !jt) && ne.length > 0) {
                    ne.sort(function (ct, Tt) { var ut, rt; var Ut = ct.key && (ut = l.dialogZ.get(ct.key)) != null ? ut : 0, St = Tt.key && (rt = l.dialogZ.get(Tt.key)) != null ? rt : 0; return Ut - St; });
                    try {
                        for (var ne_1 = __values(ne), ne_1_1 = ne_1.next(); !ne_1_1.done; ne_1_1 = ne_1.next()) {
                            var ct = ne_1_1.value;
                            var Tt = ct.key && ct.key.length > 0 ? ct.key : "".concat(nt, ".dlg.").concat(me);
                            N(ct, be, yt, _e, ye, ne, ce, "".concat(nt, ".dlg.").concat(Tt), me++);
                        }
                    }
                    catch (e_60_1) { e_60 = { error: e_60_1 }; }
                    finally {
                        try {
                            if (ne_1_1 && !ne_1_1.done && (_a = ne_1.return)) _a.call(ne_1);
                        }
                        finally { if (e_60) throw e_60.error; }
                    }
                }
            }
            else {
                B("render:draw:".concat(nt, ":text:begin"));
                var yt = vt(U, "__text", function (at) { at.style = { fontFamily: r.fontFamily, fontSize: r.fontSize, fill: r.text, fontWeight: W.bold ? "700" : "400", wordWrap: !0, wordWrapWidth: 0 }; });
                yt.text = (xt = f.text) != null ? xt : "", yt.style.fontFamily = r.fontFamily, yt.style.fontSize = r.fontSize, yt.style.fill = r.text, yt.style.fontWeight = W.bold ? "700" : "400", yt.style.wordWrap = !0, yt.style.wordWrapWidth = Math.max(0, Math.ceil(f.width) + Ne), qt(yt, 0, Pt), B("render:draw:".concat(nt, ":text:done"));
            }
        }
        B("render:root-loop");
        var D = { bold: !1 }, x = { x: 0, y: 0, w: t.renderer.width, h: t.renderer.height }, C = [], H = s.position, k = H && Number(H.y || 0) || 0, S = 0;
        for (var f = 0; f < e.children.length; f++) {
            B("render:root-loop:".concat(f));
            var G = e.children[f];
            G && (G.kind === "block" && G.tagName === "dialog" ? C.push(G) : (B("render:root-loop:".concat(f, ":dispatch")), N(G, s, D, 0, k, C, x, "root.".concat(f), S++)));
        }
        if (B("render:root-dialogs"), C.length > 0) {
            C.sort(function (G, W) { var st, z; var tt = G.key && (st = l.dialogZ.get(G.key)) != null ? st : 0, it = W.key && (z = l.dialogZ.get(W.key)) != null ? z : 0; return tt - it; });
            var f = 0;
            try {
                for (var C_1 = __values(C), C_1_1 = C_1.next(); !C_1_1.done; C_1_1 = C_1.next()) {
                    var G = C_1_1.value;
                    var W = G.key && G.key.length > 0 ? G.key : "rootdlg.".concat(f);
                    N(G, c, D, 0, 0, C, x, "dlg.".concat(W), f++);
                }
            }
            catch (e_52_1) { e_52 = { error: e_52_1 }; }
            finally {
                try {
                    if (C_1_1 && !C_1_1.done && (_b = C_1.return)) _b.call(C_1);
                }
                finally { if (e_52) throw e_52.error; }
            }
        }
        if (B("render:temporal-popups"), m.length > 0 && $n({ popups: m, stage: a, theme: r, viewportW: t.renderer.width, viewportH: t.renderer.height, temporalStates: l.temporals, getOrInitInputValue: function (f, G) { return qe(f, G); }, sliders: l.sliders, sliderBounds: l.sliderBounds, sliderDrags: l.sliderDrags, selects: l.selects, selectPopups: h, uiFocus: l, getPointerId: Bt, getCursorColor: te, requestPaint: wt, requestOverlayPaint: Xt }), B("render:select-popups"), h.length > 0)
            try {
                for (var h_2 = __values(h), h_2_1 = h_2.next(); !h_2_1.done; h_2_1 = h_2.next()) {
                    var f = h_2_1.value;
                    Fn({ popup: f, stage: a, theme: r, selectStates: l.selects, uiState: l, getPointerId: Bt, requestPaint: wt, viewportW: t.renderer.width, viewportH: t.renderer.height });
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
        var _loop_3 = function (f, G) {
            if (!(G != null && G.open))
                return "continue";
            var W = new Ct;
            W.eventMode = "static", W.cursor = "default", qt(W, G.x, G.y);
            var tt = 140, it = 28, st = 6, z = ["Copy", "Paste", "Close"], nt = new kt;
            nt.rect(0, 0, tt + st * 2, z.length * it + st * 2), nt.fill(16777215);
            var L = 1;
            nt.rect(L, L, tt + st * 2 - L * 2, z.length * it + st * 2 - L * 2), nt.stroke({ width: 2, color: te(f), alignment: 0 }), W.addChild(nt), z.forEach(function (V, K) { var U = st + K * it, gt = new Ct; gt.eventMode = "static", gt.cursor = "pointer", qt(gt, st, U); var F = new kt; F.rect(0, 0, tt, it), F.fill(16777215), gt.addChild(F); var $ = ie({ text: V, fontFamily: r.fontFamily, fontSize: r.fontSize, fill: r.text }); qt($, 8, Math.max(0, (it - $.height) / 2) + Pt), gt.addChild($); var ft = function (ot) { return Bt(ot) === f; }, q = function (ot) { if (!Lt())
                return; var bt = window.__pixiCapture, g = bt && typeof bt.objectId == "function" ? bt.objectId.bind(bt) : null; if (!g)
                return; var T = typeof F.getGlobalPosition == "function" ? F.getGlobalPosition() : { x: 0, y: 0 }; window.__TRUEOS_PIXI_LAST_GRAPHICS_FAST_PATH__ = { owner: "context-menu-hover", rootNode: g(t.stage), graphicsNode: g(F), x: 0, y: 0, w: tt, h: it, worldX: Number(T == null ? void 0 : T.x) || 0, worldY: Number(T == null ? void 0 : T.y) || 0, fillColor: ot, fillAlpha: 1 }; }; gt.on("pointerover", function (ot) { ft(ot) && (F.clear(), F.rect(0, 0, tt, it), F.fill(15921906), q(15921906)); }), gt.on("pointerout", function (ot) { ft(ot) && (F.clear(), F.rect(0, 0, tt, it), F.fill(16777215), q(16777215)); }), gt.on("pointerdown", function (ot) { var M, E, P, I, O, J, Q, Y, lt, Z, _t; if (!ft(ot))
                return; (M = ot.stopPropagation) == null || M.call(ot); var bt = (E = l.focusedKeyByPointer.get(f)) != null ? E : null, g = bt ? l.inputs.get(bt) : null, T = bt != null && l.fieldBounds.has(bt) && g != null && typeof g.value == "string"; if (V === "Copy" && T) {
                var Mt = g, xt = (P = Mt.value) != null ? P : "", yt = (O = (I = Mt.selections) == null ? void 0 : I.get(f)) != null ? O : null, at = yt ? Math.max(0, Math.min(xt.length, (J = yt.start) != null ? J : 0)) : 0, et = yt ? Math.max(0, Math.min(xt.length, (Q = yt.end) != null ? Q : at)) : at, dt = Math.min(at, et), Ft = Math.max(at, et), ht = dt !== Ft ? xt.slice(dt, Ft) : xt;
                l.clipboards.set(f, ht);
            }
            else if (V === "Paste" && T) {
                var Mt = (Y = l.clipboards.get(f)) != null ? Y : "";
                if (Mt.length > 0) {
                    var xt = g, yt = (lt = xt.value) != null ? lt : "";
                    if (xt.selections || (xt.selections = new Map), !xt.selections.has(f)) {
                        var jt = yt.length;
                        xt.selections.set(f, { start: jt, end: jt });
                    }
                    var at = xt.selections.get(f), et = Math.max(0, Math.min(yt.length, (Z = at.start) != null ? Z : yt.length)), dt = Math.max(0, Math.min(yt.length, (_t = at.end) != null ? _t : et)), Ft = Math.min(et, dt), ht = Math.max(et, dt);
                    xt.value = yt.slice(0, Ft) + Mt + yt.slice(ht);
                    var Rt = Ft + Mt.length;
                    at.start = Rt, at.end = Rt;
                }
            } var v = l.contextMenus.get(f); v && (v.open = !1, l.contextMenus.set(f, v)), wt == null || wt(); }), W.addChild(gt); }), a.addChild(W);
        };
        try {
            for (var _j = __values(l.contextMenus.entries()), _k = _j.next(); !_k.done; _k = _j.next()) {
                var _l = __read(_k.value, 2), f = _l[0], G = _l[1];
                _loop_3(f, G);
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
                var _q = __read(_p.value, 2), f = _q[0], G = _q[1];
                if (!y.has(f)) {
                    try {
                        G.removeFromParent(), (j = G.destroy) == null || j.call(G, { children: !0 });
                    }
                    catch (W) { }
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
    function Hs() {
        return rn(this, null, function () {
            var t, e, n, r, _a, i_1, _b, o_3, s, c, a_2, h, m_2, d_1, b_1, y_1, u_1, _1, p_1, R, A, _c, N, D_1, x_2, C, H_1, k_1, S_1, X_1, j_1, f_1, G_2, W_2, tt_3, it_1, st_2, z_3, nt_1, L_1, V_4, K_1, U_2, gt_2, F_2, $, ft, q_2, ot_1, g, T, v, bt_1, n_3, r;
            return __generator(this, function (_d) {
                switch (_d.label) {
                    case 0:
                        _d.trys.push([0, 9, , 10]);
                        Nt("main:start");
                        n = (t = document.getElementById("app")) != null ? t : document.body, r = !0;
                        Nt("main:yoga");
                        if (!r) return [3 /*break*/, 1];
                        _a = ks();
                        return [3 /*break*/, 3];
                    case 1: return [4 /*yield*/, Promise.resolve().then(function () { return (Pi(), Si); })];
                    case 2:
                        _a = (_d.sent()).default;
                        _d.label = 3;
                    case 3:
                        pt = _a, Nt("main:create-app");
                        i_1 = r ? Rs() : new an;
                        _b = r;
                        if (_b) return [3 /*break*/, 5];
                        return [4 /*yield*/, i_1.init({ background: "#ffffff", resizeTo: window, antialias: !1, preference: "webgl" })];
                    case 4:
                        _b = (_d.sent());
                        _d.label = 5;
                    case 5:
                        _b, Nt("main:attach-capture"), Mi(i_1), window.__TRUEOS_PIXI_APP = i_1, Nt("main:append-canvas"), n.appendChild(i_1.canvas), i_1.ticker.stop(), Nt("main:capture-flags"), r && (l.harness.enabled = !1, l.virtualCursor.enabled = !1, window.__pixiCapture && (window.__pixiCapture.persist = !1)), Nt("main:canvas-listeners"), i_1.canvas.addEventListener("contextmenu", function (g) { return g.preventDefault(); }), i_1.canvas.addEventListener("wheel", function (g) { var J, Q; var T = (J = g.offsetX) != null ? J : 0, v = (Q = g.offsetY) != null ? Q : 0, M = function (Y) { var _t; if (!Lt())
                            return; var lt = window, Z = Number((_t = lt.__TRUEOS_WHEEL_ROUTE_LOG_COUNT__) != null ? _t : 0) || 0; Z >= 32 || (lt.__TRUEOS_WHEEL_ROUTE_LOG_COUNT__ = Z + 1, console.log("[trueos pixi widgets] wheel-route ".concat(Y))); }, E = null; for (var Y = l.iframeRects.length - 1; Y >= 0; Y--) {
                            var lt = l.iframeRects[Y];
                            if (T >= lt.x && T <= lt.x + lt.w && v >= lt.y && v <= lt.y + lt.h) {
                                E = lt.key;
                                break;
                            }
                        } var P = !1; if (E) {
                            var Y = l.iframeScroll.get(E);
                            if (Y) {
                                var lt = Math.max(0, Y.contentHeight - Y.viewportHeight);
                                if (M("hit=iframe x=".concat(Math.round(T), " y=").concat(Math.round(v), " delta=").concat(Math.round(g.deltaY), " y0=").concat(Math.round(Y.y), " max=").concat(Math.round(lt))), lt > 0) {
                                    var Z = Math.max(0, Math.min(lt, Y.y + g.deltaY));
                                    Z !== Y.y && (Y.y = Z, l.iframeScroll.set(E, Y), Lt() && (window.__TRUEOS_PIXI_SCROLL_REPAINT_REQUIRED__ = !0, window.__TRUEOS_PIXI_SCROLL_REPAINT_OWNER__ = "iframe", window.__TRUEOS_PIXI_SCROLL_REPAINT_IFRAME_KEY__ = E), wt == null || wt(), g.preventDefault(), P = !0, M("owner=iframe y1=".concat(Math.round(Z), " repaint=1")));
                                }
                            }
                        } if (P)
                            return; var I = Math.max(0, l.scroll.contentHeight - l.scroll.viewportHeight); if (I <= 0) {
                            M("owner=none x=".concat(Math.round(T), " y=").concat(Math.round(v), " delta=").concat(Math.round(g.deltaY), " root_y=").concat(Math.round(l.scroll.y), " root_max=0"));
                            return;
                        } var O = Math.max(0, Math.min(I, l.scroll.y + g.deltaY)); if (O !== l.scroll.y) {
                            var Y = l.scroll.y;
                            l.scroll.y = O, Lt() && (window.__TRUEOS_PIXI_SCROLL_REPAINT_REQUIRED__ = !0, window.__TRUEOS_PIXI_SCROLL_REPAINT_OWNER__ = "root", window.__TRUEOS_PIXI_SCROLL_REPAINT_IFRAME_KEY__ = ""), wt == null || wt(), g.preventDefault(), M("owner=root x=".concat(Math.round(T), " y=").concat(Math.round(v), " delta=").concat(Math.round(g.deltaY), " y0=").concat(Math.round(Y), " y1=").concat(Math.round(O), " max=").concat(Math.round(I), " repaint=1"));
                        }
                        else
                            M("owner=root-boundary x=".concat(Math.round(T), " y=").concat(Math.round(v), " delta=").concat(Math.round(g.deltaY), " y0=").concat(Math.round(l.scroll.y), " max=").concat(Math.round(I))); }, { passive: !1 }), Nt("main:stage:eventMode"), i_1.stage.eventMode = "static", Nt("main:stage:hitArea"), i_1.stage.hitArea = i_1.screen, Nt("main:stage:on:pointerdown"), i_1.stage.on("pointerdown", function (g) {
                            var e_61, _a;
                            var T, v, M, E, P, I;
                            if ((g == null ? void 0 : g.button) === 2) {
                                var O = Bt(g);
                                if (O > 0) {
                                    var J = (T = l.contextMenus.get(O)) != null ? T : { open: !1, x: 0, y: 0 };
                                    J.open = !0, J.x = (M = (v = g.global) == null ? void 0 : v.x) != null ? M : 0, J.y = (P = (E = g.global) == null ? void 0 : E.y) != null ? P : 0, l.contextMenus.set(O, J);
                                }
                                Xt == null || Xt(), (I = g.preventDefault) == null || I.call(g);
                                return;
                            }
                            if ((g == null ? void 0 : g.button) !== 2) {
                                var O = Bt(g), J = O > 0 ? l.contextMenus.get(O) : null;
                                J && J.open && (J.open = !1, l.contextMenus.set(O, J), Xt == null || Xt());
                            }
                            if ((g == null ? void 0 : g.button) !== 2) {
                                var O = !1;
                                try {
                                    for (var _b = __values(l.selects.values()), _c = _b.next(); !_c.done; _c = _b.next()) {
                                        var J = _c.value;
                                        J.open && (J.open = !1, O = !0);
                                    }
                                }
                                catch (e_61_1) { e_61 = { error: e_61_1 }; }
                                finally {
                                    try {
                                        if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
                                    }
                                    finally { if (e_61) throw e_61.error; }
                                }
                                O && (Xt == null || Xt());
                            }
                            (g == null ? void 0 : g.button) !== 2 && _i(l.temporals) && (Xt == null || Xt()), F_2();
                        }), Nt("main:stage:done"), Nt("main:roots");
                        o_3 = new Ct, s = new Ct;
                        s.eventMode = "static";
                        c = new Ct;
                        c.eventMode = "none", i_1.stage.addChild(o_3), i_1.stage.addChild(s), i_1.stage.addChild(c);
                        a_2 = new kt;
                        a_2.label = "__trueosGlobalScrollbar", a_2.eventMode = "static", s.addChild(a_2);
                        h = function (g, T) { g.clear(); var v = T.half, M = T.strokeWidth, E = T.color; g.moveTo(-v, 0), g.lineTo(v, 0), g.stroke({ width: M, color: E }), g.moveTo(0, -v), g.lineTo(0, v), g.stroke({ width: M, color: E }); }, m_2 = new kt;
                        m_2.eventMode = "none", m_2.visible = !1, c.addChild(m_2);
                        d_1 = new kt;
                        d_1.eventMode = "none", d_1.visible = !1, c.addChild(d_1);
                        b_1 = new kt;
                        b_1.eventMode = "none", b_1.visible = !1, c.addChild(b_1);
                        y_1 = new kt;
                        y_1.eventMode = "none", c.addChild(y_1), Nt("main:text-measure");
                        u_1 = document.createElement("canvas").getContext("2d");
                        if (!u_1)
                            throw new Error("2D canvas not available");
                        u_1.font = "".concat(ee.fontSize, "px ").concat(ee.fontFamily);
                        _1 = function (g) { return u_1.measureText(g).width; }, p_1 = ee.fontSize * 1.25;
                        Nt("main:html");
                        if (!(typeof window.__TRUEOS_INPUT_HTML__ == "string")) return [3 /*break*/, 6];
                        _c = window.__TRUEOS_INPUT_HTML__;
                        return [3 /*break*/, 8];
                    case 6: return [4 /*yield*/, fetch("/input.html").then(function (g) { return g.text(); })];
                    case 7:
                        _c = _d.sent();
                        _d.label = 8;
                    case 8:
                        R = _c, A = ls(R) ? R : "";
                        Lt() && console.log("[trueos pixi widgets] input-html chars=".concat(R.length, " usable=").concat(A ? 1 : 0, " sample=\"").concat(Tn(R), "\"")), Nt("main:render-tree"), vi.clear();
                        N = bs(A), D_1 = _s(), x_2 = As(D_1.tree, N.rows), C = Ls(x_2, N.rows);
                        if (Lt() && (console.log("[trueos pixi widgets] text-fallback source=".concat(N.source, " rows=").concat(N.rows.length, " samples=").concat(ds(N.rows))), console.log("[trueos pixi widgets] render-tree source=".concat(D_1.source, " nodes=").concat(x_2.length, " trusted_text_applied=").concat(C))), x_2.length === 0)
                            throw new Error("TrueSurfer widget render tree is missing");
                        H_1 = ss(x_2), k_1 = null, S_1 = { nodes: 0, blocks: 0, text: 0, maxDepth: 0, tags: {} }, X_1 = { hash: "", renderHash: "", layoutHash: "", bytes: 0 }, j_1 = 0, f_1 = null, G_2 = function () { var g = Math.max(0, l.scroll.contentHeight - l.scroll.viewportHeight); l.scroll.y = Math.max(0, Math.min(l.scroll.y, g)); }, W_2 = function () { var g = i_1.renderer.width, T = i_1.renderer.height; l.scroll.viewportHeight = T; var v = l.scroll.contentHeight, M = Math.max(0, v - T), E = M > .5; if (a_2.clear(), a_2.visible = E, !E) {
                            l.scroll.track = { x: 0, y: 0, w: l.scroll.track.w, h: 0 }, l.scroll.thumb = { x: 0, y: 0, w: l.scroll.thumb.w, h: 0 };
                            return;
                        } var P = wn, I = Oe, O = Math.max(0, g - I - P), J = P, Q = Math.max(0, T - P * 2), lt = Math.max(24, T / Math.max(T, v) * Q), Z = Math.max(1, Q - lt), _t = M <= 0 ? 0 : l.scroll.y / M, Mt = J + Z * _t; l.scroll.track = { x: O, y: J, w: I, h: Q }, l.scroll.thumb = { x: O, y: Mt, w: I, h: lt }, a_2.rect(O, J, I, Q), a_2.fill({ color: 0, alpha: .06 }), a_2.rect(O, Mt, I, lt), a_2.fill({ color: 0, alpha: .25 }); }, tt_3 = function () {
                            var e_62, _a;
                            var T = wn, v = Oe;
                            try {
                                for (var _b = __values(l.iframeScrollRoots.entries()), _c = _b.next(); !_c.done; _c = _b.next()) {
                                    var _d = __read(_c.value, 2), M = _d[0], E = _d[1];
                                    var P = l.iframeScroll.get(M);
                                    if (!P)
                                        continue;
                                    var I = Math.max(0, P.contentHeight - P.viewportHeight);
                                    P.y = Math.max(0, Math.min(P.y, I)), qt(E, 0, -P.y);
                                    var O = l.iframeScrollbarGraphics.get(M);
                                    if (!O) {
                                        l.iframeScroll.set(M, P);
                                        continue;
                                    }
                                    var J = Math.max(0, P.rect.w), Q = Math.max(0, P.rect.h), Y = Math.max(0, J - v - T), lt = 34 + T, Z = Math.max(0, Q - 34 - T * 2), _t = I > .5 && Z > 1;
                                    if (O.clear(), O.visible = _t, !_t) {
                                        P.track = { x: 0, y: 0, w: v, h: 0 }, P.thumb = { x: 0, y: 0, w: v, h: 0 }, l.iframeScroll.set(M, P);
                                        continue;
                                    }
                                    var xt = Math.max(24, (P.viewportHeight || 1) / Math.max(1, P.contentHeight) * Z), yt = Math.max(1, Z - xt), at = I <= 0 ? 0 : P.y / I, et = lt + yt * at;
                                    P.track = { x: P.rect.x + Y, y: P.rect.y + lt, w: v, h: Z }, P.thumb = { x: P.rect.x + Y, y: P.rect.y + et, w: v, h: xt }, O.rect(Y, lt, v, Z), O.fill({ color: 0, alpha: .06 }), O.rect(Y, et, v, xt), O.fill({ color: 0, alpha: .25 }), l.iframeScroll.set(M, P);
                                }
                            }
                            catch (e_62_1) { e_62 = { error: e_62_1 }; }
                            finally {
                                try {
                                    if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
                                }
                                finally { if (e_62) throw e_62.error; }
                            }
                        }, it_1 = function (g) {
                            var e_63, _a;
                            var P;
                            var T = [], v = [], M = -l.scroll.y, E = function (I, O, J) {
                                var e_64, _a;
                                var lt;
                                var Q = O + I.x, Y = J + I.y;
                                if (I.kind === "block" && I.key) {
                                    if (I.tagName === "select") {
                                        var Z = l.selects.get(I.key);
                                        if (Z != null && Z.open) {
                                            var _t = Wn(I.attrs);
                                            Z.selectedIndex = Math.max(0, Math.min(_t.length - 1, Z.selectedIndex | 0)), v.push({ key: I.key, absX: Q, absY: Y, w: I.width, h: I.height, options: _t, selectedIndex: Z.selectedIndex });
                                        }
                                    }
                                    else if (I.tagName === "timeinput" || I.tagName === "dateinput" || I.tagName === "monthinput" || I.tagName === "weekinput" || I.tagName === "datetimelocalinput") {
                                        var Z = l.temporals.get(I.key);
                                        (Z == null ? void 0 : Z.openPanel) === "month" ? T.push({ kind: "month-panel", inputKey: I.key, absX: Q, absY: Y, anchorW: I.width, anchorH: I.height }) : (Z == null ? void 0 : Z.openPanel) === "week" ? T.push({ kind: "week-panel", inputKey: I.key, absX: Q, absY: Y, anchorW: I.width, anchorH: I.height }) : (Z == null ? void 0 : Z.openPanel) === "time" && T.push({ kind: "time-panel", inputKey: I.key, absX: Q, absY: Y, anchorW: I.width, anchorH: I.height });
                                    }
                                }
                                try {
                                    for (var _b = __values((lt = I.children) != null ? lt : []), _c = _b.next(); !_c.done; _c = _b.next()) {
                                        var Z = _c.value;
                                        Z.kind === "block" && Z.tagName === "dialog" || E(Z, Q, Y);
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
                                for (var _b = __values((P = g.children) != null ? P : []), _c = _b.next(); !_c.done; _c = _b.next()) {
                                    var I = _c.value;
                                    E(I, 0, M);
                                }
                            }
                            catch (e_63_1) { e_63 = { error: e_63_1 }; }
                            finally {
                                try {
                                    if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
                                }
                                finally { if (e_63) throw e_63.error; }
                            }
                            return { temporalPopups: T, selectPopups: v };
                        }, st_2 = function (g) {
                            var e_65, _a;
                            var _loop_4 = function (T, v) {
                                if (!(v != null && v.open))
                                    return "continue";
                                var M = new Ct;
                                M.eventMode = "static", M.cursor = "default", qt(M, v.x, v.y);
                                var E = 140, P = 28, I = 6, O = ["Copy", "Paste", "Close"], J = new kt;
                                J.rect(0, 0, E + I * 2, O.length * P + I * 2), J.fill(16777215);
                                var Q = 1;
                                J.rect(Q, Q, E + I * 2 - Q * 2, O.length * P + I * 2 - Q * 2), J.stroke({ width: 2, color: te(T), alignment: 0 }), M.addChild(J), O.forEach(function (Y, lt) { var Z = I + lt * P, _t = new Ct; _t.eventMode = "static", _t.cursor = "pointer", qt(_t, I, Z); var Mt = new kt; Mt.rect(0, 0, E, P), Mt.fill(16777215), _t.addChild(Mt); var xt = ie({ text: Y, fontFamily: ee.fontFamily, fontSize: ee.fontSize, fill: ee.text }); qt(xt, 8, Math.max(0, (P - xt.height) / 2) + Pt), _t.addChild(xt); var yt = function (at) { return Bt(at) === T; }; _t.on("pointerdown", function (at) { var jt, ne, le, ce, be, _e, ye, me, ct, Tt, Ut; if (!yt(at))
                                    return; (jt = at.stopPropagation) == null || jt.call(at); var et = (ne = l.focusedKeyByPointer.get(T)) != null ? ne : null, dt = et ? l.inputs.get(et) : null, Ft = et != null && l.fieldBounds.has(et) && dt != null && typeof dt.value == "string", ht = !1; if (Y === "Copy" && Ft) {
                                    var St = dt, ut = (le = St.value) != null ? le : "", rt = (be = (ce = St.selections) == null ? void 0 : ce.get(T)) != null ? be : null, Et = rt ? Math.max(0, Math.min(ut.length, (_e = rt.start) != null ? _e : 0)) : 0, Ht = rt ? Math.max(0, Math.min(ut.length, (ye = rt.end) != null ? ye : Et)) : Et;
                                    l.clipboards.set(T, ut.slice(Math.min(Et, Ht), Math.max(Et, Ht)) || ut);
                                }
                                else if (Y === "Paste" && Ft) {
                                    var St = (me = l.clipboards.get(T)) != null ? me : "";
                                    if (St.length > 0) {
                                        var ut = dt, rt = (ct = ut.value) != null ? ct : "";
                                        if (ut.selections || (ut.selections = new Map), !ut.selections.has(T)) {
                                            var xe = rt.length;
                                            ut.selections.set(T, { start: xe, end: xe });
                                        }
                                        var Et = ut.selections.get(T), Ht = Math.max(0, Math.min(rt.length, (Tt = Et.start) != null ? Tt : rt.length)), At = Math.max(0, Math.min(rt.length, (Ut = Et.end) != null ? Ut : Ht)), mt = Math.min(Ht, At), Yt = Math.max(Ht, At);
                                        ut.value = rt.slice(0, mt) + St + rt.slice(Yt);
                                        var Kt = mt + St.length;
                                        Et.start = Kt, Et.end = Kt, ht = !0;
                                    }
                                } var Rt = l.contextMenus.get(T); Rt && (Rt.open = !1, l.contextMenus.set(T, Rt)), ht ? wt == null || wt() : Xt == null || Xt(); }), M.addChild(_t); }), g.addChild(M);
                            };
                            try {
                                for (var _b = __values(l.contextMenus.entries()), _c = _b.next(); !_c.done; _c = _b.next()) {
                                    var _d = __read(_c.value, 2), T = _d[0], v = _d[1];
                                    _loop_4(T, v);
                                }
                            }
                            catch (e_65_1) { e_65 = { error: e_65_1 }; }
                            finally {
                                try {
                                    if (_c && !_c.done && (_a = _b.return)) _a.call(_b);
                                }
                                finally { if (e_65) throw e_65.error; }
                            }
                        }, z_3 = function (g, T, v) {
                            var e_66, _a;
                            if (g.removeChildren(), T.length > 0 && $n({ popups: T, stage: g, theme: ee, viewportW: i_1.renderer.width, viewportH: i_1.renderer.height, temporalStates: l.temporals, getOrInitInputValue: function (M, E) { return qe(M, E); }, sliders: l.sliders, sliderBounds: l.sliderBounds, sliderDrags: l.sliderDrags, selects: l.selects, selectPopups: v, uiFocus: l, getPointerId: Bt, getCursorColor: te, requestPaint: wt, requestOverlayPaint: Xt }), v.length > 0)
                                try {
                                    for (var v_1 = __values(v), v_1_1 = v_1.next(); !v_1_1.done; v_1_1 = v_1.next()) {
                                        var M = v_1_1.value;
                                        Fn({ popup: M, stage: g, theme: ee, selectStates: l.selects, uiState: l, getPointerId: Bt, requestPaint: wt, viewportW: i_1.renderer.width, viewportH: i_1.renderer.height });
                                    }
                                }
                                catch (e_66_1) { e_66 = { error: e_66_1 }; }
                                finally {
                                    try {
                                        if (v_1_1 && !v_1_1.done && (_a = v_1.return)) _a.call(v_1);
                                    }
                                    finally { if (e_66) throw e_66.error; }
                                }
                            st_2(g);
                        }, nt_1 = function () { if (!k_1)
                            return; var g = ue(o_3, "__overlayRoot"), T = f_1, _a = it_1(k_1), v = _a.temporalPopups, M = _a.selectPopups, E = window.__pixiCapture, P = E && Array.isArray(E.commands) ? E.commands : null, I = P ? P.length : 0; z_3(g, v, M); var O = Di(g); f_1 = O; var J = ke(T, O); if (Lt()) {
                            if (window.__TRUEOS_PIXI_LAST_OVERLAY_FAST_PATH__ = null, window.__TRUEOS_PIXI_INCREMENTAL_COMMANDS__ = [], window.__TRUEOS_PIXI_INCREMENTAL_ROOT__ = 0, window.__TRUEOS_PIXI_INCREMENTAL_DAMAGE__ = null, P && J && J.w > 0 && J.h > 0) {
                                var Q = P.slice(I);
                                P.splice(I, Q.length);
                                var Y = window.__pixiCapture, lt = Y && typeof Y.objectId == "function" ? Y.objectId.bind(Y) : null, Z = Y && typeof Y.snapshotNode == "function" ? Y.snapshotNode.bind(Y) : null, _t = lt ? lt(i_1.stage) : 0, Mt = Z ? Z(g) : null, xt = lt ? lt(g) : 0, at = O && xt > 0 && (!T || Ms(O, T)) ? xt : _t, et = Mt && xt > 0 ? [{ frame: 0, seq: 0, op: "snapshot", id: xt, target: "Container:__overlayRoot", args: [Mt] }] : Q;
                                window.__TRUEOS_PIXI_INCREMENTAL_COMMANDS__ = et, window.__TRUEOS_PIXI_INCREMENTAL_ROOT__ = at, window.__TRUEOS_PIXI_INCREMENTAL_DAMAGE__ = J, window.__TRUEOS_PIXI_LAST_OVERLAY_FAST_PATH__ = { rootNode: at, damageX: J.x, damageY: J.y, damageW: J.w, damageH: J.h };
                            }
                            return;
                        } i_1.renderer.render(i_1.stage); }, L_1 = function () { if (k_1) {
                            if (he("PIXI_PAINT_STEP", 96, "[trueos pixi widgets] paint-step clamp begin"), Nt("main:paint:clamp"), G_2(), he("PIXI_PAINT_STEP", 96, "[trueos pixi widgets] paint-step render-to-pixi begin"), Nt("main:paint:render-to-pixi"), Gs(i_1, k_1, o_3), f_1 = Di(ue(o_3, "__overlayRoot")), he("PIXI_PAINT_STEP", 96, "[trueos pixi widgets] paint-step render-to-pixi done"), Nt("main:paint:scrollbar"), he("PIXI_PAINT_STEP", 96, "[trueos pixi widgets] paint-step scrollbar begin"), W_2(), Nt("main:paint:renderer-render"), he("PIXI_PAINT_STEP", 96, "[trueos pixi widgets] paint-step renderer-render begin"), i_1.renderer.render(i_1.stage), he("PIXI_PAINT_STEP", 96, "[trueos pixi widgets] paint-step publish-stats begin"), Ni(H_1, S_1, Ri(x_2), ki(k_1), Oi(k_1), X_1), he("PIXI_PAINT_STEP", 96, "[trueos pixi widgets] paint-step publish-stats done"), Lt()) {
                                he("PIXI_PAINT_STEP", 96, "[trueos pixi widgets] paint-step overlays begin");
                                var g = Ts(k_1);
                                window.__TRUEOS_PIXI_LAYOUT_TEXT_OVERLAYS__ = g, j_1 < 4 && (j_1 += 1, console.log("[trueos pixi widgets] layout-text-overlays count=".concat(g.length, " samples=").concat(Es(g)))), he("PIXI_PAINT_STEP", 96, "[trueos pixi widgets] paint-step overlays done");
                            }
                            Nt("main:paint:done"), he("PIXI_PAINT_STEP", 96, "[trueos pixi widgets] paint-step done");
                        } }, V_4 = function () { var g = window.__TRUEOS_PIXI_SCROLL_REPAINT_OWNER__ || "", T = window.__TRUEOS_PIXI_SCROLL_REPAINT_IFRAME_KEY__ || ""; Nt("main:scroll-paint:clamp"), G_2(), Nt("main:scroll-paint:content-position"); var v = ue(o_3, "__contentRoot"); if (qt(v, 0, -l.scroll.y), Nt("main:scroll-paint:scrollbar"), W_2(), window.__TRUEOS_PIXI_LAST_SCROLL_FAST_PATH__ = null, g === "root") {
                            var M = window.__pixiCapture, E = M && typeof M.objectId == "function" ? M.objectId.bind(M) : null;
                            E && (window.__TRUEOS_PIXI_LAST_SCROLL_FAST_PATH__ = { owner: "root", rootNode: E(i_1.stage), contentNode: E(v), contentY: -l.scroll.y, scrollbarNode: E(a_2), scrollbarVisible: l.scroll.track.h > 0 ? 1 : 0, trackX: l.scroll.track.x, trackY: l.scroll.track.y, trackW: l.scroll.track.w, trackH: l.scroll.track.h, thumbX: l.scroll.thumb.x, thumbY: l.scroll.thumb.y, thumbW: l.scroll.thumb.w, thumbH: l.scroll.thumb.h });
                        } if (Nt("main:scroll-paint:iframe-scrollbars"), tt_3(), g === "iframe" && T) {
                            var M = window.__pixiCapture, E = M && typeof M.objectId == "function" ? M.objectId.bind(M) : null, P = l.iframeScrollRoots.get(T), I = l.iframeScrollbarGraphics.get(T), O = l.iframeScroll.get(T);
                            E && P && I && O && (window.__TRUEOS_PIXI_LAST_SCROLL_FAST_PATH__ = { owner: "iframe", rootNode: E(i_1.stage), contentNode: E(P), contentY: -O.y, scrollbarNode: E(I), scrollbarVisible: O.track.h > 0 ? 1 : 0, trackX: O.track.h > 0 ? O.track.x - O.rect.x : 0, trackY: O.track.h > 0 ? O.track.y - O.rect.y : 0, trackW: O.track.w, trackH: O.track.h, thumbX: O.thumb.h > 0 ? O.thumb.x - O.rect.x : 0, thumbY: O.thumb.h > 0 ? O.thumb.y - O.rect.y : 0, thumbW: O.thumb.w, thumbH: O.thumb.h });
                        } Nt("main:scroll-paint:renderer-render"), i_1.renderer.render(i_1.stage), Ni(H_1, S_1, Ri(x_2), k_1 ? ki(k_1) : "", k_1 ? Oi(k_1) : ""), Nt("main:scroll-paint:done"); };
                        Lt() && (window.__TRUEOS_REPAINT_NOW__ = function () { var M; var g = window.__TRUEOS_PIXI_SCROLL_REPAINT_REQUIRED__ === !0, T = !g && window.__TRUEOS_PIXI_OVERLAY_REPAINT_REQUIRED__ === !0; window.__TRUEOS_PIXI_DIRTY__ = !1, window.__TRUEOS_PIXI_REPAINT_REQUIRED__ = !1, window.__TRUEOS_PIXI_SCROLL_REPAINT_REQUIRED__ = !1, window.__TRUEOS_PIXI_OVERLAY_REPAINT_REQUIRED__ = !1, g || (window.__TRUEOS_PIXI_LAST_SCROLL_FAST_PATH__ = null), !g && !T && (window.__TRUEOS_PIXI_LAST_OVERLAY_FAST_PATH__ = null), g || (window.__TRUEOS_PIXI_LAST_GRAPHICS_FAST_PATH__ = null); var v = Number((M = window.__TRUEOS_REPAINT_NOW_LOG_COUNT__) != null ? M : 0) || 0; v < 24 && (window.__TRUEOS_REPAINT_NOW_LOG_COUNT__ = v + 1, console.log("[trueos pixi widgets] repaint-now scrollOnly=".concat(g ? 1 : 0, " overlayOnly=").concat(T ? 1 : 0, " begin"))), g ? V_4() : T ? nt_1() : L_1(), window.__TRUEOS_PIXI_SCROLL_REPAINT_OWNER__ = "", window.__TRUEOS_PIXI_SCROLL_REPAINT_IFRAME_KEY__ = "", v < 24 && console.log("[trueos pixi widgets] repaint-now scrollOnly=".concat(g ? 1 : 0, " overlayOnly=").concat(T ? 1 : 0, " done")); });
                        K_1 = function () { Nt("main:layout-build"), Dt("[trueos pixi widgets] rerender layout-build begin"); var g = vs(x_2, window.innerWidth, window.innerHeight); Dt("[trueos pixi widgets] rerender layout-build done"), Dt("[trueos pixi widgets] rerender prepixi begin"), X_1 = es(D_1.source, x_2, g, window.innerWidth, window.innerHeight), Dt("[trueos pixi widgets] rerender prepixi done"), Nt("main:layout-commit"), k_1 = g, Lt() && (window.__TRUEOS_PIXI_LAST_LAYOUT__ = g, window.__TRUEOS_PIXI_LAYOUT_TEXT_OVERLAYS__ = []), Dt("[trueos pixi widgets] rerender stats begin"), S_1 = as(g), Dt("[trueos pixi widgets] rerender stats done"), Dt("[trueos pixi widgets] rerender scroll-height begin"), l.scroll.contentHeight = Os(g), l.scroll.viewportHeight = window.innerHeight, Dt("[trueos pixi widgets] rerender paint begin"), L_1(), Dt("[trueos pixi widgets] rerender paint done"); };
                        xn = function () { K_1(); };
                        U_2 = !1, gt_2 = !1, F_2 = function () { if (Lt()) {
                            window.__TRUEOS_PIXI_DIRTY__ = !0;
                            return;
                        } gt_2 || U_2 || (gt_2 = !0, requestAnimationFrame(function () { gt_2 = !1, i_1.renderer.render(i_1.stage); })); };
                        wt = function () { if (!U_2) {
                            if (Lt()) {
                                window.__TRUEOS_PIXI_DIRTY__ = !0, window.__TRUEOS_PIXI_REPAINT_REQUIRED__ = !0;
                                return;
                            }
                            U_2 = !0, requestAnimationFrame(function () { U_2 = !1, L_1(); });
                        } }, Xt = function () { if (!U_2) {
                            if (Lt()) {
                                window.__TRUEOS_PIXI_DIRTY__ = !0, window.__TRUEOS_PIXI_OVERLAY_REPAINT_REQUIRED__ = !0;
                                return;
                            }
                            U_2 = !0, requestAnimationFrame(function () { U_2 = !1, nt_1(); });
                        } }, Nt("main:first-rerender"), K_1(), Nt("main:cursor-setup");
                        $ = 2, ft = 10, q_2 = Lt();
                        h(m_2, { half: ft, strokeWidth: $, color: te(zt) }), h(d_1, { half: ft, strokeWidth: $, color: te(Vt) }), h(b_1, { half: ft, strokeWidth: $, color: te(Qt) });
                        ot_1 = 2;
                        if (h(y_1, { half: ft, strokeWidth: $, color: te(ot_1) }), l.userCursorPos.set(zt, { x: i_1.renderer.width * .25, y: i_1.renderer.height * .5 }), l.userCursorPos.set(Vt, { x: i_1.renderer.width * .25 + 40, y: i_1.renderer.height * .5 + 20 }), l.userCursorPos.set(Qt, { x: i_1.renderer.width * .25 + 80, y: i_1.renderer.height * .5 + 40 }), m_2.visible = !q_2, d_1.visible = !q_2, b_1.visible = !q_2, !q_2) {
                            g = l.userCursorPos.get(zt), T = l.userCursorPos.get(Vt), v = l.userCursorPos.get(Qt);
                            m_2.position.set(g.x, g.y), d_1.position.set(T.x, T.y), b_1.position.set(v.x, v.y);
                        }
                        y_1.visible = !q_2 && l.virtualCursor.enabled;
                        bt_1 = function () { if (q_2) {
                            m_2.visible = !1, d_1.visible = !1, b_1.visible = !1, y_1.visible = !1;
                            return;
                        } var g = l.userCursorPos.get(zt), T = l.userCursorPos.get(Vt), v = l.userCursorPos.get(Qt); g && (m_2.visible = !0, m_2.position.set(g.x, g.y)), T && (d_1.visible = !0, d_1.position.set(T.x, T.y)), v && (b_1.visible = !0, b_1.position.set(v.x, v.y)); var M = function (E, P) { var I = null, O = null; for (var J = l.hoverRects.length - 1; J >= 0; J--) {
                            var Q = l.hoverRects[J];
                            if (E >= Q.x && E <= Q.x + Q.w && P >= Q.y && P <= Q.y + Q.h) {
                                I = Q.key, O = Q.cursor;
                                break;
                            }
                        } return { hitKey: I, hitCursor: O }; }; if (g) {
                            var _a = M(g.x, g.y), E = _a.hitKey, P = _a.hitCursor;
                            l.hoveredKeyByPointer.set(zt, E), l.hoveredCursorByPointer.set(zt, P);
                            var I = l.textDrags.has(zt) || l.sliderDrags.has(zt) || l.dialogDrags.has(zt);
                            m_2.rotation = P != null || I ? Math.PI / 4 : 0;
                        } if (T) {
                            var _b = M(T.x, T.y), E = _b.hitKey, P = _b.hitCursor;
                            l.hoveredKeyByPointer.set(Vt, E), l.hoveredCursorByPointer.set(Vt, P);
                            var I = l.textDrags.has(Vt) || l.sliderDrags.has(Vt) || l.dialogDrags.has(Vt);
                            d_1.rotation = P != null || I ? Math.PI / 4 : 0;
                        } if (v) {
                            var _c = M(v.x, v.y), E = _c.hitKey, P = _c.hitCursor;
                            l.hoveredKeyByPointer.set(Qt, E), l.hoveredCursorByPointer.set(Qt, P);
                            var I = l.textDrags.has(Qt) || l.sliderDrags.has(Qt) || l.dialogDrags.has(Qt);
                            b_1.rotation = P != null || I ? Math.PI / 4 : 0;
                        } F_2(); };
                        l.harness.enabled && setInterval(function () {
                            var e_67, _a, e_68, _b;
                            var g = l.harness.activeUserPointerId, T = g === zt ? Vt : g === Vt ? Qt : zt;
                            if (l.harness.activeUserPointerId = T, l.lastMouse.has) {
                                var Q = l.userCursorPos.get(g), Y = l.userCursorPos.get(T);
                                l.userCursorPos.set(T, { x: l.lastMouse.x, y: l.lastMouse.y }), Y ? l.userCursorPos.set(g, { x: Y.x, y: Y.y }) : Q && l.userCursorPos.set(g, { x: Q.x, y: Q.y });
                            }
                            var v = l.textDrags.size > 0, M = l.sliderDrags.size > 0, E = l.dialogDrags.size > 0, P = l.scroll.draggingPointerId != null, I = l.color.draggingPointerId != null, O = !1;
                            try {
                                for (var _c = __values(l.iframeScroll.values()), _d = _c.next(); !_d.done; _d = _c.next()) {
                                    var Q = _d.value;
                                    if (Q.draggingPointerId != null) {
                                        O = !0;
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
                            var J = v || M || E || P || I || O;
                            l.textDrags.delete(zt), l.textDrags.delete(Vt), l.textDrags.delete(Qt), l.sliderDrags.delete(zt), l.sliderDrags.delete(Vt), l.sliderDrags.delete(Qt), l.dialogDrags.delete(zt), l.dialogDrags.delete(Vt), l.dialogDrags.delete(Qt);
                            try {
                                for (var _f = __values([zt, Vt, Qt]), _g = _f.next(); !_g.done; _g = _f.next()) {
                                    var Q = _g.value;
                                    var Y = l.numberHolds.get(Q);
                                    Y && (Y.timeoutId != null && window.clearTimeout(Y.timeoutId), Y.intervalId != null && window.clearInterval(Y.intervalId), l.numberHolds.delete(Q));
                                }
                            }
                            catch (e_68_1) { e_68 = { error: e_68_1 }; }
                            finally {
                                try {
                                    if (_g && !_g.done && (_b = _f.return)) _b.call(_f);
                                }
                                finally { if (e_68) throw e_68.error; }
                            }
                            (l.scroll.draggingPointerId === zt || l.scroll.draggingPointerId === Vt || l.scroll.draggingPointerId === Qt) && (l.scroll.draggingPointerId = null), (l.color.draggingPointerId === zt || l.color.draggingPointerId === Vt || l.color.draggingPointerId === Qt) && (l.color.draggingPointerId = null), bt_1(), J && (wt == null || wt());
                        }, l.harness.periodMs), !q_2 && l.virtualCursor.enabled && i_1.ticker.add(function () { var P, I, O, J, Q; var g = Math.max(0, i_1.ticker.deltaMS) / 1e3; y_1.visible = !0, l.virtualCursor.t += g; var T = i_1.renderer.width * .75, v = i_1.renderer.height * .25, M = l.virtualCursor.t * l.virtualCursor.speed, E = l.virtualCursor.radius; l.virtualCursor.x = T + Math.cos(M) * E, l.virtualCursor.y = v + Math.sin(M) * E, y_1.position.set(l.virtualCursor.x, l.virtualCursor.y); {
                            var Y = ot_1, lt = l.virtualCursor.x, Z = l.virtualCursor.y, _t = null, Mt = null;
                            for (var at = l.hoverRects.length - 1; at >= 0; at--) {
                                var et = l.hoverRects[at];
                                if (lt >= et.x && lt <= et.x + et.w && Z >= et.y && Z <= et.y + et.h) {
                                    _t = et.key, Mt = et.cursor;
                                    break;
                                }
                            }
                            var xt = (P = l.hoveredKeyByPointer.get(Y)) != null ? P : null;
                            xt !== _t && (xt && ((O = (I = l.hoverHandlers.get(xt)) == null ? void 0 : I.out) == null || O.call(I)), _t && ((Q = (J = l.hoverHandlers.get(_t)) == null ? void 0 : J.over) == null || Q.call(J)), l.hoveredKeyByPointer.set(Y, _t)), l.hoveredCursorByPointer.set(Y, Mt);
                            var yt = l.textDrags.has(Y) || l.sliderDrags.has(Y) || l.dialogDrags.has(Y);
                            y_1.rotation = Mt != null || yt ? Math.PI / 4 : 0;
                        } }), l.virtualCursor.x = i_1.renderer.width * .75 + l.virtualCursor.radius, l.virtualCursor.y = i_1.renderer.height * .25, y_1.position.set(l.virtualCursor.x, l.virtualCursor.y), Lt() && L_1(), i_1.stage.on("pointerup", function (g) {
                            var e_69, _a;
                            var M, E, P;
                            var T = Bt(g), v = (E = (M = l.sliderDrags.get(T)) == null ? void 0 : M.key) != null ? E : null;
                            l.textDrags.delete(T), l.sliderDrags.delete(T), l.dialogDrags.delete(T), l.scroll.draggingPointerId === T && (l.scroll.draggingPointerId = null), l.color.draggingPointerId === T && (l.color.draggingPointerId = null);
                            try {
                                for (var _b = __values(l.iframeScroll.values()), _c = _b.next(); !_c.done; _c = _b.next()) {
                                    var I = _c.value;
                                    I.draggingPointerId === T && (I.draggingPointerId = null);
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
                                var I = l.numberHolds.get(T);
                                I && (I.timeoutId != null && window.clearTimeout(I.timeoutId), I.intervalId != null && window.clearInterval(I.intervalId), l.numberHolds.delete(T));
                            }
                            if (v) {
                                var I = (P = l.temporalYearOwners.get(v)) != null ? P : null;
                                if (I) {
                                    var O = l.temporals.get(I);
                                    O && O.openYear && (O.openYear = !1, l.temporals.set(I, O), wt == null || wt());
                                }
                            }
                            F_2();
                        }), i_1.stage.on("pointerupoutside", function (g) {
                            var e_70, _a;
                            var M, E, P;
                            var T = Bt(g), v = (E = (M = l.sliderDrags.get(T)) == null ? void 0 : M.key) != null ? E : null;
                            l.textDrags.delete(T), l.sliderDrags.delete(T), l.dialogDrags.delete(T), l.scroll.draggingPointerId === T && (l.scroll.draggingPointerId = null), l.color.draggingPointerId === T && (l.color.draggingPointerId = null);
                            try {
                                for (var _b = __values(l.iframeScroll.values()), _c = _b.next(); !_c.done; _c = _b.next()) {
                                    var I = _c.value;
                                    I.draggingPointerId === T && (I.draggingPointerId = null);
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
                                var I = l.numberHolds.get(T);
                                I && (I.timeoutId != null && window.clearTimeout(I.timeoutId), I.intervalId != null && window.clearInterval(I.intervalId), l.numberHolds.delete(T));
                            }
                            if (v) {
                                var I = (P = l.temporalYearOwners.get(v)) != null ? P : null;
                                if (I) {
                                    var O = l.temporals.get(I);
                                    O && O.openYear && (O.openYear = !1, l.temporals.set(I, O), wt == null || wt());
                                }
                            }
                            F_2();
                        }), a_2.on("pointerdown", function (g) { var Z, _t, Mt, xt, yt, at; if ((g == null ? void 0 : g.button) === 2)
                            return; var T = Bt(g); if (T <= 0)
                            return; var v = (_t = (Z = g.global) == null ? void 0 : Z.x) != null ? _t : 0, M = (xt = (Mt = g.global) == null ? void 0 : Mt.y) != null ? xt : 0, E = l.scroll.track, P = l.scroll.thumb; if (!(v >= E.x && v <= E.x + E.w && M >= E.y && M <= E.y + E.h))
                            return; var O = Math.max(0, l.scroll.contentHeight - l.scroll.viewportHeight); if (O <= .5)
                            return; if (v >= P.x && v <= P.x + P.w && M >= P.y && M <= P.y + P.h) {
                            l.scroll.draggingPointerId = T, l.scroll.dragOffsetY = M - P.y, (yt = g.stopPropagation) == null || yt.call(g);
                            return;
                        } var Q = Math.max(1, E.h - P.h), Y = Math.max(E.y, Math.min(E.y + Q, M - P.h / 2)), lt = (Y - E.y) / Q; l.scroll.y = Math.max(0, Math.min(O, lt * O)), l.scroll.draggingPointerId = T, l.scroll.dragOffsetY = M - Y, Lt() && (window.__TRUEOS_PIXI_SCROLL_REPAINT_REQUIRED__ = !0, window.__TRUEOS_PIXI_SCROLL_REPAINT_OWNER__ = "root", window.__TRUEOS_PIXI_SCROLL_REPAINT_IFRAME_KEY__ = ""), wt == null || wt(), (at = g.stopPropagation) == null || at.call(g); }), i_1.stage.on("pointermove", function (g) {
                            var e_71, _a;
                            var O, J, Q, Y, lt, Z, _t, Mt, xt, yt, at, et, dt, Ft, ht, Rt, jt, ne, le, ce, be, _e, ye, me, ct, Tt, Ut, St;
                            var T = Number((Q = (J = g == null ? void 0 : g.pointerId) != null ? J : (O = g == null ? void 0 : g.data) == null ? void 0 : O.pointerId) != null ? Q : 1);
                            if (String((Z = (lt = g == null ? void 0 : g.pointerType) != null ? lt : (Y = g == null ? void 0 : g.data) == null ? void 0 : Y.pointerType) != null ? Z : "").toLowerCase() === "mouse" || T === 1) {
                                var ut = (Mt = (_t = g.global) == null ? void 0 : _t.x) != null ? Mt : 0, rt = (yt = (xt = g.global) == null ? void 0 : xt.y) != null ? yt : 0;
                                l.lastMouse.x = ut, l.lastMouse.y = rt, l.lastMouse.has = !0, l.primaryMousePointerId = T;
                                var Et = l.harness.enabled ? l.harness.activeUserPointerId : T;
                                l.userCursorPos.set(Et, { x: ut, y: rt }), bt_1();
                            }
                            var E = Bt(g);
                            if (E <= 0)
                                return;
                            var P = !1, I = !1;
                            {
                                var ut = l.textDrags.get(E);
                                if (ut) {
                                    var rt = ut.key, Et = l.fieldBounds.get(rt), Ht = l.inputs.get(rt);
                                    if (Et && Ht && typeof Ht.value == "string") {
                                        var At = Et.isPassword ? "\u2022".repeat(Ht.value.length) : Ht.value, mt = Te(we(At, Math.max(0, Et.innerWidth), _1), Et.maxLines), Yt = ((et = (at = g.global) == null ? void 0 : at.x) != null ? et : 0) - Et.x - Et.innerLeft, Kt = ((Ft = (dt = g.global) == null ? void 0 : dt.y) != null ? Ft : 0) - Et.y - Et.innerTop, xe = Ae({ fullText: At, lines: mt, localX: Yt, localY: Kt, lineHeight: p_1, measure: _1 });
                                        Ht.selections || (Ht.selections = new Map), Ht.selections.set(E, { start: ut.anchor, end: xe }), P = !0;
                                    }
                                }
                            }
                            {
                                var ut = l.sliderDrags.get(E);
                                if (ut) {
                                    var rt = ut.key, Et = l.sliderBounds.get(rt);
                                    if (Et) {
                                        var At = ((Rt = (ht = g.global) == null ? void 0 : ht.x) != null ? Rt : 0) - Et.x, mt = Math.max(1, Et.w - Et.innerPad * 2), Yt = (At - Et.innerPad) / mt, Kt = Pe(l.sliders, rt, void 0);
                                        Kt.value = Math.max(0, Math.min(1, Yt)), P = !0;
                                    }
                                }
                            }
                            {
                                var ut = l.color.draggingPointerId;
                                if (ut != null && ut === E) {
                                    var rt = l.color.bounds;
                                    if (rt) {
                                        var Et = (ne = (jt = g.global) == null ? void 0 : jt.x) != null ? ne : 0, Ht = (ce = (le = g.global) == null ? void 0 : le.y) != null ? ce : 0, At = Et - rt.x, mt = Ht - rt.y, Yt = Hn({ lx: At, ly: mt, w: rt.w, h: rt.h });
                                        Yt && (l.color.rgb = Yt, l.color.pick = { x: At, y: mt }, P = !0);
                                    }
                                }
                            }
                            {
                                var ut = l.scroll.draggingPointerId;
                                if (ut != null && ut === E) {
                                    var rt = l.scroll.track, Et = l.scroll.thumb, Ht = Math.max(0, l.scroll.contentHeight - l.scroll.viewportHeight);
                                    if (Ht > .5 && rt.h > 0 && Et.h > 0) {
                                        var At = (_e = (be = g.global) == null ? void 0 : be.y) != null ? _e : 0, mt = Math.max(1, rt.h - Et.h), Kt = (Math.max(rt.y, Math.min(rt.y + mt, At - l.scroll.dragOffsetY)) - rt.y) / mt;
                                        l.scroll.y = Math.max(0, Math.min(Ht, Kt * Ht)), P = !0, I = !0, Lt() && (window.__TRUEOS_PIXI_SCROLL_REPAINT_OWNER__ = "root", window.__TRUEOS_PIXI_SCROLL_REPAINT_IFRAME_KEY__ = "");
                                    }
                                }
                            }
                            try {
                                for (var _b = __values(l.iframeScroll.entries()), _c = _b.next(); !_c.done; _c = _b.next()) {
                                    var _d = __read(_c.value, 2), ut = _d[0], rt = _d[1];
                                    if (rt.draggingPointerId == null || rt.draggingPointerId !== E)
                                        continue;
                                    var Et = Math.max(0, rt.contentHeight - rt.viewportHeight);
                                    if (Et <= .5 || rt.track.h <= 0 || rt.thumb.h <= 0)
                                        continue;
                                    var Ht = (me = (ye = g.global) == null ? void 0 : ye.y) != null ? me : 0, At = Math.max(1, rt.track.h - rt.thumb.h), Yt = (Math.max(rt.track.y, Math.min(rt.track.y + At, Ht - rt.dragOffsetY)) - rt.track.y) / At;
                                    rt.y = Math.max(0, Math.min(Et, Yt * Et)), P = !0, I = !0, Lt() && (window.__TRUEOS_PIXI_SCROLL_REPAINT_OWNER__ = "iframe", window.__TRUEOS_PIXI_SCROLL_REPAINT_IFRAME_KEY__ = ut);
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
                                var ut = l.dialogDrags.get(E);
                                if (ut) {
                                    var rt = cn(l.dialogs, ut.key), Et = (Tt = (ct = g.global) == null ? void 0 : ct.x) != null ? Tt : 0, Ht = (St = (Ut = g.global) == null ? void 0 : Ut.y) != null ? St : 0;
                                    rt.x = ut.originX + (Et - ut.startGX), rt.y = ut.originY + (Ht - ut.startGY);
                                    var At = l.dialogDragBounds.get(ut.key);
                                    At && (rt.x = Math.max(At.minX, Math.min(At.maxX, rt.x)), rt.y = Math.max(At.minY, Math.min(At.maxY, rt.y))), P = !0;
                                }
                            }
                            P && (I && Lt() && (window.__TRUEOS_PIXI_SCROLL_REPAINT_REQUIRED__ = !0), wt == null || wt());
                        }), Nt("main:input-listeners"), window.addEventListener("keydown", function (g) {
                            var Mt, xt, yt, at, et, dt, Ft;
                            var T = l.keyboardOwnerPointerId, v = (Mt = l.focusedKeyByPointer.get(T)) != null ? Mt : null;
                            if (!v)
                                return;
                            var M = l.inputs.get(v);
                            if (!M || typeof M.value != "string")
                                return;
                            if (M.selections || (M.selections = new Map), !M.selections.has(T)) {
                                var ht = M.value.length;
                                M.selections.set(T, { start: ht, end: ht });
                            }
                            var E = M.selections.get(T), P = M.value.length, I = function (ht) { return Math.max(0, Math.min(P, ht)); }, O = I((xt = E.start) != null ? xt : P), J = I((yt = E.end) != null ? yt : O);
                            E.start = O, E.end = J;
                            var Q = Math.min(O, J), Y = Math.max(O, J), lt = Q !== Y, Z = function (ht) { var Rt = Math.max(0, Math.min(M.value.length, ht)); E.start = Rt, E.end = Rt; }, _t = function (ht, Rt) { E.start = Math.max(0, Math.min(M.value.length, ht)), E.end = Math.max(0, Math.min(M.value.length, Rt)); };
                            if (g.key.toLowerCase() === "a" && (g.ctrlKey || g.metaKey)) {
                                _t(0, M.value.length), g.preventDefault(), L_1();
                                return;
                            }
                            if (g.key === "ArrowLeft" || g.key === "ArrowRight") {
                                var ht = g.key === "ArrowLeft" ? -1 : 1;
                                if (g.shiftKey) {
                                    var Rt = (at = E.start) != null ? at : P, jt = ((et = E.end) != null ? et : Rt) + ht;
                                    _t(Rt, jt);
                                }
                                else
                                    Z((lt ? Q : Y) + ht);
                                g.preventDefault(), K_1();
                                return;
                            }
                            if (g.key === "Home") {
                                g.shiftKey ? _t((dt = E.start) != null ? dt : P, 0) : Z(0), g.preventDefault(), K_1();
                                return;
                            }
                            if (g.key === "End") {
                                g.shiftKey ? _t((Ft = E.start) != null ? Ft : 0, M.value.length) : Z(M.value.length), g.preventDefault(), K_1();
                                return;
                            }
                            if (g.key === "Backspace") {
                                if (lt)
                                    M.value = M.value.slice(0, Q) + M.value.slice(Y), Z(Q);
                                else {
                                    var ht = Y;
                                    ht > 0 && (M.value = M.value.slice(0, ht - 1) + M.value.slice(ht), Z(ht - 1));
                                }
                                g.preventDefault(), K_1();
                                return;
                            }
                            if (g.key === "Enter") {
                                var ht = "\n";
                                if (lt)
                                    M.value = M.value.slice(0, Q) + ht + M.value.slice(Y), Z(Q + ht.length);
                                else {
                                    var Rt = Y;
                                    M.value = M.value.slice(0, Rt) + ht + M.value.slice(Rt), Z(Rt + ht.length);
                                }
                                g.preventDefault(), K_1();
                                return;
                            }
                            if (g.key === "Delete") {
                                if (lt)
                                    M.value = M.value.slice(0, Q) + M.value.slice(Y), Z(Q);
                                else {
                                    var ht = Y;
                                    ht < M.value.length && (M.value = M.value.slice(0, ht) + M.value.slice(ht + 1), Z(ht));
                                }
                                g.preventDefault(), K_1();
                                return;
                            }
                            if (g.key === "Escape") {
                                l.focusedKeyByPointer.set(T, null), K_1();
                                return;
                            }
                            if (g.key.length === 1 && !g.ctrlKey && !g.metaKey && !g.altKey) {
                                if (lt)
                                    M.value = M.value.slice(0, Q) + g.key + M.value.slice(Y), Z(Q + 1);
                                else {
                                    var ht = Y;
                                    M.value = M.value.slice(0, ht) + g.key + M.value.slice(ht), Z(ht + 1);
                                }
                                g.preventDefault(), K_1();
                            }
                        }), window.addEventListener("resize", function () { K_1(), y_1.visible = l.virtualCursor.enabled; }), Nt("main:done"), r && (window.__TRUEOS_PIXI_APP_READY__ = !0, window.__TRUEOS_PIXI_APP_ERROR__ = "", window.__TRUEOS_PIXI_APP_PHASE__ = "ready");
                        return [3 /*break*/, 10];
                    case 9:
                        n_3 = _d.sent();
                        window.__TRUEOS_PIXI_APP_READY__ = !1, window.__TRUEOS_PIXI_APP_ERROR__ = Ki(n_3);
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
    Hs().then(function () { window.__TRUEOS_PIXI_APP_ERROR__ || (window.__TRUEOS_PIXI_APP_READY__ = !0, window.__TRUEOS_PIXI_APP_ERROR__ = "", window.__TRUEOS_PIXI_APP_PHASE__ = "ready"); }).catch(function (t) { var n; window.__TRUEOS_PIXI_APP_READY__ = !1, window.__TRUEOS_PIXI_APP_ERROR__ = Ki(t), console.error(t); var e = document.createElement("pre"); e.textContent = String((n = t == null ? void 0 : t.stack) != null ? n : t), document.body.appendChild(e); });
})();
