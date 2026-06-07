(function (G) {
    "use strict";

    if (!G.navigator) {
        G.navigator = { userAgent: "TRUEOS QuickJS" };
    } else if (typeof G.navigator.userAgent !== "string") {
        G.navigator.userAgent = "TRUEOS QuickJS";
    }

    if (!G.performance) {
        G.performance = { now: function now() { return 0; } };
    }

    if (!G.self) {
        G.self = G;
    }

    if (!G.window) {
        G.window = G;
    }
})(typeof globalThis !== "undefined" ? globalThis : this);
