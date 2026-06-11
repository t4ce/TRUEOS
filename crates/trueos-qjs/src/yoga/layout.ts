import { defaultTheme } from '../theme';
import { SCROLLBAR_PAD } from '../runtimeConfig';
import type { LayoutBox, RenderNode } from '../renderTypes';
import { applyYogaDefaultsProgressOrMeter } from '../widgets/progressMeter';
import { applyYogaDefaultsSlider, createYogaNodeForSliderLabel } from '../widgets/slider';
import { getEffectiveDetailsChildren } from '../widgets/detailsSummary';
import { applyYogaDefaultsDetails, applyYogaDefaultsSummary } from '../widgets/detailsSummary';
import { applyYogaDefaultsHr } from '../widgets/hr';
import { applyYogaDefaultsButton } from '../widgets/button';
import { applyYogaDefaultsCell, applyYogaDefaultsTable, applyYogaDefaultsTr } from '../widgets/table';
import { applyYogaDefaultsHeading, isHeadingTag } from '../widgets/headings';
import { applyYogaDefaultsImg } from '../widgets/img';
import { applyYogaDefaultsSvg } from '../widgets/svgElement';
import { applyYogaDefaultsCanvas } from '../widgets/canvasElement';
import { applyYogaDefaultsIframe } from '../widgets/iframe';
import { applyYogaDefaultsInput } from '../widgets/input';
import { applyYogaDefaultsTextarea } from '../widgets/textarea';
import { applyYogaDefaultsBarrow } from '../widgets/barrow';
import { applyYogaDefaultsSearchButton, applyYogaDefaultsSearchRow } from '../widgets/search';
import { applyYogaDefaultsDialog } from '../widgets/dialog';
import { applyYogaDefaultsNumber } from '../widgets/number';
import { applyYogaDefaultsColor } from '../widgets/color';
import { applyYogaDefaultsSelect } from '../widgets/select';
import { applyYogaDefaultsTemporalInput } from '../widgets/temporal';

export type LayoutBuildContext = {
  Yoga: any;
  detailsOpen: Map<string, boolean>;
  normalizeWhitespace: (text: string) => string;
  onMeasureText?: () => void;
  setTrueosLayoutStep?: (step: string) => void;
};

function createTextMeasurer(
  font: string,
  normalizeWhitespace: (text: string) => string,
  onMeasureText?: () => void
) {
  const canvas = document.createElement('canvas');
  const ctx = canvas.getContext('2d');
  if (!ctx) throw new Error('2D canvas not available');
  ctx.font = font;

  const pxAt = font.indexOf('px');
  let digitStart = pxAt;
  while (digitStart > 0) {
    const c = font.charCodeAt(digitStart - 1);
    if (c < 48 || c > 57) break;
    digitStart -= 1;
  }
  const fontSize = pxAt > digitStart ? Number(font.slice(digitStart, pxAt)) : 16;
  const lineHeight = Math.ceil(fontSize * 1.25);

  return {
    measure(text: string, maxWidth?: number) {
      onMeasureText?.();
      const words = normalizeWhitespace(text).split(' ').filter(Boolean);
      if (words.length === 0) return { width: 0, height: lineHeight, lines: [''] };

      const lines: string[] = [];
      let current = '';
      for (const word of words) {
        const next = current ? `${current} ${word}` : word;
        const nextWidth = ctx.measureText(next).width;
        const limit = maxWidth ?? Number.POSITIVE_INFINITY;
        if (nextWidth <= limit || !current) {
          current = next;
        } else {
          lines.push(current);
          current = word;
        }
      }
      if (current) lines.push(current);

      const width = Math.min(
        Math.max(...lines.map((line) => ctx.measureText(line).width)),
        maxWidth ?? Number.POSITIVE_INFINITY
      );
      const height = lines.length * lineHeight;

      return { width: Math.ceil(width), height: Math.ceil(height), lines };
    },
    lineHeight,
    font,
  };
}

