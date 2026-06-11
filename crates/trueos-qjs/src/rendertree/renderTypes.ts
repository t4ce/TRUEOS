export type BlockNode = {
  kind: 'block';
  key: string;
  tagName: string;
  attrs?: Record<string, string>;
  children: RenderNode[];
};

export type TextNode = {
  kind: 'text';
  text: string;
};

export type RenderNode = BlockNode | TextNode;

export type LayoutBox = {
  kind: 'block' | 'text';
  key?: string;
  tagName?: string;
  attrs?: Record<string, string>;
  text?: string;
  x: number;
  y: number;
  width: number;
  height: number;
  children: LayoutBox[];
};

export type TextStyleCtx = {
  bold: boolean;
};

export type TrueosBridgeStats = {
  renderNodes: number;
  renderBlocks: number;
  renderText: number;
  renderTags: string;
  renderTextSamples: string;
  layoutBoxes: number;
  layoutBlocks: number;
  layoutText: number;
  layoutMaxDepth: number;
  layoutTextSamples: string;
  prePixiHash: string;
  prePixiRenderHash: string;
  prePixiLayoutHash: string;
  prePixiTraceBytes: number;
  measureTextCalls: number;
  scrollbarVisible: number;
  scrollbarTrack: string;
  scrollbarThumb: string;
  pixiCommands: number;
  pixiOps: string;
  pixiUnsupported: string;
};

export type TrueosTreeStats = {
  nodes: number;
  blocks: number;
  text: number;
  maxDepth: number;
  tags: Record<string, number>;
};

export type PrePixiTraceInfo = {
  hash: string;
  renderHash: string;
  layoutHash: string;
  bytes: number;
};
