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
    stage.on("pointerdown", function () {});
    stage.removeAllListeners("pointerdown");

    var graphics = new pixi.Graphics();
    if (typeof graphics.clear === "function") {
        graphics.clear();
    }
    if (typeof graphics.rect === "function") {
        graphics.rect(8, 12, 96, 32);
    }
    if (typeof graphics.fill === "function") {
        graphics.fill({ color: 0x2d7ff9, alpha: 1 });
    }
    if (typeof graphics.rect === "function") {
        graphics.rect(112, 12, 48, 32);
    }
    if (typeof graphics.fill === "function") {
        graphics.fill({ color: 0xffb020, alpha: 0.9 });
    }
    if (typeof graphics.rect === "function") {
        graphics.rect(8, 52, 152, 24);
    }
    if (typeof graphics.stroke === "function") {
        graphics.stroke({ color: 0xffffff, alpha: 0.85, width: 2 });
    }
    if (typeof graphics.rect === "function") {
        graphics.rect(8, 86, 32, 24);
    }
    if (typeof graphics.fill === "function") {
        graphics.fill({ color: 0x22c55e, alpha: 1 });
    }
    if (typeof graphics.stroke === "function") {
        graphics.stroke({ color: 0x111111, alpha: 1, width: 1 });
    }
    if (typeof graphics.circle === "function") {
        graphics.circle(190, 28, 12);
    }
    if (typeof graphics.fill === "function") {
        graphics.fill({ color: 0xffb020, alpha: 0.9 });
    }
    if (typeof graphics.moveTo === "function") {
        graphics.moveTo(176, 70);
    }
    if (typeof graphics.lineTo === "function") {
        graphics.lineTo(216, 92);
    }
    if (typeof graphics.stroke === "function") {
        graphics.stroke({ color: 0x22c55e, alpha: 1, width: 2 });
    }
    stage.addChild(graphics);

    var text = new pixi.Text({ text: "TRUEOS UI3 Pixi" });
    stage.addChildAt(text, 1);
    stage.setChildIndex(graphics, 0);

    var scratch = new pixi.Container();
    scratch.addChild(new pixi.Graphics());
    scratch.removeChildren();

    var renderCall = G.__trueosRender(stage);

    G.__trueosPixiServiceStage = stage;
    G.__trueosPixiServiceStatus = {
        ready: 1,
        version: String(pixi.VERSION || ""),
        children: stage.children.length,
        graphicsRect: typeof graphics.rect === "function" ? 1 : 0,
        graphicsFill: typeof graphics.fill === "function" ? 1 : 0,
        graphicsStroke: typeof graphics.stroke === "function" ? 1 : 0,
        graphicsCircle: typeof graphics.circle === "function" ? 1 : 0,
        graphicsLine: typeof graphics.lineTo === "function" ? 1 : 0,
        renderCall: Number(renderCall) || 0,
    };
    G.__trueosPixiServiceReady = 1;
})(typeof globalThis !== "undefined" ? globalThis : this);