export function createCaptureOnlyYoga() {
  const EDGE_LEFT = 0;
  const EDGE_TOP = 1;
  const EDGE_RIGHT = 2;
  const EDGE_BOTTOM = 3;
  const FLEX_DIRECTION_COLUMN = 0;
  const FLEX_DIRECTION_ROW = 1;
  const MEASURE_MODE_UNDEFINED = 0;

  class Node {
    children: Node[];
    measureFunc: ((width: number, widthMode: number) => { width: number; height: number }) | null;
    paddingLeft: number;
    paddingTop: number;
    paddingRight: number;
    paddingBottom: number;
    marginLeft: number;
    marginTop: number;
    marginRight: number;
    marginBottom: number;
    width: number;
    height: number;
    minWidth: number;
    minHeight: number;
    flexDirection: number;
    computed: { left: number; top: number; width: number; height: number };

    constructor() {
      this.children = [];
      this.measureFunc = null;
      this.paddingLeft = 0;
      this.paddingTop = 0;
      this.paddingRight = 0;
      this.paddingBottom = 0;
      this.marginLeft = 0;
      this.marginTop = 0;
      this.marginRight = 0;
      this.marginBottom = 0;
      this.width = 0;
      this.height = 0;
      this.minWidth = 0;
      this.minHeight = 0;
      this.flexDirection = FLEX_DIRECTION_COLUMN;
      this.computed = { left: 0, top: 0, width: 0, height: 0 };
    }

    static create() {
      return new Node();
    }

    setMeasureFunc(fn: Node['measureFunc']) { this.measureFunc = fn; }
    setMargin(edge: number, value: number) {
      const v = Number(value) || 0;
      if (edge === EDGE_LEFT) this.marginLeft = v;
      else if (edge === EDGE_TOP) this.marginTop = v;
      else if (edge === EDGE_RIGHT) this.marginRight = v;
      else if (edge === EDGE_BOTTOM) this.marginBottom = v;
    }
    setPadding(edge: number, value: number) {
      const v = Number(value) || 0;
      if (edge === EDGE_LEFT) this.paddingLeft = v;
      else if (edge === EDGE_TOP) this.paddingTop = v;
      else if (edge === EDGE_RIGHT) this.paddingRight = v;
      else if (edge === EDGE_BOTTOM) this.paddingBottom = v;
    }
    setFlexDirection(value: number) { this.flexDirection = value; }
    setAlignItems(_value: number) {}
    setJustifyContent(_value: number) {}
    setFlexWrap(_value: number) {}
    setFlexGrow(_value: number) {}
    setFlexShrink(_value: number) {}
    setAlignSelf(_value: number) {}
    setPositionType(_value: number) {}
    setPosition(_edge: number, _value: number) {}
    setWidth(value: number) { this.width = Math.max(0, Number(value) || 0); }
    setHeight(value: number) { this.height = Math.max(0, Number(value) || 0); }
    setMinWidth(value: number) { this.minWidth = Math.max(0, Number(value) || 0); }
    setMinHeight(value: number) { this.minHeight = Math.max(0, Number(value) || 0); }
    insertChild(child: Node, index: number) {
      this.children.splice(Math.max(0, Math.min(index, this.children.length)), 0, child);
    }
    getChildCount() { return this.children.length; }
    getComputedLeft() { return this.computed.left; }
    getComputedTop() { return this.computed.top; }
    getComputedWidth() { return this.computed.width; }
    getComputedHeight() { return this.computed.height; }
    freeRecursive() {}

    calculateLayout(width = this.width, height = this.height) {
      this.layout(0, 0, Math.max(1, Number(width) || this.width || 1), Math.max(1, Number(height) || this.height || 1));
    }

    private layout(x: number, y: number, availableW: number, availableH: number) {
      const padX = this.paddingLeft + this.paddingRight;
      const padY = this.paddingTop + this.paddingBottom;
      const ownW = Math.max(this.minWidth, this.width || availableW);
      let ownH = Math.max(this.minHeight, this.height || 0);

      this.computed.left = x;
      this.computed.top = y;
      this.computed.width = ownW;

      if (this.measureFunc) {
        const measured = this.measureFunc(Math.max(0, ownW - padX), MEASURE_MODE_UNDEFINED);
        ownH = Math.max(ownH, Math.ceil(Number(measured.height) || 0) + padY);
        this.computed.height = ownH;
        return;
      }

      if (this.flexDirection === FLEX_DIRECTION_ROW) {
        let cx = this.paddingLeft;
        let rowH = 0;
        for (const child of this.children) {
          const childW = child.width || child.minWidth || Math.max(24, (ownW - padX) / Math.max(1, this.children.length));
          child.layout(cx + child.marginLeft, this.paddingTop + child.marginTop, childW, availableH);
          cx += child.computed.width + child.marginLeft + child.marginRight;
          rowH = Math.max(rowH, child.computed.height + child.marginTop + child.marginBottom);
        }
        ownH = Math.max(ownH, rowH + padY);
      } else {
        let cy = this.paddingTop;
        for (const child of this.children) {
          const childW = Math.max(0, ownW - padX - child.marginLeft - child.marginRight);
          child.layout(this.paddingLeft + child.marginLeft, cy + child.marginTop, childW, availableH);
          cy += child.computed.height + child.marginTop + child.marginBottom;
        }
        ownH = Math.max(ownH, cy + this.paddingBottom);
      }
      this.computed.height = Math.max(this.minHeight, ownH);
    }
  }

  return {
    Node,
    EDGE_LEFT,
    EDGE_TOP,
    EDGE_RIGHT,
    EDGE_BOTTOM,
    FLEX_DIRECTION_COLUMN,
    FLEX_DIRECTION_ROW,
    FLEX_DIRECTION_ROW_REVERSE: FLEX_DIRECTION_ROW,
    ALIGN_STRETCH: 0,
    ALIGN_CENTER: 1,
    ALIGN_FLEX_START: 2,
    JUSTIFY_CENTER: 0,
    JUSTIFY_FLEX_START: 1,
    JUSTIFY_SPACE_BETWEEN: 2,
    WRAP_WRAP: 1,
    WRAP_NO_WRAP: 0,
    POSITION_TYPE_ABSOLUTE: 1,
    DIRECTION_LTR: 0,
    MEASURE_MODE_UNDEFINED,
  };
}

