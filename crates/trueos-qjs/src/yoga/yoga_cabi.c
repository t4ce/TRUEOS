#include <stdbool.h>
#include <stdint.h>

// Minimal in-kernel Yoga C ABI implementation.
// This is not a full Yoga engine; it implements the subset of YG* symbols
// used by trueos-qjs and computes a simple deterministic flex-like layout.

typedef void *YGNodeRef;
typedef void *YGConfigRef;

enum {
    EDGE_LEFT = 0,
    EDGE_TOP = 1,
    EDGE_RIGHT = 2,
    EDGE_BOTTOM = 3,
};

enum {
    FLEX_DIRECTION_COLUMN = 0,
    FLEX_DIRECTION_ROW = 2,
};

enum {
    POSITION_TYPE_RELATIVE = 1,
    POSITION_TYPE_ABSOLUTE = 2,
};

#define YG_MAX_NODES 8192
#define YG_MAX_CONFIGS 64
#define YG_INVALID_IDX ((uint32_t)0xFFFFFFFFu)

typedef struct {
    bool used;
    bool use_web_defaults;
} YGConfigData;

typedef struct {
    bool used;

    uint32_t parent;
    uint32_t first_child;
    uint32_t last_child;
    uint32_t next_sibling;
    uint32_t prev_sibling;
    uint32_t child_count;

    int32_t flex_direction;
    int32_t position_type;

    float width;
    float height;
    float min_width;
    float min_height;

    float padding[4];
    float margin[4];
    float position[4];

    float layout_left;
    float layout_top;
    float layout_width;
    float layout_height;
} YGNodeData;

static YGConfigData g_configs[YG_MAX_CONFIGS];
static YGNodeData g_nodes[YG_MAX_NODES];

static inline float yg_maxf(float a, float b) {
    return (a > b) ? a : b;
}

static inline uint32_t ref_to_idx(void *ref) {
    uintptr_t raw = (uintptr_t)ref;
    if (raw == 0) {
        return YG_INVALID_IDX;
    }
    uint32_t idx = (uint32_t)(raw - 1u);
    if (idx >= YG_MAX_NODES) {
        return YG_INVALID_IDX;
    }
    if (!g_nodes[idx].used) {
        return YG_INVALID_IDX;
    }
    return idx;
}

static inline void *idx_to_ref(uint32_t idx) {
    return (void *)((uintptr_t)idx + 1u);
}

static inline uint32_t cfg_ref_to_idx(void *ref) {
    uintptr_t raw = (uintptr_t)ref;
    if (raw == 0) {
        return YG_INVALID_IDX;
    }
    uint32_t idx = (uint32_t)(raw - 1u);
    if (idx >= YG_MAX_CONFIGS) {
        return YG_INVALID_IDX;
    }
    if (!g_configs[idx].used) {
        return YG_INVALID_IDX;
    }
    return idx;
}

static inline void *cfg_idx_to_ref(uint32_t idx) {
    return (void *)((uintptr_t)idx + 1u);
}

static uint32_t alloc_config(void) {
    for (uint32_t i = 0; i < YG_MAX_CONFIGS; i++) {
        if (!g_configs[i].used) {
            g_configs[i].used = true;
            g_configs[i].use_web_defaults = false;
            return i;
        }
    }
    return YG_INVALID_IDX;
}

static uint32_t alloc_node(uint32_t cfg_idx) {
    bool use_web_defaults = false;
    if (cfg_idx != YG_INVALID_IDX && cfg_idx < YG_MAX_CONFIGS && g_configs[cfg_idx].used) {
        use_web_defaults = g_configs[cfg_idx].use_web_defaults;
    }
    for (uint32_t i = 0; i < YG_MAX_NODES; i++) {
        if (!g_nodes[i].used) {
            g_nodes[i].used = true;
            g_nodes[i].parent = YG_INVALID_IDX;
            g_nodes[i].first_child = YG_INVALID_IDX;
            g_nodes[i].last_child = YG_INVALID_IDX;
            g_nodes[i].next_sibling = YG_INVALID_IDX;
            g_nodes[i].prev_sibling = YG_INVALID_IDX;
            g_nodes[i].child_count = 0;

            // Match Yoga's useWebDefaults behavior for implemented fields.
            g_nodes[i].flex_direction = use_web_defaults ? FLEX_DIRECTION_ROW : FLEX_DIRECTION_COLUMN;
            g_nodes[i].position_type = POSITION_TYPE_RELATIVE;

            g_nodes[i].width = -1.0f;
            g_nodes[i].height = -1.0f;
            g_nodes[i].min_width = 0.0f;
            g_nodes[i].min_height = 0.0f;

            for (int e = 0; e < 4; e++) {
                g_nodes[i].padding[e] = 0.0f;
                g_nodes[i].margin[e] = 0.0f;
                g_nodes[i].position[e] = 0.0f;
            }

            g_nodes[i].layout_left = 0.0f;
            g_nodes[i].layout_top = 0.0f;
            g_nodes[i].layout_width = 0.0f;
            g_nodes[i].layout_height = 0.0f;
            return i;
        }
    }
    return YG_INVALID_IDX;
}

