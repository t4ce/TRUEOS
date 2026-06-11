export function walkWidgets(node, visitor, depth = 0, parent = null) {
  visitor(node, { depth, parent });
  for (const child of node.children ?? []) walkWidgets(child, visitor, depth + 1, node);
}

export function flattenWidgetTree(root) {
  const out = [];
  walkWidgets(root, (node, ctx) => {
    out.push({ node, depth: ctx.depth, parent: ctx.parent });
  });
  return out;
}

export function collectWidgetStats(root) {
  const stats = {
    nodes: 0,
    widgets: 0,
    text: 0,
    complex: 0,
    interactive: 0,
    tags: {},
    categories: {},
  };

  walkWidgets(root, (node) => {
    stats.nodes += 1;

    if (node.kind === 'text') {
      stats.text += 1;
      return;
    }

    if (node.kind !== 'widget') return;

    stats.widgets += 1;
    if (node.meta?.complexity === 'complex') stats.complex += 1;
    if (node.meta?.interactive) stats.interactive += 1;

    const tag = node.tag ?? 'unknown';
    const category = node.category ?? 'unknown';
    stats.tags[tag] = (stats.tags[tag] ?? 0) + 1;
    stats.categories[category] = (stats.categories[category] ?? 0) + 1;
  });

  return stats;
}

