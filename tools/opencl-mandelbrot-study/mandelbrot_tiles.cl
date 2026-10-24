#pragma OPENCL FP_CONTRACT OFF

#define LEAF_SIDE 64u
#define MAX_SIDE 512u

typedef struct { uint x, y, w, h; } Tile;

static uint pixel_iterations(uint x, uint y, float origin_x, float origin_y,
                             float step, uint max_iterations)
{
    float cx = origin_x + (float)x * step, cy = origin_y + (float)y * step;
    float zx = 0.0f, zy = 0.0f;
    uint n = 0;
    while (n < max_iterations && zx * zx + zy * zy <= 4.0f) {
        float next_x = zx * zx - zy * zy + cx;
        zy = 2.0f * zx * zy + cy;
        zx = next_x;
        ++n;
    }
    return n;
}

static void split_tile(Tile tile, __private Tile *first, __private Tile *second)
{
    *first = tile;
    *second = tile;
    if (tile.w >= tile.h) {
        first->w = tile.w / 2u;
        second->x += first->w;
        second->w -= first->w;
    } else {
        first->h = tile.h / 2u;
        second->y += first->h;
        second->h -= first->h;
    }
}

static void render_leaf(__global uint *output, uint width, Tile tile,
                        float origin_x, float origin_y, float step, uint limit)
{
    uint lane = (uint)get_local_id(0), stride = (uint)get_local_size(0);
    for (uint p = lane; p < tile.w * tile.h; p += stride) {
        uint x = tile.x + p % tile.w, y = tile.y + p / tile.w;
        output[y * width + x] = pixel_iterations(x, y, origin_x, origin_y, step, limit);
    }
}

#ifdef RECURSIVE_PROBE
__attribute__((noinline))
static void visit(__global uint *output, uint width, Tile tile,
                  float origin_x, float origin_y, float step, uint limit)
{
    if (tile.w <= LEAF_SIDE && tile.h <= LEAF_SIDE) {
        render_leaf(output, width, tile, origin_x, origin_y, step, limit);
        return;
    }
    Tile first, second;
    split_tile(tile, &first, &second);
    visit(output, width, first, origin_x, origin_y, step, limit);
    visit(output, width, second, origin_x, origin_y, step, limit);
}
#endif

#ifdef FORCE_SIMD8
__attribute__((intel_reqd_sub_group_size(8)))
#endif
__kernel void mandelbrot(__global uint *output, uint width, uint height,
                         float origin_x, float origin_y, float step, uint limit)
{
    if (width == 0u || height == 0u || width > MAX_SIDE || height > MAX_SIDE) return;
    Tile tile = {0u, 0u, width, height};
#ifdef RECURSIVE_PROBE
    if (get_group_id(0) != 0u) return;
    visit(output, width, tile, origin_x, origin_y, step, limit);
#elif defined(PARALLEL_LEAVES)
    // Six path bits are valid only for this complete 512-to-64 binary tree.
    uint leaf = (uint)get_group_id(0);
    if (width != 512u || height != 512u || leaf >= 64u) return;
    for (uint depth = 0u; depth < 6u; ++depth) {
        Tile first, second;
        split_tile(tile, &first, &second);
        tile = ((leaf >> (5u - depth)) & 1u) ? second : first;
    }
    render_leaf(output, width, tile, origin_x, origin_y, step, limit);
#else
    if (get_group_id(0) != 0u) return;
    // With both dimensions <=512, DFS needs at most seven pending nodes.
    Tile pending[7];
    uint count = 1u;
    pending[0] = tile;
    while (count != 0u) {
        tile = pending[--count];
        if (tile.w <= LEAF_SIDE && tile.h <= LEAF_SIDE) {
            render_leaf(output, width, tile, origin_x, origin_y, step, limit);
        } else {
            Tile first, second;
            split_tile(tile, &first, &second);
            pending[count++] = second;
            pending[count++] = first;
        }
    }
#endif
}
