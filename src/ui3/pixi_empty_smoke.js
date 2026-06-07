(function (G) {
    "use strict";

    var pixi = G.PIXI;
    if (!pixi) {
        throw new Error("ui3 pixi smoke: PIXI global missing");
    }
    if (typeof pixi.Container !== "function") {
        throw new Error("ui3 pixi smoke: PIXI.Container missing");
    }
    if (typeof pixi.Graphics !== "function") {
        throw new Error("ui3 pixi smoke: PIXI.Graphics missing");
    }
    if (typeof pixi.Text !== "function") {
        throw new Error("ui3 pixi smoke: PIXI.Text missing");
    }
    if (typeof G.__trueosRender !== "function") {
        throw new Error("ui3 pixi smoke: __trueosRender missing");
    }

    var stage = new pixi.Container();
    var graphics = new pixi.Graphics();
    if (typeof graphics.rect === "function") {
        graphics.rect(1, 2, 3, 4);
    }
    if (typeof graphics.fill === "function") {
        graphics.fill({ color: 0xffffff, alpha: 1 });
    }
    stage.addChild(graphics);

    var text = new pixi.Text({ text: "TRUEOS UI3 Pixi" });
    stage.addChild(text);

    var renderCall = G.__trueosRender(stage);

    G.__trueosPixiServiceStage = stage;
    G.__trueosPixiServiceStatus = {
        ready: 1,
        version: String(pixi.VERSION || ""),
        children: stage.children.length,
        graphicsRect: typeof graphics.rect === "function" ? 1 : 0,
        graphicsFill: typeof graphics.fill === "function" ? 1 : 0,
        renderCall: Number(renderCall) || 0,
    };
    G.__trueosPixiServiceReady = 1;
})(typeof globalThis !== "undefined" ? globalThis : this);
