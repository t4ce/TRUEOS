import { createSVGWindow } from './dom-lite.mjs';
import * as svgjs from './svg.esm.mjs';

const trueosSvgWindow = createSVGWindow();
svgjs.registerWindow(trueosSvgWindow, trueosSvgWindow.document);

export const window = trueosSvgWindow;
export const document = trueosSvgWindow.document;
export const createWindow = createSVGWindow;

export * from './svg.esm.mjs';
export default svgjs;