export async function loadYoga(captureOnly: boolean): Promise<any> {
  return captureOnly ? createCaptureOnlyYoga() : (await import('yoga-layout')).default;
}

export function buildLayoutTree(
  renderNodes: RenderNode[],
  viewportWidth: number,
  viewportHeight: number,
  ctx: LayoutBuildContext
): LayoutBox {
  const { Yoga, detailsOpen, normalizeWhitespace, onMeasureText } = ctx;
  const setTrueosLayoutStep = ctx.setTrueosLayoutStep ?? (() => {});
  setTrueosLayoutStep(`build:start nodes=${renderNodes.length} viewport=${viewportWidth}x${viewportHeight}`);
  const padding = 12;
  const gap = 8;

  const theme = defaultTheme;
  setTrueosLayoutStep('build:measurer');
  const measurer = createTextMeasurer(`${theme.fontSize}px ${theme.fontFamily}`, normalizeWhitespace, onMeasureText);

  function gapAfter(child: RenderNode): number {
    if (child.kind !== 'block') return 0;
    if (child.tagName === 'hr') return 0;
    if (child.tagName === 'tr' || child.tagName === 'td' || child.tagName === 'th') return 0;
    return gap;
  }

  function yogaForNode(node: RenderNode): { yogaNode: any; buildBox: () => LayoutBox } {
    const nodeLabel = node.kind === 'text' ? `text:${node.text.slice(0, 24)}` : `${node.tagName}:${node.key}`;
    setTrueosLayoutStep(`node:${nodeLabel}:start`);
    if (node.kind === 'text') {
      const yogaNode = Yoga.Node.create();
      setTrueosLayoutStep(`node:${nodeLabel}:measure-func`);
      yogaNode.setMeasureFunc((width: number, widthMode: number) => {
        setTrueosLayoutStep(`node:${nodeLabel}:measure-call`);
        const maxWidth = widthMode === Yoga.MEASURE_MODE_UNDEFINED ? undefined : Math.max(0, width);
        const m = measurer.measure(node.text, maxWidth);
        return { width: m.width, height: m.height };
      });
      yogaNode.setMargin(Yoga.EDGE_RIGHT, 6);
      yogaNode.setMargin(Yoga.EDGE_BOTTOM, 0);

      return {
        yogaNode,
        buildBox: () => ({
          kind: 'text',
          text: node.text,
          x: yogaNode.getComputedLeft(),
          y: yogaNode.getComputedTop(),
          width: yogaNode.getComputedWidth(),
          height: yogaNode.getComputedHeight(),
          children: [],
        }),
      };
    }

    if (node.tagName === 'sliderlabel') {
      setTrueosLayoutStep(`node:${node.tagName}:${node.key}:sliderlabel`);
      return createYogaNodeForSliderLabel({ node, Yoga, measurer });
    }

    setTrueosLayoutStep(`node:${node.tagName}:${node.key}:create`);
    const yogaNode = Yoga.Node.create();

    setTrueosLayoutStep(`node:${node.tagName}:${node.key}:base-defaults`);
    yogaNode.setFlexDirection(Yoga.FLEX_DIRECTION_COLUMN);
    yogaNode.setAlignItems(Yoga.ALIGN_STRETCH);
    yogaNode.setPadding(Yoga.EDGE_LEFT, padding);
    yogaNode.setPadding(Yoga.EDGE_RIGHT, padding);
    yogaNode.setPadding(Yoga.EDGE_TOP, padding);
    yogaNode.setPadding(Yoga.EDGE_BOTTOM, padding);
    yogaNode.setMargin(Yoga.EDGE_BOTTOM, 0);

    if (isHeadingTag(node.tagName)) {
      setTrueosLayoutStep(`node:${node.tagName}:${node.key}:heading-defaults`);
      applyYogaDefaultsHeading(yogaNode, Yoga);
    }

    if (node.tagName === 'hr') {
      setTrueosLayoutStep(`node:${node.tagName}:${node.key}:hr-defaults`);
      applyYogaDefaultsHr(yogaNode, Yoga);
    }

    if (node.tagName === 'p' || node.tagName === 'label') {
      setTrueosLayoutStep(`node:${node.tagName}:${node.key}:inline-scan`);
      const hasControls = node.children.some(
        (c) =>
          c.kind === 'block' &&
          (c.tagName === 'input' ||
            c.tagName === 'button' ||
            c.tagName === 'select' ||
            c.tagName === 'textarea' ||
            c.tagName === 'timeinput' ||
            c.tagName === 'dateinput' ||
            c.tagName === 'monthinput' ||
            c.tagName === 'weekinput' ||
            c.tagName === 'datetimelocalinput' ||
            c.tagName === 'progress' ||
            c.tagName === 'meter' ||
            c.tagName === 'slider' ||
            c.tagName === 'number' ||
            c.tagName === 'color')
      );

      if (hasControls) {
        yogaNode.setFlexDirection(Yoga.FLEX_DIRECTION_ROW);
        yogaNode.setFlexWrap(Yoga.WRAP_WRAP);
        yogaNode.setAlignItems(Yoga.ALIGN_CENTER);
      }

      yogaNode.setPadding(Yoga.EDGE_TOP, 4);
      yogaNode.setPadding(Yoga.EDGE_BOTTOM, 4);
      yogaNode.setPadding(Yoga.EDGE_LEFT, 4);
      yogaNode.setPadding(Yoga.EDGE_RIGHT, 4);
    }

    if (node.tagName === 'table') {
      setTrueosLayoutStep(`node:${node.tagName}:${node.key}:table-defaults`);
      applyYogaDefaultsTable(yogaNode, Yoga);
    }
    if (node.tagName === 'tr') {
      setTrueosLayoutStep(`node:${node.tagName}:${node.key}:tr-defaults`);
      applyYogaDefaultsTr(yogaNode, Yoga);
    }
    if (node.tagName === 'td' || node.tagName === 'th') {
      setTrueosLayoutStep(`node:${node.tagName}:${node.key}:cell-defaults`);
      applyYogaDefaultsCell(yogaNode, Yoga);
    }

    if (node.tagName === 'input') {
      setTrueosLayoutStep(`node:${node.tagName}:${node.key}:input-defaults`);
      applyYogaDefaultsInput(yogaNode, node, Yoga);
    }
    if (node.tagName === 'textarea') {
      setTrueosLayoutStep(`node:${node.tagName}:${node.key}:textarea-defaults`);
      applyYogaDefaultsTextarea(yogaNode, Yoga);
    }
    if (node.tagName === 'select') {
      setTrueosLayoutStep(`node:${node.tagName}:${node.key}:select-defaults`);
      applyYogaDefaultsSelect(yogaNode, Yoga);
    }

    if (
      node.tagName === 'timeinput' ||
      node.tagName === 'dateinput' ||
      node.tagName === 'monthinput' ||
      node.tagName === 'weekinput' ||
      node.tagName === 'datetimelocalinput'
    ) {
      const kind =
        node.tagName === 'timeinput'
          ? 'time'
          : node.tagName === 'monthinput'
            ? 'month'
            : node.tagName === 'weekinput'
              ? 'week'
              : node.tagName === 'dateinput'
                ? 'date'
                : 'datetime-local';
      setTrueosLayoutStep(`node:${node.tagName}:${node.key}:temporal-defaults`);
      applyYogaDefaultsTemporalInput(yogaNode, Yoga, kind);
    }

    if (node.tagName === 'img') {
      setTrueosLayoutStep(`node:${node.tagName}:${node.key}:img-defaults`);
      applyYogaDefaultsImg(yogaNode, node, Yoga);
    }
    if (node.tagName === 'svg') {
      setTrueosLayoutStep(`node:${node.tagName}:${node.key}:svg-defaults`);
      applyYogaDefaultsSvg(yogaNode, node, Yoga);
    }
    if (node.tagName === 'canvas') {
      setTrueosLayoutStep(`node:${node.tagName}:${node.key}:canvas-defaults`);
      applyYogaDefaultsCanvas(yogaNode, node, Yoga);
    }
    if (node.tagName === 'iframe') {
      setTrueosLayoutStep(`node:${node.tagName}:${node.key}:iframe-defaults`);
      applyYogaDefaultsIframe(yogaNode, node, Yoga);
    }
    if (node.tagName === 'button') {
      setTrueosLayoutStep(`node:${node.tagName}:${node.key}:button-defaults`);
      applyYogaDefaultsButton(yogaNode, Yoga);
    }
    if (node.tagName === 'dialog') {
      setTrueosLayoutStep(`node:${node.tagName}:${node.key}:dialog-defaults`);
      applyYogaDefaultsDialog(yogaNode, Yoga);
    }
    if (node.tagName === 'number') {
      setTrueosLayoutStep(`node:${node.tagName}:${node.key}:number-defaults`);
      applyYogaDefaultsNumber(yogaNode, Yoga);
    }
    if (node.tagName === 'color') {
      setTrueosLayoutStep(`node:${node.tagName}:${node.key}:color-defaults`);
      applyYogaDefaultsColor(yogaNode, node, Yoga);
    }
    if (node.tagName === 'searchrow') {
      setTrueosLayoutStep(`node:${node.tagName}:${node.key}:searchrow-defaults`);
      applyYogaDefaultsSearchRow(yogaNode, Yoga);
    }
    if (node.tagName === 'searchbutton') {
      setTrueosLayoutStep(`node:${node.tagName}:${node.key}:searchbutton-defaults`);
      applyYogaDefaultsSearchButton(yogaNode, Yoga);
    }

    if (node.tagName === 'summary') {
      setTrueosLayoutStep(`node:${node.tagName}:${node.key}:summary-defaults`);
      applyYogaDefaultsSummary(yogaNode, Yoga);
    }
    if (node.tagName === 'details') {
      setTrueosLayoutStep(`node:${node.tagName}:${node.key}:details-defaults`);
      applyYogaDefaultsDetails(yogaNode, Yoga);
    }

    if (node.tagName === 'barrow') {
      setTrueosLayoutStep(`node:${node.tagName}:${node.key}:barrow-defaults`);
      applyYogaDefaultsBarrow(yogaNode, Yoga);
    }
    if (node.tagName === 'progress' || node.tagName === 'meter') {
      setTrueosLayoutStep(`node:${node.tagName}:${node.key}:progress-defaults`);
      applyYogaDefaultsProgressOrMeter(yogaNode, Yoga);
    }
    if (node.tagName === 'slider') {
      setTrueosLayoutStep(`node:${node.tagName}:${node.key}:slider-defaults`);
      applyYogaDefaultsSlider(yogaNode, Yoga);
    }

    setTrueosLayoutStep(`node:${node.tagName}:${node.key}:children-effective`);
    const effectiveChildren = getEffectiveDetailsChildren(node as any, detailsOpen) as RenderNode[];

    setTrueosLayoutStep(`node:${node.tagName}:${node.key}:children-map count=${effectiveChildren.length}`);
    const childPairs = effectiveChildren.map(yogaForNode);
    setTrueosLayoutStep(`node:${node.tagName}:${node.key}:children-insert`);
    for (let i = 0; i < childPairs.length; i++) {
      const childRender = effectiveChildren[i];
      const childPair = childPairs[i];
      if (childRender && childRender.kind === 'block') {
        const m = i === childPairs.length - 1 ? 0 : gapAfter(childRender);
        childPair.yogaNode.setMargin(Yoga.EDGE_BOTTOM, m);
      }
      yogaNode.insertChild(childPair.yogaNode, yogaNode.getChildCount());
    }

    return {
      yogaNode,
      buildBox: () => ({
        kind: 'block',
        key: node.key,
        tagName: node.tagName,
        attrs: node.attrs,
        x: yogaNode.getComputedLeft(),
        y: yogaNode.getComputedTop(),
        width: yogaNode.getComputedWidth(),
        height: yogaNode.getComputedHeight(),
        children: childPairs.map((c) => c.buildBox()),
      }),
    };
  }

  const rootYoga = Yoga.Node.create();
  setTrueosLayoutStep('root:flex-direction');
  rootYoga.setFlexDirection(Yoga.FLEX_DIRECTION_COLUMN);
  setTrueosLayoutStep('root:align-items');
  rootYoga.setAlignItems(Yoga.ALIGN_STRETCH);
  setTrueosLayoutStep('root:width');
  rootYoga.setWidth(viewportWidth);
  setTrueosLayoutStep('root:height');
  rootYoga.setHeight(viewportHeight);
  setTrueosLayoutStep('root:padding-left');
  rootYoga.setPadding(Yoga.EDGE_LEFT, 16);
  setTrueosLayoutStep('root:padding-top');
  rootYoga.setPadding(Yoga.EDGE_TOP, 16);
  setTrueosLayoutStep('root:padding-right');
  rootYoga.setPadding(Yoga.EDGE_RIGHT, 16 + SCROLLBAR_PAD);
  setTrueosLayoutStep('root:padding-bottom');
  rootYoga.setPadding(Yoga.EDGE_BOTTOM, 16);

  setTrueosLayoutStep(`root:children-map count=${renderNodes.length}`);
  const pairs = renderNodes.map(yogaForNode);
  setTrueosLayoutStep('root:children-insert');
  for (let i = 0; i < pairs.length; i++) {
    const renderNode = renderNodes[i];
    const pair = pairs[i];
    if (renderNode && renderNode.kind === 'block') {
      const m = i === pairs.length - 1 ? 0 : gapAfter(renderNode);
      pair.yogaNode.setMargin(Yoga.EDGE_BOTTOM, m);
    }
    rootYoga.insertChild(pair.yogaNode, rootYoga.getChildCount());
  }

  setTrueosLayoutStep('root:calculate');
  rootYoga.calculateLayout(viewportWidth, viewportHeight, Yoga.DIRECTION_LTR);

  setTrueosLayoutStep('root:build-box');
  const box: LayoutBox = {
    kind: 'block',
    tagName: 'root',
    x: 0,
    y: 0,
    width: rootYoga.getComputedWidth(),
    height: rootYoga.getComputedHeight(),
    children: pairs.map((p) => p.buildBox()),
  };

  setTrueosLayoutStep('root:free');
  rootYoga.freeRecursive?.();

  setTrueosLayoutStep('build:done');
  return box;
}