static void detach_child(uint32_t child_idx) {
    if (child_idx == YG_INVALID_IDX || !g_nodes[child_idx].used) {
        return;
    }
    uint32_t parent_idx = g_nodes[child_idx].parent;
    if (parent_idx == YG_INVALID_IDX || !g_nodes[parent_idx].used) {
        g_nodes[child_idx].parent = YG_INVALID_IDX;
        g_nodes[child_idx].prev_sibling = YG_INVALID_IDX;
        g_nodes[child_idx].next_sibling = YG_INVALID_IDX;
        return;
    }

    uint32_t prev = g_nodes[child_idx].prev_sibling;
    uint32_t next = g_nodes[child_idx].next_sibling;

    if (prev != YG_INVALID_IDX) {
        g_nodes[prev].next_sibling = next;
    } else {
        g_nodes[parent_idx].first_child = next;
    }

    if (next != YG_INVALID_IDX) {
        g_nodes[next].prev_sibling = prev;
    } else {
        g_nodes[parent_idx].last_child = prev;
    }

    if (g_nodes[parent_idx].child_count > 0) {
        g_nodes[parent_idx].child_count--;
    }

    g_nodes[child_idx].parent = YG_INVALID_IDX;
    g_nodes[child_idx].prev_sibling = YG_INVALID_IDX;
    g_nodes[child_idx].next_sibling = YG_INVALID_IDX;
}

static void insert_child_at(uint32_t parent_idx, uint32_t child_idx, uint32_t index) {
    if (parent_idx == YG_INVALID_IDX || child_idx == YG_INVALID_IDX) {
        return;
    }
    if (!g_nodes[parent_idx].used || !g_nodes[child_idx].used || parent_idx == child_idx) {
        return;
    }

    detach_child(child_idx);

    uint32_t count = g_nodes[parent_idx].child_count;
    if (index > count) {
        index = count;
    }

    if (count == 0) {
        g_nodes[parent_idx].first_child = child_idx;
        g_nodes[parent_idx].last_child = child_idx;
        g_nodes[child_idx].parent = parent_idx;
        g_nodes[child_idx].prev_sibling = YG_INVALID_IDX;
        g_nodes[child_idx].next_sibling = YG_INVALID_IDX;
        g_nodes[parent_idx].child_count = 1;
        return;
    }

    if (index == count) {
        uint32_t old_last = g_nodes[parent_idx].last_child;
        g_nodes[old_last].next_sibling = child_idx;
        g_nodes[child_idx].prev_sibling = old_last;
        g_nodes[child_idx].next_sibling = YG_INVALID_IDX;
        g_nodes[child_idx].parent = parent_idx;
        g_nodes[parent_idx].last_child = child_idx;
        g_nodes[parent_idx].child_count = count + 1;
        return;
    }

    uint32_t at = g_nodes[parent_idx].first_child;
    for (uint32_t i = 0; i < index && at != YG_INVALID_IDX; i++) {
        at = g_nodes[at].next_sibling;
    }
    if (at == YG_INVALID_IDX) {
        uint32_t old_last = g_nodes[parent_idx].last_child;
        g_nodes[old_last].next_sibling = child_idx;
        g_nodes[child_idx].prev_sibling = old_last;
        g_nodes[child_idx].next_sibling = YG_INVALID_IDX;
        g_nodes[child_idx].parent = parent_idx;
        g_nodes[parent_idx].last_child = child_idx;
        g_nodes[parent_idx].child_count = count + 1;
        return;
    }

    uint32_t prev = g_nodes[at].prev_sibling;
    g_nodes[child_idx].parent = parent_idx;
    g_nodes[child_idx].prev_sibling = prev;
    g_nodes[child_idx].next_sibling = at;
    g_nodes[at].prev_sibling = child_idx;

    if (prev != YG_INVALID_IDX) {
        g_nodes[prev].next_sibling = child_idx;
    } else {
        g_nodes[parent_idx].first_child = child_idx;
    }

    g_nodes[parent_idx].child_count = count + 1;
}

static void free_recursive_idx(uint32_t idx) {
    if (idx == YG_INVALID_IDX || !g_nodes[idx].used) {
        return;
    }

    uint32_t ch = g_nodes[idx].first_child;
    while (ch != YG_INVALID_IDX) {
        uint32_t next = g_nodes[ch].next_sibling;
        free_recursive_idx(ch);
        ch = next;
    }

    detach_child(idx);
    g_nodes[idx].used = false;
}

