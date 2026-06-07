(function (G) {
    "use strict";

    var pixi = G.PIXI;
    var emit = G.__trueosPixiOp;
    if (!pixi || typeof emit !== "function") {
        throw new Error("ui3 pixi capture: missing PIXI or __trueosPixiOp");
    }

    var nextId = 1;

    function own(obj, key, value) {
        Object.defineProperty(obj, key, {
            value: value,
            enumerable: false,
            configurable: false,
            writable: false,
        });
    }

    function id(obj, kind) {
        if (!obj) {
            return 0;
        }
        if (!obj.__trueosPixiId) {
            own(obj, "__trueosPixiId", nextId++);
        }
        if (kind && obj.__trueosPixiKind !== kind) {
            Object.defineProperty(obj, "__trueosPixiKind", {
                value: kind,
                enumerable: false,
                configurable: true,
                writable: true,
            });
            emit("node", obj.__trueosPixiId, kind);
        }
        return obj.__trueosPixiId;
    }

    function kindOf(obj) {
        return obj && obj.__trueosPixiKind ? obj.__trueosPixiKind : "Container";
    }

    function num(value, fallback) {
        var out = Number(value);
        return Number.isFinite(out) ? out : fallback;
    }

    function color(value) {
        if (typeof value === "number") {
            return value >>> 0;
        }
        if (value && typeof value.color === "number") {
            return value.color >>> 0;
        }
        return 0xffffff;
    }

    function alpha(value) {
        if (value && typeof value.alpha === "number") {
            return num(value.alpha, 1);
        }
        return 1;
    }

    function strokeWidth(value) {
        if (typeof value === "number") {
            return 1;
        }
        if (value && typeof value.width === "number") {
            return num(value.width, 1);
        }
        return 1;
    }

    function textFromArg(arg, obj) {
        if (typeof arg === "string") {
            return arg;
        }
        if (arg && typeof arg.text !== "undefined") {
            return String(arg.text);
        }
        if (obj && typeof obj.text !== "undefined") {
            return String(obj.text);
        }
        return "";
    }

    function emitPosition(obj) {
        if (!obj || !obj.position) {
            return;
        }
        emit("position", id(obj, kindOf(obj)), num(obj.position.x, 0), num(obj.position.y, 0));
    }

    function patchSetters(proto) {
        if (!proto || proto.__trueosPixiSettersPatched) {
            return;
        }
        Object.defineProperty(proto, "__trueosPixiSettersPatched", {
            value: true,
            enumerable: false,
            configurable: true,
            writable: true,
        });

        var visibleDesc = Object.getOwnPropertyDescriptor(proto, "visible");
        if (visibleDesc && (visibleDesc.set || visibleDesc.get)) {
            Object.defineProperty(proto, "visible", {
                enumerable: visibleDesc.enumerable,
                configurable: true,
                get: function () {
                    return visibleDesc.get ? visibleDesc.get.call(this) : visibleDesc.value;
                },
                set: function (value) {
                    emit("visible", id(this, kindOf(this)), value ? 1 : 0);
                    if (visibleDesc.set) {
                        visibleDesc.set.call(this, value);
                    } else {
                        visibleDesc.value = value;
                    }
                },
            });
        }
    }

    function patchPosition(obj) {
        var p = obj && obj.position;
        if (!p || p.__trueosPixiPositionPatched) {
            return;
        }
        Object.defineProperty(p, "__trueosPixiPositionPatched", {
            value: true,
            enumerable: false,
            configurable: true,
            writable: true,
        });

        if (typeof p.set === "function") {
            var setOrig = p.set;
            p.set = function () {
                var out = setOrig.apply(this, arguments);
                emitPosition(obj);
                return out;
            };
        }
        if (typeof p.copyFrom === "function") {
            var copyOrig = p.copyFrom;
            p.copyFrom = function () {
                var out = copyOrig.apply(this, arguments);
                emitPosition(obj);
                return out;
            };
        }
    }

    function wrapCtor(name, kind, after) {
        var Native = pixi[name];
        if (typeof Native !== "function") {
            return;
        }
        function Wrapped() {
            var args = Array.prototype.slice.call(arguments);
            var self = Reflect.construct(Native, args, new.target || Wrapped);
            id(self, kind);
            patchPosition(self);
            emitPosition(self);
            if (after) {
                after(self, args);
            }
            return self;
        }
        Object.setPrototypeOf(Wrapped, Native);
        Wrapped.prototype = Native.prototype;
        pixi[name] = Wrapped;
    }

    function patch(proto, name, fn) {
        if (!proto || typeof proto[name] !== "function") {
            return;
        }
        var orig = proto[name];
        proto[name] = function () {
            fn(this, arguments);
            return orig.apply(this, arguments);
        };
    }

    function patchGetterSetter(proto, name, fn) {
        if (!proto) {
            return;
        }
        var desc = Object.getOwnPropertyDescriptor(proto, name);
        if (!desc || (!desc.set && !desc.writable)) {
            return;
        }
        Object.defineProperty(proto, name, {
            enumerable: desc.enumerable,
            configurable: true,
            get: function () {
                return desc.get ? desc.get.call(this) : desc.value;
            },
            set: function (value) {
                fn(this, value);
                if (desc.set) {
                    desc.set.call(this, value);
                } else {
                    desc.value = value;
                }
            },
        });
    }

    function patchRendererRender(renderer) {
        if (!renderer || typeof renderer.render !== "function" || renderer.__trueosPixiRenderPatched) {
            return;
        }
        var orig = renderer.render;
        renderer.render = function (root) {
            var renderRoot = root || this.stage || (G.__trueosPixiApplication && G.__trueosPixiApplication.stage);
            if (renderRoot && typeof G.__trueosRender === "function") {
                G.__trueosRender(renderRoot);
            }
            return orig.apply(this, arguments);
        };
        Object.defineProperty(renderer, "__trueosPixiRenderPatched", {
            value: true,
            enumerable: false,
            configurable: true,
            writable: true,
        });
    }

    patchSetters(pixi.Container && pixi.Container.prototype);
    patchSetters(pixi.Graphics && pixi.Graphics.prototype);
    patchSetters(pixi.Text && pixi.Text.prototype);

    wrapCtor("Container", "Container");
    wrapCtor("Graphics", "Graphics");
    wrapCtor("Text", "Text", function (obj, args) {
        emit("text", id(obj, "Text"), textFromArg(args[0], obj));
    });

    if (pixi.Application && pixi.Application.prototype && typeof pixi.Application.prototype.init === "function") {
        var appInitOrig = pixi.Application.prototype.init;
        pixi.Application.prototype.init = function () {
            var app = this;
            var out = appInitOrig.apply(this, arguments);
            return Promise.resolve(out).then(function (value) {
                G.__trueosPixiApplication = app;
                patchRendererRender(app.renderer);
                return value;
            });
        };
    }

    var cp = pixi.Container && pixi.Container.prototype;
    patch(cp, "addChild", function (parent, args) {
        var parentId = id(parent, "Container");
        for (var i = 0; i < args.length; i++) {
            emit("addChild", parentId, id(args[i], kindOf(args[i])));
        }
    });
    patch(cp, "addChildAt", function (parent, args) {
        emit("addChildAt", id(parent, "Container"), id(args[0], kindOf(args[0])), num(args[1], 0));
    });
    patch(cp, "setChildIndex", function (parent, args) {
        emit("setChildIndex", id(parent, "Container"), id(args[0], kindOf(args[0])), num(args[1], 0));
    });
    patch(cp, "removeChild", function (parent, args) {
        var parentId = id(parent, "Container");
        for (var i = 0; i < args.length; i++) {
            emit("removeChild", parentId, id(args[i], kindOf(args[i])));
        }
    });
    patch(cp, "removeFromParent", function (node) {
        emit("removeFromParent", id(node, kindOf(node)));
    });
    patch(cp, "removeChildren", function (parent) {
        emit("removeChildren", id(parent, "Container"));
    });
    patch(cp, "on", function (node, args) {
        emit("listen", id(node, kindOf(node)), String(args[0] || ""));
    });
    patch(cp, "removeAllListeners", function (node) {
        emit("removeAllListeners", id(node, kindOf(node)));
    });

    var gp = pixi.Graphics && pixi.Graphics.prototype;
    patch(gp, "clear", function (node) {
        emit("clear", id(node, "Graphics"));
    });
    patch(gp, "rect", function (node, args) {
        emit("rect", id(node, "Graphics"), num(args[0], 0), num(args[1], 0), num(args[2], 0), num(args[3], 0));
    });
    patch(gp, "roundRect", function (node, args) {
        emit("rect", id(node, "Graphics"), num(args[0], 0), num(args[1], 0), num(args[2], 0), num(args[3], 0));
    });
    patch(gp, "circle", function (node, args) {
        emit("circle", id(node, "Graphics"), num(args[0], 0), num(args[1], 0), num(args[2], 0));
    });
    patch(gp, "ellipse", function (node, args) {
        emit("circle", id(node, "Graphics"), num(args[0], 0), num(args[1], 0), Math.max(num(args[2], 0), num(args[3], 0)));
    });
    patch(gp, "poly", function (node, args) {
        var points = args[0];
        if (!Array.isArray(points) || points.length < 2) {
            return;
        }
        emit("moveTo", id(node, "Graphics"), num(points[0], 0), num(points[1], 0));
        for (var i = 2; i + 1 < points.length; i += 2) {
            emit("lineTo", id(node, "Graphics"), num(points[i], 0), num(points[i + 1], 0));
        }
    });
    patch(gp, "closePath", function () {});
    patch(gp, "moveTo", function (node, args) {
        emit("moveTo", id(node, "Graphics"), num(args[0], 0), num(args[1], 0));
    });
    patch(gp, "lineTo", function (node, args) {
        emit("lineTo", id(node, "Graphics"), num(args[0], 0), num(args[1], 0));
    });
    patch(gp, "fill", function (node, args) {
        var style = args[0];
        emit("fill", id(node, "Graphics"), color(style), alpha(style));
    });
    patch(gp, "stroke", function (node, args) {
        var style = args[0];
        emit("stroke", id(node, "Graphics"), color(style), alpha(style), strokeWidth(style));
    });

    var tp = pixi.Text && pixi.Text.prototype;
    patchGetterSetter(tp, "text", function (node, value) {
        emit("text", id(node, "Text"), String(value == null ? "" : value));
    });
    patchGetterSetter(tp, "style", function (node, value) {
        emit("textFill", id(node, "Text"), color(value && value.fill), alpha(value && value.fill));
    });

    G.__trueosPixiCaptureReady = 1;
})(typeof globalThis !== "undefined" ? globalThis : this);
