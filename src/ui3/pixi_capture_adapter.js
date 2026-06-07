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

    function wrapCtor(name, kind, after) {
        var Native = pixi[name];
        if (typeof Native !== "function") {
            return;
        }
        function Wrapped() {
            var args = Array.prototype.slice.call(arguments);
            var self = Reflect.construct(Native, args, new.target || Wrapped);
            id(self, kind);
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

    wrapCtor("Container", "Container");
    wrapCtor("Graphics", "Graphics");
    wrapCtor("Text", "Text", function (obj, args) {
        emit("text", id(obj, "Text"), textFromArg(args[0], obj));
    });

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
    patch(gp, "circle", function (node, args) {
        emit("circle", id(node, "Graphics"), num(args[0], 0), num(args[1], 0), num(args[2], 0));
    });
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

    G.__trueosPixiCaptureReady = 1;
})(typeof globalThis !== "undefined" ? globalThis : this);