static void compute_layout(uint32_t idx, float avail_w, float avail_h) {
    if (idx == YG_INVALID_IDX || !g_nodes[idx].used) {
        return;
    }

    YGNodeData *n = &g_nodes[idx];

    float pad_l = n->padding[EDGE_LEFT];
    float pad_t = n->padding[EDGE_TOP];
    float pad_r = n->padding[EDGE_RIGHT];
    float pad_b = n->padding[EDGE_BOTTOM];

    float explicit_w = n->width;
    float explicit_h = n->height;

    float inner_avail_w = (explicit_w >= 0.0f) ? (explicit_w - pad_l - pad_r) : (avail_w - pad_l - pad_r);
    if (inner_avail_w < 0.0f) {
        inner_avail_w = 0.0f;
    }

    float inner_avail_h = (explicit_h >= 0.0f) ? (explicit_h - pad_t - pad_b) : (avail_h - pad_t - pad_b);
    if (inner_avail_h < 0.0f) {
        inner_avail_h = 0.0f;
    }

    float cursor_x = pad_l;
    float cursor_y = pad_t;
    float max_x = pad_l;
    float max_y = pad_t;

    uint32_t child = n->first_child;
    while (child != YG_INVALID_IDX) {
        YGNodeData *c = &g_nodes[child];
        compute_layout(child, inner_avail_w, inner_avail_h);

        float ml = c->margin[EDGE_LEFT];
        float mt = c->margin[EDGE_TOP];
        float mr = c->margin[EDGE_RIGHT];
        float mb = c->margin[EDGE_BOTTOM];

        if (c->position_type == POSITION_TYPE_ABSOLUTE) {
            c->layout_left = pad_l + c->position[EDGE_LEFT] + ml;
            c->layout_top = pad_t + c->position[EDGE_TOP] + mt;
        } else if (n->flex_direction == FLEX_DIRECTION_ROW) {
            c->layout_left = cursor_x + ml;
            c->layout_top = pad_t + mt;
            cursor_x = c->layout_left + c->layout_width + mr;
        } else {
            c->layout_left = pad_l + ml;
            c->layout_top = cursor_y + mt;
            cursor_y = c->layout_top + c->layout_height + mb;
        }

        float end_x = c->layout_left + c->layout_width + mr;
        float end_y = c->layout_top + c->layout_height + mb;
        if (end_x > max_x) {
            max_x = end_x;
        }
        if (end_y > max_y) {
            max_y = end_y;
        }

        child = c->next_sibling;
    }

    float inferred_w = max_x + pad_r;
    float inferred_h = max_y + pad_b;

    float out_w = (explicit_w >= 0.0f) ? explicit_w : inferred_w;
    float out_h = (explicit_h >= 0.0f) ? explicit_h : inferred_h;

    out_w = yg_maxf(out_w, n->min_width);
    out_h = yg_maxf(out_h, n->min_height);

    if (out_w < 0.0f) {
        out_w = 0.0f;
    }
    if (out_h < 0.0f) {
        out_h = 0.0f;
    }

    n->layout_width = out_w;
    n->layout_height = out_h;
}

YGConfigRef YGConfigNew(void) {
    uint32_t idx = alloc_config();
    if (idx == YG_INVALID_IDX) {
        return (YGConfigRef)0;
    }
    return (YGConfigRef)cfg_idx_to_ref(idx);
}

void YGConfigFree(YGConfigRef config) {
    uint32_t idx = cfg_ref_to_idx(config);
    if (idx == YG_INVALID_IDX) {
        return;
    }
    g_configs[idx].used = false;
    g_configs[idx].use_web_defaults = false;
}

void YGConfigSetUseWebDefaults(YGConfigRef config, bool enabled) {
    uint32_t idx = cfg_ref_to_idx(config);
    if (idx == YG_INVALID_IDX) {
        return;
    }
    g_configs[idx].use_web_defaults = enabled;
}

YGNodeRef YGNodeNewWithConfig(YGConfigRef config) {
    uint32_t cfg_idx = cfg_ref_to_idx(config);
    uint32_t idx = alloc_node(cfg_idx);
    if (idx == YG_INVALID_IDX) {
        return (YGNodeRef)0;
    }
    return (YGNodeRef)idx_to_ref(idx);
}

void YGNodeFreeRecursive(YGNodeRef node) {
    uint32_t idx = ref_to_idx(node);
    if (idx == YG_INVALID_IDX) {
        return;
    }
    free_recursive_idx(idx);
}

void YGNodeInsertChild(YGNodeRef node, YGNodeRef child, uint32_t index) {
    uint32_t p = ref_to_idx(node);
    uint32_t c = ref_to_idx(child);
    insert_child_at(p, c, index);
}

uint32_t YGNodeGetChildCount(YGNodeRef node) {
    uint32_t idx = ref_to_idx(node);
    if (idx == YG_INVALID_IDX) {
        return 0;
    }
    return g_nodes[idx].child_count;
}

void YGNodeCalculateLayout(YGNodeRef node, float width, float height, int32_t direction) {
    (void)direction;
    uint32_t idx = ref_to_idx(node);
    if (idx == YG_INVALID_IDX) {
        return;
    }
    g_nodes[idx].layout_left = 0.0f;
    g_nodes[idx].layout_top = 0.0f;
    compute_layout(idx, width, height);
}

void YGNodeStyleSetFlexDirection(YGNodeRef node, int32_t value) {
    uint32_t idx = ref_to_idx(node);
    if (idx != YG_INVALID_IDX) {
        g_nodes[idx].flex_direction = value;
    }
}

void YGNodeStyleSetAlignItems(YGNodeRef node, int32_t value) {
    (void)node;
    (void)value;
}

void YGNodeStyleSetAlignSelf(YGNodeRef node, int32_t value) {
    (void)node;
    (void)value;
}

void YGNodeStyleSetJustifyContent(YGNodeRef node, int32_t value) {
    (void)node;
    (void)value;
}

void YGNodeStyleSetFlexWrap(YGNodeRef node, int32_t value) {
    (void)node;
    (void)value;
}

void YGNodeStyleSetFlexGrow(YGNodeRef node, float value) {
    (void)node;
    (void)value;
}

void YGNodeStyleSetFlexShrink(YGNodeRef node, float value) {
    (void)node;
    (void)value;
}

void YGNodeStyleSetPositionType(YGNodeRef node, int32_t value) {
    uint32_t idx = ref_to_idx(node);
    if (idx != YG_INVALID_IDX) {
        g_nodes[idx].position_type = value;
    }
}

void YGNodeStyleSetWidth(YGNodeRef node, float value) {
    uint32_t idx = ref_to_idx(node);
    if (idx != YG_INVALID_IDX) {
        g_nodes[idx].width = value;
    }
}

void YGNodeStyleSetHeight(YGNodeRef node, float value) {
    uint32_t idx = ref_to_idx(node);
    if (idx != YG_INVALID_IDX) {
        g_nodes[idx].height = value;
    }
}

void YGNodeStyleSetMinWidth(YGNodeRef node, float value) {
    uint32_t idx = ref_to_idx(node);
    if (idx != YG_INVALID_IDX) {
        g_nodes[idx].min_width = value;
    }
}

void YGNodeStyleSetMinHeight(YGNodeRef node, float value) {
    uint32_t idx = ref_to_idx(node);
    if (idx != YG_INVALID_IDX) {
        g_nodes[idx].min_height = value;
    }
}

void YGNodeStyleSetPadding(YGNodeRef node, int32_t edge, float value) {
    uint32_t idx = ref_to_idx(node);
    if (idx == YG_INVALID_IDX) {
        return;
    }
    if (edge >= 0 && edge <= 3) {
        g_nodes[idx].padding[edge] = value;
    }
}

void YGNodeStyleSetMargin(YGNodeRef node, int32_t edge, float value) {
    uint32_t idx = ref_to_idx(node);
    if (idx == YG_INVALID_IDX) {
        return;
    }
    if (edge >= 0 && edge <= 3) {
        g_nodes[idx].margin[edge] = value;
    }
}

void YGNodeStyleSetPosition(YGNodeRef node, int32_t edge, float value) {
    uint32_t idx = ref_to_idx(node);
    if (idx == YG_INVALID_IDX) {
        return;
    }
    if (edge >= 0 && edge <= 3) {
        g_nodes[idx].position[edge] = value;
    }
}

float YGNodeLayoutGetLeft(YGNodeRef node) {
    uint32_t idx = ref_to_idx(node);
    if (idx == YG_INVALID_IDX) {
        return 0.0f;
    }
    return g_nodes[idx].layout_left;
}

float YGNodeLayoutGetTop(YGNodeRef node) {
    uint32_t idx = ref_to_idx(node);
    if (idx == YG_INVALID_IDX) {
        return 0.0f;
    }
    return g_nodes[idx].layout_top;
}

float YGNodeLayoutGetWidth(YGNodeRef node) {
    uint32_t idx = ref_to_idx(node);
    if (idx == YG_INVALID_IDX) {
        return 0.0f;
    }
    return g_nodes[idx].layout_width;
}

float YGNodeLayoutGetHeight(YGNodeRef node) {
    uint32_t idx = ref_to_idx(node);
    if (idx == YG_INVALID_IDX) {
        return 0.0f;
    }
    return g_nodes[idx].layout_height;
}
