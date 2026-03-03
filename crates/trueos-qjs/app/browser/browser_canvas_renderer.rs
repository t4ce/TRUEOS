#![cfg(feature = "trueos")]

pub const CANVAS_RENDERER_SHIM_JS: &[u8] = br#"
(function () {
    const G = (typeof globalThis !== 'undefined') ? globalThis : this;
    const TRACE_SUBMIT = true;
    const TRACE_LAYOUT = true;
    const TRACE_PIPELINES = false;
    const TRACE_BINDGROUPS = false;
    const TRACE_TEXPATH = true;
    const TRACE_VERTEX_FRAMES = true;
    const FORCE_CURSOR_HEARTBEAT = false;
    const FORCE_DEBUG_OVERLAY = false;
    const FORCE_OPAQUE_ALPHA = true;

    if (!G.__trueosRendererState) {
        G.__trueosRendererState = {
            frameSeq: 0,
            fallbackTexId: 0,
            layoutLogCount: 0,
            layoutLoggedByPipeline: {},
            pipelineFamilies: {},
            bindGroupLogByPipeline: {},
            texTraceDone: false,
            vertexDumpDone: false,
            // Submit-scope cache: this will be used by follow-up draw translation.
            cache: {
                pipelineId: 0,
                indexBufferId: 0,
                vertexBufferIds: {},
                viewportKey: '',
                scissorKey: '',
                stencilRef: 0,
            },
        };
    }

    function __trueosClamp01(x) {
        const n = Number(x);
        if (!Number.isFinite(n)) return 0;
        if (n <= 0) return 0;
        if (n >= 1) return 1;
        return n;
    }

    function __trueosColorToRgb24(c) {
        if (Array.isArray(c)) {
            const r = Math.round(__trueosClamp01(c[0]) * 255) & 0xff;
            const g = Math.round(__trueosClamp01(c[1]) * 255) & 0xff;
            const b = Math.round(__trueosClamp01(c[2]) * 255) & 0xff;
            return ((r << 16) | (g << 8) | b) >>> 0;
        }
        if (c && typeof c === 'object') {
            const r = Math.round(__trueosClamp01(c.r) * 255) & 0xff;
            const g = Math.round(__trueosClamp01(c.g) * 255) & 0xff;
            const b = Math.round(__trueosClamp01(c.b) * 255) & 0xff;
            return ((r << 16) | (g << 8) | b) >>> 0;
        }
        return 0x000000;
    }

    function __trueosExtractPassViewport(passDesc, fallbackW, fallbackH) {
        const ca0 = passDesc && passDesc.colorAttachments && passDesc.colorAttachments[0];
        const tex = ca0 && ca0.view && ca0.view.texture;
        const tw = tex && Number(tex.width) > 0 ? (Number(tex.width) | 0) : fallbackW;
        const th = tex && Number(tex.height) > 0 ? (Number(tex.height) | 0) : fallbackH;
        return {
            width: Math.max(1, tw | 0),
            height: Math.max(1, th | 0),
        };
    }

    function __trueosScanSubmit(cmdBuffers, cache) {
        let hasWork = false;
        let clearRgb = null;
        let viewportW = 1280;
        let viewportH = 800;
        let passCount = 0;
        let opCount = 0;
        let drawCount = 0;

        for (let i = 0; i < cmdBuffers.length; i++) {
            const cb = cmdBuffers[i];
            if (!cb || !Array.isArray(cb.__cmds)) continue;
            const cmds = cb.__cmds;
            for (let j = 0; j < cmds.length; j++) {
                const cmd = cmds[j];
                if (!cmd || cmd.__kind !== 'render-pass') continue;

                passCount++;
                hasWork = true;

                const vp = __trueosExtractPassViewport(cmd.__desc || {}, viewportW, viewportH);
                viewportW = vp.width;
                viewportH = vp.height;

                const ca0 = cmd.__desc && cmd.__desc.colorAttachments && cmd.__desc.colorAttachments[0];
                if (ca0 && ca0.loadOp === 'clear') {
                    clearRgb = __trueosColorToRgb24(ca0.clearValue);
                }

                const ops = Array.isArray(cmd.__ops) ? cmd.__ops : [];
                for (let k = 0; k < ops.length; k++) {
                    const op = ops[k];
                    if (!Array.isArray(op) || op.length === 0) continue;
                    if (op[0] === 'executeBundles') {
                        const bundles = Array.isArray(op[1]) ? op[1] : [];
                        for (let bi = 0; bi < bundles.length; bi++) {
                            const b = bundles[bi];
                            const bOps = (b && Array.isArray(b.__ops)) ? b.__ops : [];
                            for (let bo = 0; bo < bOps.length; bo++) {
                                const bop = bOps[bo];
                                if (!Array.isArray(bop) || bop.length === 0) continue;
                                opCount++;
                                switch (bop[0]) {
                                    case 'setPipeline': {
                                        const p = bop[1];
                                        cache.pipelineId = p && p.__id ? (p.__id | 0) : 0;
                                        break;
                                    }
                                    case 'setVertexBuffer': {
                                        const slot = Number(bop[1]) | 0;
                                        const bbuf = bop[2];
                                        cache.vertexBufferIds[slot] = bbuf && bbuf.__id ? (bbuf.__id | 0) : 0;
                                        break;
                                    }
                                    case 'setIndexBuffer': {
                                        const bbuf = bop[1];
                                        cache.indexBufferId = bbuf && bbuf.__id ? (bbuf.__id | 0) : 0;
                                        break;
                                    }
                                    case 'setViewport': {
                                        cache.viewportKey = `${bop[1]},${bop[2]},${bop[3]},${bop[4]},${bop[5]},${bop[6]}`;
                                        break;
                                    }
                                    case 'setScissorRect': {
                                        cache.scissorKey = `${bop[1]},${bop[2]},${bop[3]},${bop[4]}`;
                                        break;
                                    }
                                    case 'setStencilReference': {
                                        cache.stencilRef = Number(bop[1]) | 0;
                                        break;
                                    }
                                    case 'draw':
                                    case 'drawIndexed': {
                                        drawCount++;
                                        break;
                                    }
                                    default:
                                        break;
                                }
                            }
                        }
                        continue;
                    }
                    opCount++;
                    switch (op[0]) {
                        case 'setPipeline': {
                            const p = op[1];
                            cache.pipelineId = p && p.__id ? (p.__id | 0) : 0;
                            break;
                        }
                        case 'setVertexBuffer': {
                            const slot = Number(op[1]) | 0;
                            const b = op[2];
                            cache.vertexBufferIds[slot] = b && b.__id ? (b.__id | 0) : 0;
                            break;
                        }
                        case 'setIndexBuffer': {
                            const b = op[1];
                            cache.indexBufferId = b && b.__id ? (b.__id | 0) : 0;
                            break;
                        }
                        case 'setViewport': {
                            cache.viewportKey = `${op[1]},${op[2]},${op[3]},${op[4]},${op[5]},${op[6]}`;
                            break;
                        }
                        case 'setScissorRect': {
                            cache.scissorKey = `${op[1]},${op[2]},${op[3]},${op[4]}`;
                            break;
                        }
                        case 'setStencilReference': {
                            cache.stencilRef = Number(op[1]) | 0;
                            break;
                        }
                        case 'draw':
                        case 'drawIndexed': {
                            drawCount++;
                            break;
                        }
                        default:
                            break;
                    }
                }
            }
        }

        return {
            hasWork,
            clearRgb,
            viewportW,
            viewportH,
            passCount,
            opCount,
            drawCount,
        };
    }

    function __trueosMarkTextureUpload(tex) {
        if (!tex) return;
        const rev = Number(tex.__uploadRev || 0) | 0;
        tex.__uploadRev = (rev + 1) >>> 0;
    }

    function __trueosExtractTextureFromBindGroup(bg) {
        if (!bg || !Array.isArray(bg.entries)) return null;
        for (let i = 0; i < bg.entries.length; i++) {
            const e = bg.entries[i];
            const r = e && e.resource;
            if (!r) continue;
            if (r.texture) return r.texture;
            if (r.view && r.view.texture) return r.view.texture;
        }
        return null;
    }

    function __trueosCollectTexturesFromResource(resource, out) {
        if (!resource) return;
        if (Array.isArray(resource)) {
            for (let i = 0; i < resource.length; i++) {
                __trueosCollectTexturesFromResource(resource[i], out);
            }
            return;
        }
        if (resource.texture) {
            out.push(resource.texture);
            return;
        }
        if (resource.view && resource.view.texture) {
            out.push(resource.view.texture);
            return;
        }
    }

    function __trueosCollectBoundTextures(bindGroups) {
        const list = [];
        const seen = {};
        for (let i = 0; i < bindGroups.length; i++) {
            const bg = bindGroups[i];
            if (!bg || !Array.isArray(bg.entries)) continue;
            for (let j = 0; j < bg.entries.length; j++) {
                const e = bg.entries[j];
                const r = e && e.resource;
                if (!r) continue;
                const tmp = [];
                __trueosCollectTexturesFromResource(r, tmp);
                for (let k = 0; k < tmp.length; k++) {
                    const t = tmp[k];
                    if (!t) continue;
                    const id = Number(t.__id || 0) | 0;
                    const key = String(id > 0 ? id : (`obj:${list.length}`));
                    if (seen[key]) continue;
                    seen[key] = true;
                    list.push(t);
                }
            }
        }
        return list;
    }

    function __trueosFindBoundTexture(bindGroups, pageHint) {
        const all = __trueosCollectBoundTextures(bindGroups);
        if (all.length <= 0) {
            for (let i = 0; i < bindGroups.length; i++) {
                const tex = __trueosExtractTextureFromBindGroup(bindGroups[i]);
                if (tex) return tex;
            }
            return null;
        }

        const texArea = (t) => {
            const w = Math.max(1, Number(t && t.width || 1) | 0);
            const h = Math.max(1, Number(t && t.height || 1) | 0);
            return w * h;
        };
        const isUploaded = (t) => (Number(t && t.__uploadRev || 0) | 0) > 0;
        const isTiny = (t) => texArea(t) <= 4;
        const isEmpty = (t) => __trueosTexLooksEmpty(t);

        const p = Math.max(0, Number(pageHint || 0) | 0);
        let preferred = (p < all.length) ? all[p] : all[0];

        // Common case in current browser path: slot 0 can be a 1x1 fallback while
        // the real UI atlas/page texture is also bound. Avoid picking tiny fallback
        // if a larger uploaded texture exists.
        if (!preferred || isTiny(preferred) || isEmpty(preferred)) {
            let best = null;
            let bestArea = 0;
            for (let i = 0; i < all.length; i++) {
                const t = all[i];
                if (!t) continue;
                if (!isUploaded(t)) continue;
                if (isEmpty(t)) continue;
                const area = texArea(t);
                if (area > bestArea) {
                    bestArea = area;
                    best = t;
                }
            }
            if (best) return best;
        }

        return preferred;
    }

    function __trueosEnsureCmdTexture(cmd, tex) {
        if (!cmd || !tex || !(tex.__data instanceof Uint8Array)) return 0;
        const w = Math.max(1, Number(tex.width || 1) | 0);
        const h = Math.max(1, Number(tex.height || 1) | 0);
        const need = w * h * 4;
        if (tex.__data.byteLength < need) return 0;

        const rev = Number(tex.__uploadRev || 0) | 0;
        let id = Number(tex.__cmdTexId || 0) | 0;
        const uploadedRev = Number(tex.__cmdTexUploadedRev || -1) | 0;

        if (id <= 0) {
            if (typeof cmd.createTextureRgba !== 'function') return 0;
            const created = cmd.createTextureRgba(w, h, tex.__data);
            id = Number(created || 0) | 0;
            if (id <= 0) return 0;
            tex.__cmdTexId = id;
            tex.__cmdTexUploadedRev = rev;
            return id;
        }

        if (uploadedRev !== rev && typeof cmd.updateTextureRgba === 'function') {
            cmd.updateTextureRgba(id, w, h, tex.__data);
            tex.__cmdTexUploadedRev = rev;
        }
        return id;
    }

    function __trueosTexLabel(tex) {
        if (!tex) return 'none';
        const id = Number(tex.__id || 0) | 0;
        const w = Number(tex.width || 0) | 0;
        const h = Number(tex.height || 0) | 0;
        const rev = Number(tex.__uploadRev || 0) | 0;
        const cmdTexId = Number(tex.__cmdTexId || 0) | 0;
        const cmdRev = Number(tex.__cmdTexUploadedRev || -1) | 0;
        const len = (tex.__data instanceof Uint8Array) ? tex.__data.byteLength : 0;
        return `id=${id} ${w}x${h} uploadRev=${rev} cmdTex=${cmdTexId} cmdRev=${cmdRev} data=${len}`;
    }

    function __trueosDecodedBounds(bytes20) {
        if (!(bytes20 instanceof Uint8Array)) return 'none';
        const n = Math.floor(bytes20.byteLength / 20);
        if (n <= 0) return 'none';
        const dv = new DataView(bytes20.buffer, bytes20.byteOffset, bytes20.byteLength);
        let minX = Infinity;
        let minY = Infinity;
        let maxX = -Infinity;
        let maxY = -Infinity;
        for (let i = 0; i < n; i++) {
            const off = i * 20;
            const x = Number(dv.getFloat32(off + 0, true));
            const y = Number(dv.getFloat32(off + 4, true));
            if (!Number.isFinite(x) || !Number.isFinite(y)) continue;
            if (x < minX) minX = x;
            if (y < minY) minY = y;
            if (x > maxX) maxX = x;
            if (y > maxY) maxY = y;
        }
        if (!Number.isFinite(minX) || !Number.isFinite(minY) || !Number.isFinite(maxX) || !Number.isFinite(maxY)) {
            return 'none';
        }
        return `${minX.toFixed(3)},${minY.toFixed(3)}..${maxX.toFixed(3)},${maxY.toFixed(3)}`;
    }

    function __trueosTexStats(tex) {
        if (!tex || !(tex.__data instanceof Uint8Array)) return 'none';
        const d = tex.__data;
        const px = Math.min((d.byteLength / 4) | 0, 64);
        if (px <= 0) return 'none';
        let minA = 255;
        let maxA = 0;
        let minL = 255;
        let maxL = 0;
        let sumL = 0;
        for (let i = 0; i < px; i++) {
            const o = i * 4;
            const r = d[o + 0] | 0;
            const g = d[o + 1] | 0;
            const b = d[o + 2] | 0;
            const a = d[o + 3] | 0;
            const l = ((r + g + b) / 3) | 0;
            if (a < minA) minA = a;
            if (a > maxA) maxA = a;
            if (l < minL) minL = l;
            if (l > maxL) maxL = l;
            sumL += l;
        }
        const avgL = (sumL / px).toFixed(1);
        return `px=${px} lum=${minL}-${maxL}@${avgL} a=${minA}-${maxA}`;
    }

    function __trueosTexLooksEmpty(tex) {
        if (!tex || !(tex.__data instanceof Uint8Array)) return false;
        const d = tex.__data;
        const px = Math.min((d.byteLength / 4) | 0, 128);
        if (px <= 0) return false;
        for (let i = 0; i < px; i++) {
            const o = i * 4;
            if ((d[o + 0] | d[o + 1] | d[o + 2] | d[o + 3]) !== 0) {
                return false;
            }
        }
        return true;
    }

    function __trueosDecodedLooksFullscreen(bytes20) {
        if (!(bytes20 instanceof Uint8Array) || bytes20.byteLength < 20) return false;
        const dv = new DataView(bytes20.buffer, bytes20.byteOffset, bytes20.byteLength);
        const n = Math.floor(bytes20.byteLength / 20);
        let minX = Infinity;
        let minY = Infinity;
        let maxX = -Infinity;
        let maxY = -Infinity;
        for (let i = 0; i < n; i++) {
            const off = i * 20;
            const x = Number(dv.getFloat32(off + 0, true));
            const y = Number(dv.getFloat32(off + 4, true));
            if (!Number.isFinite(x) || !Number.isFinite(y)) continue;
            if (x < minX) minX = x;
            if (y < minY) minY = y;
            if (x > maxX) maxX = x;
            if (y > maxY) maxY = y;
        }
        if (!Number.isFinite(minX) || !Number.isFinite(minY) || !Number.isFinite(maxX) || !Number.isFinite(maxY)) return false;
        return minX <= -0.98 && maxX >= 0.98 && minY <= -0.98 && maxY >= 0.98;
    }

    function __trueosLogLayoutOnce(pipeline, msg) {
        if (!TRACE_LAYOUT) return;
        const rs = G.__trueosRendererState;
        if (rs.layoutLogCount > 32) return;
        const pid = Number(pipeline && pipeline.__id ? pipeline.__id : 0) | 0;
        const key = `${pid}:${msg}`;
        if (rs.layoutLoggedByPipeline[key]) return;
        rs.layoutLoggedByPipeline[key] = true;
        rs.layoutLogCount = (rs.layoutLogCount + 1) >>> 0;
        console.log(`[submit-layout] ${key}`);
    }

    function __trueosLogPipelineDetailOnce(pipeline, vbSlots, tag) {
        if (!TRACE_LAYOUT) return;
        const rs = G.__trueosRendererState;
        if (rs.layoutLogCount > 48) return;
        const pid = Number(pipeline && pipeline.__id ? pipeline.__id : 0) | 0;
        const key = `${pid}:${tag}:detail`;
        if (rs.layoutLoggedByPipeline[key]) return;
        rs.layoutLoggedByPipeline[key] = true;
        rs.layoutLogCount = (rs.layoutLogCount + 1) >>> 0;

        try {
            const v = pipeline && pipeline.__desc && pipeline.__desc.vertex;
            const p = pipeline && pipeline.__desc && pipeline.__desc.primitive;
            const buffers = Array.isArray(v && v.buffers) ? v.buffers : [];
            const parts = [];
            for (let slot = 0; slot < buffers.length; slot++) {
                const b = buffers[slot] || {};
                const attrs = Array.isArray(b.attributes) ? b.attributes : [];
                const attrStr = attrs
                    .map((a) => `loc${Number(a.shaderLocation)}:${String(a.format)}@${Number(a.offset || 0)}`)
                    .join('|');
                parts.push(`slot${slot}[stride=${Number(b.arrayStride || 0)},step=${String(b.stepMode || 'vertex')},attrs=${attrStr}]`);
            }
            const vbState = Object.keys(vbSlots)
                .map((k) => {
                    const s = vbSlots[k];
                    const len = s && s.buffer && s.buffer.__data instanceof Uint8Array ? s.buffer.__data.byteLength : 0;
                    return `slot${k}{off=${Number(s && s.offset || 0)},len=${len}}`;
                })
                .join(' ');
            console.log(
                `[submit-layout-detail] pid=${pid} tag=${tag} topo=${String((p && p.topology) || 'triangle-list')} ${parts.join(' ')} vb=${vbState}`
            );
        } catch (_) {
            console.log(`[submit-layout-detail] pid=${pid} tag=${tag} detail-error`);
        }
    }

    function __trueosF16ToF32(u16) {
        const s = (u16 & 0x8000) ? -1 : 1;
        const e = (u16 >> 10) & 0x1f;
        const f = u16 & 0x03ff;
        if (e === 0) {
            if (f === 0) return s * 0;
            return s * Math.pow(2, -14) * (f / 1024);
        }
        if (e === 31) {
            if (f === 0) return s * Infinity;
            return NaN;
        }
        return s * Math.pow(2, e - 15) * (1 + (f / 1024));
    }

    function __trueosReadAttr(data, byteOffset, format) {
        if (!(data instanceof Uint8Array) || byteOffset < 0 || byteOffset >= data.byteLength) return null;
        const dv = new DataView(data.buffer, data.byteOffset, data.byteLength);
        switch (String(format || '')) {
            case 'float32x2':
                if (byteOffset + 8 > data.byteLength) return null;
                return [dv.getFloat32(byteOffset + 0, true), dv.getFloat32(byteOffset + 4, true)];
            case 'float32x3':
                if (byteOffset + 12 > data.byteLength) return null;
                return [
                    dv.getFloat32(byteOffset + 0, true),
                    dv.getFloat32(byteOffset + 4, true),
                    dv.getFloat32(byteOffset + 8, true),
                ];
            case 'float32x4':
                if (byteOffset + 16 > data.byteLength) return null;
                return [
                    dv.getFloat32(byteOffset + 0, true),
                    dv.getFloat32(byteOffset + 4, true),
                    dv.getFloat32(byteOffset + 8, true),
                    dv.getFloat32(byteOffset + 12, true),
                ];
            case 'float16x2': {
                if (byteOffset + 4 > data.byteLength) return null;
                const a0 = dv.getUint16(byteOffset + 0, true);
                const a1 = dv.getUint16(byteOffset + 2, true);
                return [__trueosF16ToF32(a0), __trueosF16ToF32(a1)];
            }
            case 'float16x4': {
                if (byteOffset + 8 > data.byteLength) return null;
                const a0 = dv.getUint16(byteOffset + 0, true);
                const a1 = dv.getUint16(byteOffset + 2, true);
                const a2 = dv.getUint16(byteOffset + 4, true);
                const a3 = dv.getUint16(byteOffset + 6, true);
                return [
                    __trueosF16ToF32(a0),
                    __trueosF16ToF32(a1),
                    __trueosF16ToF32(a2),
                    __trueosF16ToF32(a3),
                ];
            }
            case 'unorm8x4':
                if (byteOffset + 4 > data.byteLength) return null;
                return [
                    data[byteOffset + 0] / 255,
                    data[byteOffset + 1] / 255,
                    data[byteOffset + 2] / 255,
                    data[byteOffset + 3] / 255,
                ];
            case 'bgra8unorm':
                if (byteOffset + 4 > data.byteLength) return null;
                return [
                    data[byteOffset + 2] / 255,
                    data[byteOffset + 1] / 255,
                    data[byteOffset + 0] / 255,
                    data[byteOffset + 3] / 255,
                ];
            case 'uint8x4':
                if (byteOffset + 4 > data.byteLength) return null;
                return [
                    data[byteOffset + 0],
                    data[byteOffset + 1],
                    data[byteOffset + 2],
                    data[byteOffset + 3],
                ];
            case 'uint16x2':
                if (byteOffset + 4 > data.byteLength) return null;
                return [
                    dv.getUint16(byteOffset + 0, true),
                    dv.getUint16(byteOffset + 2, true),
                ];
            default:
                return null;
        }
    }

    function __trueosSampleVec2Attr(attr, vbSlots) {
        const slotState = vbSlots[attr.slot];
        if (!slotState || !(slotState.buffer && slotState.buffer.__data instanceof Uint8Array)) return null;
        const data = slotState.buffer.__data;
        const baseOff = Math.max(0, Number(slotState.offset || 0) | 0);
        const stride = Math.max(1, Number(attr.arrayStride || 1) | 0);
        let minV = Infinity;
        let maxV = -Infinity;
        let finite = 0;
        let in01 = 0;
        let samples = 0;
        for (let i = 0; i < 8; i++) {
            const off = baseOff + (i * stride) + attr.offset;
            const v = __trueosReadAttr(data, off, attr.format);
            if (!v || v.length < 2) break;
            const x = Number(v[0]);
            const y = Number(v[1]);
            if (!Number.isFinite(x) || !Number.isFinite(y)) continue;
            finite++;
            if (x < minV) minV = x;
            if (y < minV) minV = y;
            if (x > maxV) maxV = x;
            if (y > maxV) maxV = y;
            if (x >= -0.01 && x <= 1.01 && y >= -0.01 && y <= 1.01) in01++;
            samples++;
        }
        if (samples <= 0 || finite <= 0) return null;
        return {
            samples,
            finite,
            minV,
            maxV,
            span: maxV - minV,
            in01Ratio: in01 / samples,
        };
    }

    function __trueosBuildDecodePlan(pipeline, vbSlots) {
        const v = pipeline && pipeline.__desc && pipeline.__desc.vertex;
        const buffers = Array.isArray(v && v.buffers) ? v.buffers : [];
        if (buffers.length === 0) return null;

        const attrs = [];
        for (let slot = 0; slot < buffers.length; slot++) {
            const b = buffers[slot];
            if (!b || String(b.stepMode || 'vertex') !== 'vertex') continue;
            const arrStride = Math.max(1, Number(b.arrayStride || 0) | 0);
            const list = Array.isArray(b.attributes) ? b.attributes : [];
            for (let i = 0; i < list.length; i++) {
                const a = list[i] || {};
                attrs.push({
                    slot,
                    shaderLocation: Math.max(0, Number(a.shaderLocation || 0) | 0),
                    offset: Math.max(0, Number(a.offset || 0) | 0),
                    format: String(a.format || ''),
                    arrayStride: arrStride,
                });
            }
        }

        const isVec2Like = (fmt) => {
            const f = String(fmt || '');
            return f === 'float32x2' || f === 'float16x2' || f === 'float32x3';
        };
        const isColorLike = (fmt) => {
            const f = String(fmt || '');
            return f === 'unorm8x4' || f === 'bgra8unorm' || f === 'uint8x4' || f === 'float32x4' || f === 'float16x4';
        };
        const isTexPageLike = (fmt) => String(fmt || '') === 'uint16x2';

        // Prefer explicit conventional locations only if their format matches expected semantic class.
        const byLoc = (loc) => attrs.find((a) => a.shaderLocation === loc) || null;
        const vec2Attrs = attrs.filter((a) => isVec2Like(a.format));

        let pos = null;
        let uv = null;

        const loc0 = byLoc(0);
        if (loc0 && isVec2Like(loc0.format)) pos = loc0;

        if (!pos && vec2Attrs.length > 0) {
            // Use sampled value distribution: UVs are usually [0..1], positions are usually wider.
            let ranked = vec2Attrs
                .map((a) => ({ a, s: __trueosSampleVec2Attr(a, vbSlots) }))
                .filter((x) => !!x.s)
                .map((x) => ({
                    a: x.a,
                    s: x.s,
                    // Larger span and lower in01 ratio look more like positions.
                    posScore: (x.s.span || 0) + ((1 - x.s.in01Ratio) * 2),
                    uvScore: (x.s.in01Ratio * 2) - (x.s.span || 0),
                }));
            if (ranked.length > 0) {
                ranked.sort((l, r) => r.posScore - l.posScore);
                pos = ranked[0].a;
                const others = ranked.filter((x) => x.a !== pos);
                if (others.length > 0) {
                    others.sort((l, r) => r.uvScore - l.uvScore);
                    uv = others[0].a;
                }
            }
        }

        if (!uv) {
            const loc1 = byLoc(1);
            if (loc1 && isVec2Like(loc1.format) && (!pos || loc1.slot !== pos.slot || loc1.offset !== pos.offset)) {
                uv = loc1;
            }
        }
        if (!uv) {
            uv = vec2Attrs.find((a) => (!pos || a.slot !== pos.slot || a.offset !== pos.offset)) || null;
        }

        let color = byLoc(2);
        if (!color || !isColorLike(color.format)) {
            color = attrs.find((a) => isColorLike(a.format)) || null;
        }

        let texPage = byLoc(2);
        if (!texPage || !isTexPageLike(texPage.format)) {
            texPage = attrs.find((a) => isTexPageLike(a.format)) || null;
        }

        if (!pos) return null;

        // Verify required slot state exists.
        const posSlot = vbSlots[pos.slot];
        if (!posSlot || !(posSlot.buffer && posSlot.buffer.__data instanceof Uint8Array)) return null;
        if (uv) {
            const uvSlot = vbSlots[uv.slot];
            if (!uvSlot || !(uvSlot.buffer && uvSlot.buffer.__data instanceof Uint8Array)) return null;
        }
        if (color) {
            const cSlot = vbSlots[color.slot];
            if (!cSlot || !(cSlot.buffer && cSlot.buffer.__data instanceof Uint8Array)) return null;
        }

        return {
            pos,
            uv,
            color,
            texPage,
            topology: String((pipeline && pipeline.__desc && pipeline.__desc.primitive && pipeline.__desc.primitive.topology) || 'triangle-list'),
        };
    }

    function __trueosLogDecodedSampleOnce(pipeline, plan, verts20) {
        if (!TRACE_LAYOUT) return;
        if (!(verts20 instanceof Uint8Array) || verts20.byteLength < 20) return;
        const rs = G.__trueosRendererState;
        if (rs.layoutLogCount > 56) return;
        const pid = Number(pipeline && pipeline.__id ? pipeline.__id : 0) | 0;
        const key = `${pid}:decoded-sample`;
        if (rs.layoutLoggedByPipeline[key]) return;
        rs.layoutLoggedByPipeline[key] = true;
        rs.layoutLogCount = (rs.layoutLogCount + 1) >>> 0;

        const dv = new DataView(verts20.buffer, verts20.byteOffset, verts20.byteLength);
        const x = dv.getFloat32(0, true);
        const y = dv.getFloat32(4, true);
        const u = dv.getFloat32(8, true);
        const v = dv.getFloat32(12, true);
        const r = verts20[16] | 0;
        const g = verts20[17] | 0;
        const b = verts20[18] | 0;
        const a = verts20[19] | 0;
        console.log(
            `[submit-layout-sample] pid=${pid} pos=loc${plan.pos.shaderLocation}:${plan.pos.format}@${plan.pos.offset}/s${plan.pos.slot} uv=${plan.uv ? `loc${plan.uv.shaderLocation}:${plan.uv.format}@${plan.uv.offset}/s${plan.uv.slot}` : 'none'} color=${plan.color ? `loc${plan.color.shaderLocation}:${plan.color.format}@${plan.color.offset}/s${plan.color.slot}` : 'none'} texPage=${plan.texPage ? `loc${plan.texPage.shaderLocation}:${plan.texPage.format}@${plan.texPage.offset}/s${plan.texPage.slot}` : 'none'} v0=(${x.toFixed(3)},${y.toFixed(3)}) uv0=(${u.toFixed(3)},${v.toFixed(3)}) rgba0=${r},${g},${b},${a}`
        );
    }

    function __trueosClassifyPipelineFamily(pipeline) {
        const d = (pipeline && pipeline.__desc) || {};
        const primitive = d.primitive || {};
        const topology = String(primitive.topology || 'triangle-list');
        const vertex = d.vertex || {};
        const vb = Array.isArray(vertex.buffers) ? vertex.buffers : [];

        const attrs = [];
        for (let i = 0; i < vb.length; i++) {
            const b = vb[i] || {};
            const list = Array.isArray(b.attributes) ? b.attributes : [];
            for (let j = 0; j < list.length; j++) {
                const a = list[j] || {};
                attrs.push(`${Number(a.shaderLocation || 0)}:${String(a.format || '')}@${Number(a.offset || 0)}`);
            }
        }
        attrs.sort();

        const frag = d.fragment || {};
        const targets = Array.isArray(frag.targets) ? frag.targets : [];
        const t0 = targets[0] || {};
        const fmt = String(t0.format || 'unknown');
        const blend = t0.blend ? 'blend' : 'noblend';

        return `topo=${topology}|fmt=${fmt}|${blend}|attrs=${attrs.join(',')}`;
    }

    function __trueosTrackPipelineFamily(pipeline) {
        if (!TRACE_PIPELINES || !pipeline) return;
        const rs = G.__trueosRendererState;
        const sig = __trueosClassifyPipelineFamily(pipeline);
        const fam = rs.pipelineFamilies[sig];
        if (!fam) {
            rs.pipelineFamilies[sig] = {
                count: 1,
                firstPipelineId: Number(pipeline.__id || 0) | 0,
            };
            console.log(`[submit-pipeline] new ${sig} pid=${Number(pipeline.__id || 0) | 0}`);
        } else {
            fam.count = (Number(fam.count || 0) + 1) >>> 0;
        }
    }

    function __trueosLogBindGroupOnce(pipeline, slot, bg) {
        if (!TRACE_BINDGROUPS || !pipeline || !bg) return;
        const rs = G.__trueosRendererState;
        const pid = Number(pipeline.__id || 0) | 0;
        const key = `${pid}:slot${slot}`;
        if (rs.bindGroupLogByPipeline[key]) return;
        rs.bindGroupLogByPipeline[key] = true;

        const parts = [];
        const entries = Array.isArray(bg.entries) ? bg.entries : [];
        for (let i = 0; i < entries.length; i++) {
            const e = entries[i] || {};
            const r = e.resource;
            if (!r) {
                parts.push(`b${Number(e.binding || 0)}:none`);
                continue;
            }
            if (r.buffer && r.buffer.__data instanceof Uint8Array) {
                const bo = Math.max(0, Number(r.offset || 0) | 0);
                const size = Math.max(0, Number(r.size || (r.buffer.__data.byteLength - bo)) | 0);
                const end = Math.min(r.buffer.__data.byteLength, bo + size);
                const slice = r.buffer.__data.subarray(bo, end);
                const dv = new DataView(slice.buffer, slice.byteOffset, slice.byteLength);
                const f = [];
                const maxF = Math.min(8, (slice.byteLength / 4) | 0);
                for (let k = 0; k < maxF; k++) {
                    f.push(dv.getFloat32(k * 4, true).toFixed(3));
                }
                parts.push(`b${Number(e.binding || 0)}:buffer bytes=${slice.byteLength} f32=[${f.join(',')}]`);
                continue;
            }
            if (r.texture || (r.view && r.view.texture)) {
                const t = r.texture || (r.view && r.view.texture);
                parts.push(`b${Number(e.binding || 0)}:texture id=${Number(t && t.__id || 0)} ${Number(t && t.width || 0)}x${Number(t && t.height || 0)}`);
                continue;
            }
            if (r.sampler) {
                parts.push(`b${Number(e.binding || 0)}:sampler`);
                continue;
            }
            parts.push(`b${Number(e.binding || 0)}:other`);
        }
        console.log(`[submit-bindgroup] pid=${pid} slot=${slot} ${parts.join(' ; ')}`);
    }

    function __trueosDecodeVertexTo20(plan, vbSlots, vertexIndex, viewportW, viewportH) {
        const pSlot = vbSlots[plan.pos.slot];
        const pData = pSlot.buffer.__data;
        const pBase = (Math.max(0, Number(pSlot.offset || 0) | 0)) + (vertexIndex * plan.pos.arrayStride) + plan.pos.offset;
        const p = __trueosReadAttr(pData, pBase, plan.pos.format);
        if (!p || p.length < 2) return null;

        let x = Number(p[0]);
        let y = Number(p[1]);
        if (!Number.isFinite(x) || !Number.isFinite(y)) return null;

        // Heuristic: if not already clip-like, assume pixel coordinates and project.
        if (!(x >= -1.5 && x <= 1.5 && y >= -1.5 && y <= 1.5)) {
            const w = Math.max(1, Number(viewportW || 1) | 0);
            const h = Math.max(1, Number(viewportH || 1) | 0);
            x = (2.0 * (x / w)) - 1.0;
            y = 1.0 - (2.0 * (y / h));
        }

        let u = 0;
        let v = 0;
        if (plan.uv) {
            const uSlot = vbSlots[plan.uv.slot];
            const uData = uSlot.buffer.__data;
            const uBase = (Math.max(0, Number(uSlot.offset || 0) | 0)) + (vertexIndex * plan.uv.arrayStride) + plan.uv.offset;
            const uv = __trueosReadAttr(uData, uBase, plan.uv.format);
            if (!uv || uv.length < 2) return null;
            u = Number(uv[0]) || 0;
            v = Number(uv[1]) || 0;
        }

        let r = 255;
        let g = 255;
        let b = 255;
        let a = 255;
        if (plan.color) {
            const cSlot = vbSlots[plan.color.slot];
            const cData = cSlot.buffer.__data;
            const cBase = (Math.max(0, Number(cSlot.offset || 0) | 0)) + (vertexIndex * plan.color.arrayStride) + plan.color.offset;
            const col = __trueosReadAttr(cData, cBase, plan.color.format);
            if (!col) return null;
            if (String(plan.color.format) === 'uint8x4') {
                r = (Number(col[0]) || 0) & 0xff;
                g = (Number(col[1]) || 0) & 0xff;
                b = (Number(col[2]) || 0) & 0xff;
                a = (Number(col[3]) || 0) & 0xff;
            } else {
                r = Math.round(__trueosClamp01(Number(col[0])) * 255) & 0xff;
                g = Math.round(__trueosClamp01(Number(col[1])) * 255) & 0xff;
                b = Math.round(__trueosClamp01(Number(col[2])) * 255) & 0xff;
                a = Math.round(__trueosClamp01(Number((col.length > 3) ? col[3] : 1)) * 255) & 0xff;
            }
            if ((r | g | b | a) === 0) {
                r = 255;
                g = 255;
                b = 255;
                a = 255;
            }
        }

        let texPageHint = 0;
        if (plan.texPage) {
            const tSlot = vbSlots[plan.texPage.slot];
            const tData = tSlot && tSlot.buffer && tSlot.buffer.__data;
            if (tData instanceof Uint8Array) {
                const tBase = (Math.max(0, Number(tSlot.offset || 0) | 0)) + (vertexIndex * plan.texPage.arrayStride) + plan.texPage.offset;
                const t = __trueosReadAttr(tData, tBase, plan.texPage.format);
                if (t && t.length > 0) {
                    texPageHint = Math.max(0, Number(t[0]) | 0);
                }
            }
        }

        return { x, y, u, v, r, g, b, a, texPageHint };
    }

    function __trueosPackVertices20(vertices) {
        const out = new Uint8Array(vertices.length * 20);
        const dv = new DataView(out.buffer);
        let off = 0;
        for (let i = 0; i < vertices.length; i++) {
            const v = vertices[i];
            dv.setFloat32(off + 0, v.x, true);
            dv.setFloat32(off + 4, v.y, true);
            dv.setFloat32(off + 8, v.u, true);
            dv.setFloat32(off + 12, v.v, true);
            out[off + 16] = v.r & 0xff;
            out[off + 17] = v.g & 0xff;
            out[off + 18] = v.b & 0xff;
            out[off + 19] = v.a & 0xff;
            off += 20;
        }
        return out;
    }

    function __trueosConvert20ToRgb12(bytes20) {
        if (!(bytes20 instanceof Uint8Array)) return null;
        const n = Math.floor(bytes20.byteLength / 20);
        if (n <= 0) return null;
        const out = new Uint8Array(n * 12);
        const srcDv = new DataView(bytes20.buffer, bytes20.byteOffset, bytes20.byteLength);
        const dstDv = new DataView(out.buffer);
        let so = 0;
        let doff = 0;
        for (let i = 0; i < n; i++) {
            dstDv.setFloat32(doff + 0, srcDv.getFloat32(so + 0, true), true);
            dstDv.setFloat32(doff + 4, srcDv.getFloat32(so + 4, true), true);
            out[doff + 8] = bytes20[so + 16] | 0;
            out[doff + 9] = bytes20[so + 17] | 0;
            out[doff + 10] = bytes20[so + 18] | 0;
            out[doff + 11] = FORCE_OPAQUE_ALPHA ? 255 : (bytes20[so + 19] | 0);
            so += 20;
            doff += 12;
        }
        return out;
    }

    function __trueosHexBytes(u8, off, len) {
        if (!(u8 instanceof Uint8Array)) return '';
        const s = Math.max(0, Number(off) | 0);
        const e = Math.min(u8.byteLength, s + Math.max(0, Number(len) | 0));
        const out = [];
        for (let i = s; i < e; i++) out.push((u8[i] & 0xff).toString(16).padStart(2, '0'));
        return out.join('');
    }

    function __trueosDumpVertexFrameOnce(drawMetaList) {
        if (!TRACE_VERTEX_FRAMES) return;
        const rs = G.__trueosRendererState;
        if (rs.vertexDumpDone) return;
        if (!Array.isArray(drawMetaList) || drawMetaList.length === 0) return;

        rs.vertexDumpDone = true;
        console.log(`[vertex-frame] begin draws=${drawMetaList.length}`);
        for (let di = 0; di < drawMetaList.length; di++) {
            const dm = drawMetaList[di] || {};
            const bytes = dm.bytes;
            const pid = Number(dm.pid || 0) | 0;
            const kind = String(dm.kind || 'draw');
            const idx = Array.isArray(dm.vertexIndices) ? dm.vertexIndices : [];
            if (!(bytes instanceof Uint8Array) || bytes.byteLength < 20) {
                console.log(`[vertex-frame] draw=${di} pid=${pid} op=${kind} empty`);
                continue;
            }
            const vcount = Math.floor(bytes.byteLength / 20);
            const dv = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
            console.log(`[vertex-frame] draw=${di} pid=${pid} op=${kind} verts=${vcount} bytes=${bytes.byteLength}`);
            for (let i = 0; i < vcount; i++) {
                const off = i * 20;
                const x = dv.getFloat32(off + 0, true);
                const y = dv.getFloat32(off + 4, true);
                const u = dv.getFloat32(off + 8, true);
                const v = dv.getFloat32(off + 12, true);
                const r = bytes[off + 16] | 0;
                const g = bytes[off + 17] | 0;
                const b = bytes[off + 18] | 0;
                const a = bytes[off + 19] | 0;
                const src = (i < idx.length) ? idx[i] : -1;
                const raw20 = __trueosHexBytes(bytes, off, 20);
                console.log(`[vertex-frame] d=${di} v=${i} src=${src} pos=(${x.toFixed(6)},${y.toFixed(6)}) uv=(${u.toFixed(6)},${v.toFixed(6)}) rgba=${r},${g},${b},${a} raw20=${raw20}`);
            }
        }
        console.log('[vertex-frame] end');
    }

    function __trueosBuildDebugRgbQuad() {
        // 2D RGB vertex format expected by cmd_stream.drawTrianglesU8: [x:f32, y:f32, r:u8,g:u8,b:u8,a:u8]
        const out = new Uint8Array(6 * 12);
        const dv = new DataView(out.buffer);
        let off = 0;
        const push = (x, y, r, g, b, a) => {
            dv.setFloat32(off + 0, x, true);
            dv.setFloat32(off + 4, y, true);
            out[off + 8] = r & 0xff;
            out[off + 9] = g & 0xff;
            out[off + 10] = b & 0xff;
            out[off + 11] = a & 0xff;
            off += 12;
        };

        // Small top-left marker quad, high-contrast red.
        const x0 = -0.95;
        const y0 = 0.95;
        const x1 = -0.75;
        const y1 = 0.75;
        const r = 235;
        const g = 40;
        const b = 40;
        const a = 255;

        push(x0, y1, r, g, b, a);
        push(x1, y1, r, g, b, a);
        push(x1, y0, r, g, b, a);
        push(x0, y1, r, g, b, a);
        push(x1, y0, r, g, b, a);
        push(x0, y0, r, g, b, a);
        return out;
    }

    function __trueosBuildDebugCursorCross(cursor, viewportW, viewportH) {
        const w = Math.max(1, Number(viewportW || 1) | 0);
        const h = Math.max(1, Number(viewportH || 1) | 0);
        const srcX = Number(cursor && cursor.x);
        const srcY = Number(cursor && cursor.y);
        const cxPx = Number.isFinite(srcX) ? srcX : (w * 0.5);
        const cyPx = Number.isFinite(srcY) ? srcY : (h * 0.5);

        const armPx = 18;
        const halfThickPx = 4;
        const marginPx = armPx + halfThickPx + 2;
        const clampedX = Math.min(w - marginPx, Math.max(marginPx, cxPx));
        const clampedY = Math.min(h - marginPx, Math.max(marginPx, cyPx));
        const xScale = 2.0 / w;
        const yScale = 2.0 / h;

        const cx = (2.0 * (clampedX / w)) - 1.0;
        const cy = 1.0 - (2.0 * (clampedY / h));
        const armX = armPx * xScale;
        const armY = armPx * yScale;
        const tx = halfThickPx * xScale;
        const ty = halfThickPx * yScale;

        const out = new Uint8Array(12 * 12);
        const dv = new DataView(out.buffer);
        let off = 0;
        const r = 56;
        const g = 196;
        const b = 245;
        const a = 255;

        const push = (x, y) => {
            dv.setFloat32(off + 0, x, true);
            dv.setFloat32(off + 4, y, true);
            out[off + 8] = r;
            out[off + 9] = g;
            out[off + 10] = b;
            out[off + 11] = a;
            off += 12;
        };
        const pushRect = (x0, y0, x1, y1) => {
            push(x0, y1);
            push(x1, y1);
            push(x1, y0);
            push(x0, y1);
            push(x1, y0);
            push(x0, y0);
        };

        // Horizontal and vertical bars to form a crosshair.
        pushRect(cx - armX, cy - ty, cx + armX, cy + ty);
        pushRect(cx - tx, cy - armY, cx + tx, cy + armY);
        return out;
    }

    function __trueosDecodeDrawVertices(plan, vbSlots, firstVertex, vertexCount, viewportW, viewportH) {
        const n = Math.max(0, Number(vertexCount) | 0);
        if (n <= 0) {
            G.__trueosLastDecodeFail = 'draw:no-vertices';
            return null;
        }
        const first = Math.max(0, Number(firstVertex) | 0);
        const verts = new Array(n);
        const vertexIndices = new Array(n);
        let texPageHint = 0;
        for (let i = 0; i < n; i++) {
            const srcIdx = first + i;
            const d = __trueosDecodeVertexTo20(plan, vbSlots, srcIdx, viewportW, viewportH);
            if (!d) {
                G.__trueosLastDecodeFail = `draw:vertex-decode i=${i} v=${srcIdx}`;
                return null;
            }
            if (i === 0) texPageHint = Math.max(0, Number(d.texPageHint || 0) | 0);
            verts[i] = d;
            vertexIndices[i] = srcIdx;
        }
        return {
            bytes: __trueosPackVertices20(verts),
            texPageHint,
            vertexIndices,
        };
    }

    function __trueosDecodeIndexedVertices(plan, vbSlots, ibState, indexFormat, firstIndex, indexCount, baseVertex, viewportW, viewportH) {
        if (!ibState || !(ibState.buffer && ibState.buffer.__data instanceof Uint8Array)) {
            G.__trueosLastDecodeFail = 'indexed:no-index-buffer';
            return null;
        }
        const ib = ibState.buffer.__data;
        const idxSize = (String(indexFormat || 'uint16') === 'uint32') ? 4 : 2;
        const n = Math.max(0, Number(indexCount) | 0);
        if (n <= 0) {
            G.__trueosLastDecodeFail = 'indexed:no-indices';
            return null;
        }
        const start = (Math.max(0, Number(ibState.offset || 0) | 0)) + (Math.max(0, Number(firstIndex) | 0) * idxSize);
        const end = start + (n * idxSize);
        if (end > ib.byteLength) {
            G.__trueosLastDecodeFail = `indexed:oob start=${start} end=${end} ib=${ib.byteLength} idxSize=${idxSize} first=${firstIndex} count=${n}`;
            return null;
        }

        const base = Number(baseVertex || 0) | 0;
        const triCount = Math.floor(n / 3);
        if (triCount <= 0) {
            G.__trueosLastDecodeFail = `indexed:insufficient-indices count=${n}`;
            return null;
        }

        const verts = [];
        const vertexIndices = [];
        let texPageHint = 0;
        let skipped = 0;

        const decodeIndex = (i) => {
            const off = start + (i * idxSize);
            let idx = 0;
            if (idxSize === 2) {
                idx = ib[off] | (ib[off + 1] << 8);
            } else {
                idx = (ib[off]) | (ib[off + 1] << 8) | (ib[off + 2] << 16) | (ib[off + 3] << 24);
                idx >>>= 0;
            }
            idx = (idx + base) | 0;
            if (idx < 0) return null;
            const d = __trueosDecodeVertexTo20(plan, vbSlots, idx, viewportW, viewportH);
            if (!d) return null;
            return { idx, d };
        };

        for (let t = 0; t < triCount; t++) {
            const i0 = t * 3;
            const i1 = i0 + 1;
            const i2 = i0 + 2;
            const a = decodeIndex(i0);
            const b = decodeIndex(i1);
            const c = decodeIndex(i2);
            if (!a || !b || !c) {
                skipped++;
                continue;
            }
            if (verts.length === 0) texPageHint = Math.max(0, Number(a.d.texPageHint || 0) | 0);
            verts.push(a.d, b.d, c.d);
            vertexIndices.push(a.idx, b.idx, c.idx);
        }

        if (verts.length <= 0) {
            G.__trueosLastDecodeFail = `indexed:no-valid-tris total=${triCount} skipped=${skipped}`;
            return null;
        }

        if (skipped > 0) {
            G.__trueosLastDecodeFail = `indexed:salvaged skipped=${skipped}/${triCount}`;
        }

        return {
            bytes: __trueosPackVertices20(verts),
            texPageHint,
            vertexIndices,
        };
    }

    function __trueosExecCopyBufferToBuffer(entry) {
        if (!Array.isArray(entry) || entry[0] !== 'copyBufferToBuffer') return;
        const src = entry[1];
        const srcOff = Math.max(0, Number(entry[2] || 0) | 0);
        const dst = entry[3];
        const dstOff = Math.max(0, Number(entry[4] || 0) | 0);
        const size = Math.max(0, Number(entry[5] || 0) | 0);
        if (!src || !dst || !(src.__data instanceof Uint8Array) || !(dst.__data instanceof Uint8Array)) return;
        if (size <= 0) return;
        if (srcOff >= src.__data.byteLength || dstOff >= dst.__data.byteLength) return;

        const srcEnd = Math.min(src.__data.byteLength, srcOff + size);
        const copy = Math.max(0, srcEnd - srcOff);
        if (copy <= 0) return;
        const dstEnd = Math.min(dst.__data.byteLength, dstOff + copy);
        const n = Math.max(0, dstEnd - dstOff);
        if (n <= 0) return;
        dst.__data.set(src.__data.subarray(srcOff, srcOff + n), dstOff);
    }

    function __trueosExecCopyBufferToTexture(entry, traceTexPath) {
        if (!Array.isArray(entry) || entry[0] !== 'copyBufferToTexture') return;
        const srcDesc = entry[1] || {};
        const dstDesc = entry[2] || {};
        const size = entry[3] || {};

        const srcBuffer = srcDesc.buffer || srcDesc;
        if (!srcBuffer || !(srcBuffer.__data instanceof Uint8Array)) return;
        const tex = dstDesc.texture || dstDesc;
        if (!tex) return;

        const srcOffset = Math.max(0, Number(srcDesc.offset || 0) | 0);
        const bytesPerRow = Math.max(0, Number(srcDesc.bytesPerRow || 0) | 0);
        const rowsPerImage = Math.max(0, Number(srcDesc.rowsPerImage || 0) | 0);

        const width = Math.max(0, Number(size.width || size[0] || tex.width || 0) | 0);
        const height = Math.max(0, Number(size.height || size[1] || tex.height || 0) | 0);
        if (width <= 0 || height <= 0) return;

        const origin = dstDesc.origin || { x: 0, y: 0 };
        const ox = Math.max(0, Number(Array.isArray(origin) ? origin[0] : origin.x || 0) | 0);
        const oy = Math.max(0, Number(Array.isArray(origin) ? origin[1] : origin.y || 0) | 0);

        const rowPitch = bytesPerRow > 0 ? bytesPerRow : (width * 4);
        const imageRows = rowsPerImage > 0 ? rowsPerImage : height;
        const needed = srcOffset + (Math.max(0, imageRows - 1) * rowPitch) + (width * 4);
        const end = Math.min(srcBuffer.__data.byteLength, needed);
        if (end <= srcOffset) return;

        const src = srcBuffer.__data.subarray(srcOffset, end);
        if (typeof G.__trueosUploadLinearRgbaToTexture === 'function') {
            G.__trueosUploadLinearRgbaToTexture(tex, src, rowPitch, width, height, ox, oy);
            __trueosMarkTextureUpload(tex);
            if (traceTexPath) {
                const tid = Number(tex.__id || 0) | 0;
                console.log(`[textrace] copy-b2t tex=${tid} size=${width}x${height} origin=${ox},${oy} bpr=${rowPitch} src=${src.byteLength}`);
            }
        }
    }

    function __trueosExecCopyTextureToTexture(entry, traceTexPath) {
        if (!Array.isArray(entry) || entry[0] !== 'copyTextureToTexture') return;
        const srcDesc = entry[1] || {};
        const dstDesc = entry[2] || {};
        const size = entry[3] || {};

        const srcTex = srcDesc.texture || srcDesc;
        const dstTex = dstDesc.texture || dstDesc;
        if (!srcTex || !dstTex || !(srcTex.__data instanceof Uint8Array)) return;

        const width = Math.max(0, Number(size.width || size[0] || srcTex.width || dstTex.width || 0) | 0);
        const height = Math.max(0, Number(size.height || size[1] || srcTex.height || dstTex.height || 0) | 0);
        if (width <= 0 || height <= 0) return;

        const srcOrigin = srcDesc.origin || { x: 0, y: 0 };
        const dstOrigin = dstDesc.origin || { x: 0, y: 0 };
        const sx = Math.max(0, Number(Array.isArray(srcOrigin) ? srcOrigin[0] : srcOrigin.x || 0) | 0);
        const sy = Math.max(0, Number(Array.isArray(srcOrigin) ? srcOrigin[1] : srcOrigin.y || 0) | 0);
        const dx = Math.max(0, Number(Array.isArray(dstOrigin) ? dstOrigin[0] : dstOrigin.x || 0) | 0);
        const dy = Math.max(0, Number(Array.isArray(dstOrigin) ? dstOrigin[1] : dstOrigin.y || 0) | 0);

        const srcW = Math.max(1, Number(srcTex.width || 1) | 0);
        const srcH = Math.max(1, Number(srcTex.height || 1) | 0);
        const rowPitch = srcW * 4;
        if (sx >= srcW || sy >= srcH) return;

        const maxW = Math.max(0, srcW - sx);
        const maxH = Math.max(0, srcH - sy);
        const copyW = Math.min(width, maxW);
        const copyH = Math.min(height, maxH);
        if (copyW <= 0 || copyH <= 0) return;

        const srcOff = ((sy * srcW) + sx) * 4;
        const need = srcOff + ((copyH - 1) * rowPitch) + (copyW * 4);
        if (need > srcTex.__data.byteLength) return;

        const src = srcTex.__data.subarray(srcOff, need);
        if (typeof G.__trueosUploadLinearRgbaToTexture === 'function') {
            G.__trueosUploadLinearRgbaToTexture(dstTex, src, rowPitch, copyW, copyH, dx, dy);
            __trueosMarkTextureUpload(dstTex);
            if (traceTexPath) {
                const sid = Number(srcTex.__id || 0) | 0;
                const did = Number(dstTex.__id || 0) | 0;
                console.log(`[textrace] copy-t2t src=${sid} dst=${did} size=${copyW}x${copyH} srcO=${sx},${sy} dstO=${dx},${dy}`);
            }
        }
    }

    function __trueosExecuteSubmit(cmd, cmdBuffers, summary) {
        let began = false;
        let draws = 0;
        const dumpRows = [];
        const rs = G.__trueosRendererState;
        const traceTexPath = TRACE_TEXPATH && !rs.texTraceDone;
        let traceSawDrawOp = false;
        if (traceTexPath) {
            console.log(`[textrace] begin vp=${summary.viewportW}x${summary.viewportH} pass=${summary.passCount} ops=${summary.opCount} draws=${summary.drawCount}`);
        }

        let curPipeline = null;
        const vbSlots = {};
        let ibState = null;
        let curIndexFormat = 'uint16';
        const bindGroups = [null, null, null, null];

        for (let i = 0; i < cmdBuffers.length; i++) {
            const cb = cmdBuffers[i];
            if (!cb || !Array.isArray(cb.__cmds)) continue;
            const cmds = cb.__cmds;
            for (let j = 0; j < cmds.length; j++) {
                const pass = cmds[j];
                if (Array.isArray(pass)) {
                    __trueosExecCopyBufferToBuffer(pass);
                    __trueosExecCopyBufferToTexture(pass, traceTexPath);
                    __trueosExecCopyTextureToTexture(pass, traceTexPath);
                    continue;
                }
                if (!pass || pass.__kind !== 'render-pass') continue;
                const rawOps = Array.isArray(pass.__ops) ? pass.__ops : [];
                const ops = [];
                for (let rk = 0; rk < rawOps.length; rk++) {
                    const rop = rawOps[rk];
                    if (!Array.isArray(rop) || rop.length === 0) continue;
                    if (rop[0] === 'executeBundles') {
                        const bundles = Array.isArray(rop[1]) ? rop[1] : [];
                        for (let bi = 0; bi < bundles.length; bi++) {
                            const b = bundles[bi];
                            const bOps = (b && Array.isArray(b.__ops)) ? b.__ops : [];
                            for (let bo = 0; bo < bOps.length; bo++) {
                                const bop = bOps[bo];
                                if (!Array.isArray(bop) || bop.length === 0) continue;
                                ops.push(bop);
                            }
                        }
                        continue;
                    }
                    ops.push(rop);
                }

                for (let k = 0; k < ops.length; k++) {
                    const op = ops[k];
                    if (!Array.isArray(op) || op.length === 0) continue;
                    const kind = op[0];

                    if (kind === 'setBindGroup') {
                        const slot = Math.max(0, Number(op[1]) | 0);
                        if (slot < bindGroups.length) {
                            bindGroups[slot] = op[2] || null;
                            __trueosLogBindGroupOnce(curPipeline, slot, bindGroups[slot]);
                        }
                        continue;
                    }

                    if (kind === 'setPipeline') {
                        curPipeline = op[1] || null;
                        __trueosTrackPipelineFamily(curPipeline);
                        continue;
                    }

                    if (kind === 'setVertexBuffer') {
                        const slot = Math.max(0, Number(op[1]) | 0);
                        vbSlots[slot] = {
                            buffer: op[2] || null,
                            offset: Math.max(0, Number(op[3] || 0) | 0),
                        };
                        continue;
                    }

                    if (kind === 'setIndexBuffer') {
                        ibState = {
                            buffer: op[1] || null,
                            offset: Math.max(0, Number(op[3] || 0) | 0),
                        };
                        curIndexFormat = String(op[2] || 'uint16');
                        continue;
                    }

                    let decoded = null;
                    const plan = __trueosBuildDecodePlan(curPipeline, vbSlots);
                    if (!plan) {
                        if (traceTexPath && (kind === 'draw' || kind === 'drawIndexed')) {
                            const pid = Number(curPipeline && curPipeline.__id || 0) | 0;
                            console.log(`[textrace] skip pid=${pid} op=${kind} reason=no-decode-plan`);
                        }
                        __trueosLogLayoutOnce(curPipeline, 'no-decode-plan');
                        __trueosLogPipelineDetailOnce(curPipeline, vbSlots, 'no-decode-plan');
                        continue;
                    }

                    if (kind === 'draw') {
                        traceSawDrawOp = traceSawDrawOp || traceTexPath;
                        const vtxCount = Math.max(0, Number(op[1]) | 0);
                        const instCount = Math.max(0, Number(op[2] == null ? 1 : op[2]) | 0);
                        const firstV = Math.max(0, Number(op[3] || 0) | 0);
                        if (instCount !== 1 || vtxCount <= 0 || plan.topology !== 'triangle-list') {
                            if (traceTexPath) {
                                const pid = Number(curPipeline && curPipeline.__id || 0) | 0;
                                console.log(`[textrace] skip pid=${pid} op=draw reason=topo-inst topo=${plan.topology} inst=${instCount} vtx=${vtxCount}`);
                            }
                            __trueosLogLayoutOnce(curPipeline, `skip-draw topo=${plan.topology} inst=${instCount}`);
                            continue;
                        }
                        decoded = __trueosDecodeDrawVertices(plan, vbSlots, firstV, vtxCount, summary.viewportW, summary.viewportH);
                    } else if (kind === 'drawIndexed') {
                        traceSawDrawOp = traceSawDrawOp || traceTexPath;
                        const idxCount = Math.max(0, Number(op[1]) | 0);
                        const instCount = Math.max(0, Number(op[2] == null ? 1 : op[2]) | 0);
                        const firstIndex = Math.max(0, Number(op[3] || 0) | 0);
                        const baseVertex = Number(op[4] || 0) | 0;
                        if (instCount !== 1 || idxCount <= 0 || plan.topology !== 'triangle-list') {
                            if (traceTexPath) {
                                const pid = Number(curPipeline && curPipeline.__id || 0) | 0;
                                console.log(`[textrace] skip pid=${pid} op=drawIndexed reason=topo-inst topo=${plan.topology} inst=${instCount} idx=${idxCount}`);
                            }
                            __trueosLogLayoutOnce(curPipeline, `skip-drawIndexed topo=${plan.topology} inst=${instCount}`);
                            continue;
                        }
                        decoded = __trueosDecodeIndexedVertices(plan, vbSlots, ibState, curIndexFormat, firstIndex, idxCount, baseVertex, summary.viewportW, summary.viewportH);
                    } else {
                        continue;
                    }

                    if (!decoded || !(decoded.bytes instanceof Uint8Array) || decoded.bytes.byteLength === 0) {
                        if (traceTexPath) {
                            const pid = Number(curPipeline && curPipeline.__id || 0) | 0;
                            const why = String(G.__trueosLastDecodeFail || 'unknown');
                            console.log(`[textrace] skip pid=${pid} op=${kind} reason=decode-failed why=${why}`);
                        }
                        __trueosLogLayoutOnce(curPipeline, 'decode-failed');
                        __trueosLogPipelineDetailOnce(curPipeline, vbSlots, 'decode-failed');
                        continue;
                    }

                    __trueosLogDecodedSampleOnce(curPipeline, plan, decoded.bytes);
                    if (TRACE_VERTEX_FRAMES && !rs.vertexDumpDone) {
                        dumpRows.push({
                            pid: Number(curPipeline && curPipeline.__id || 0) | 0,
                            kind,
                            bytes: decoded.bytes,
                            vertexIndices: decoded.vertexIndices,
                        });
                    }

                    const bound = traceTexPath ? __trueosCollectBoundTextures(bindGroups) : null;
                    const tex = __trueosFindBoundTexture(bindGroups, decoded.texPageHint);
                    const texId = __trueosEnsureCmdTexture(cmd, tex);
                    const skipEmptyFullscreenCompose =
                        (texId > 0) &&
                        __trueosTexLooksEmpty(tex) &&
                        __trueosDecodedLooksFullscreen(decoded.bytes);
                    if (traceTexPath) {
                        const pid = Number(curPipeline && curPipeline.__id || 0) | 0;
                        const vtx = Math.floor(decoded.bytes.byteLength / 20);
                        const boundList = (bound && bound.length > 0)
                            ? bound.map((t, i) => `${i}:${__trueosTexLabel(t)}`).join(' | ')
                            : 'none';
                        const chosen = __trueosTexLabel(tex);
                        const tstats = __trueosTexStats(tex);
                        const bounds = __trueosDecodedBounds(decoded.bytes);
                        const status = skipEmptyFullscreenCompose
                            ? 'skip-empty-fullscreen'
                            : ((texId > 0) ? 'ok' : 'no-cmd-texture');
                        console.log(
                            `[textrace] draw pid=${pid} op=${kind} verts=${vtx} hint=${Number(decoded.texPageHint || 0) | 0} ndc=${bounds} bound=[${boundList}] chosen={${chosen}} tstats=${tstats} status=${status} texId=${texId}`
                        );
                    }
                    if (skipEmptyFullscreenCompose) {
                        if (!began) {
                            if (typeof cmd.setViewport === 'function') {
                                cmd.setViewport(summary.viewportW | 0, summary.viewportH | 0);
                            }
                            if (summary.clearRgb != null && typeof cmd.setClearRgb === 'function') {
                                cmd.setClearRgb(summary.clearRgb >>> 0);
                            }
                            cmd.beginFrame();
                            began = true;
                        }
                        if (typeof cmd.drawTrianglesU8 === 'function') {
                            const rgb = __trueosConvert20ToRgb12(decoded.bytes);
                            if (rgb && rgb.byteLength > 0) {
                                cmd.setBlendEnabled(false);
                                cmd.drawTrianglesU8(rgb);
                                draws++;
                                if (traceTexPath) {
                                    const pid = Number(curPipeline && curPipeline.__id || 0) | 0;
                                    console.log(`[textrace] salvage-empty-fullscreen pid=${pid} op=${kind} verts=${Math.floor(decoded.bytes.byteLength / 20)}`);
                                }
                            }
                        }
                        continue;
                    }

                    if (!began) {
                        if (typeof cmd.setViewport === 'function') {
                            cmd.setViewport(summary.viewportW | 0, summary.viewportH | 0);
                        }
                        if (summary.clearRgb != null && typeof cmd.setClearRgb === 'function') {
                            cmd.setClearRgb(summary.clearRgb >>> 0);
                        }
                        cmd.beginFrame();
                        began = true;
                    }

                    if (texId > 0) {
                        cmd.drawTexturedTrianglesU8(texId, decoded.bytes);
                        draws++;
                    } else if (typeof cmd.drawTrianglesU8 === 'function') {
                        const rgb = __trueosConvert20ToRgb12(decoded.bytes);
                        if (rgb && rgb.byteLength > 0) {
                            cmd.setBlendEnabled(false);
                            cmd.drawTrianglesU8(rgb);
                            draws++;
                            if (traceTexPath) {
                                const pid = Number(curPipeline && curPipeline.__id || 0) | 0;
                                console.log(`[textrace] fallback-rgb pid=${pid} op=${kind} verts=${Math.floor(decoded.bytes.byteLength / 20)}`);
                            }
                        }
                    }
                }
            }
        }

        if (began) {
            if (FORCE_CURSOR_HEARTBEAT && typeof cmd.drawTrianglesU8 === 'function') {
                const cursor = G.__trueosCursorDebug || null;
                cmd.setBlendEnabled(false);
                cmd.drawTrianglesU8(__trueosBuildDebugCursorCross(cursor, summary.viewportW, summary.viewportH));
            }
            if (FORCE_DEBUG_OVERLAY && typeof cmd.drawTrianglesU8 === 'function') {
                // Overlay a deterministic marker independent of texture/shader semantics.
                cmd.setBlendEnabled(false);
                cmd.drawTrianglesU8(__trueosBuildDebugRgbQuad());
            }
            cmd.endFrame();
        } else if ((summary.drawCount > 0 || summary.opCount > 0)) {
            if (typeof cmd.setViewport === 'function') {
                cmd.setViewport(summary.viewportW | 0, summary.viewportH | 0);
            }
            if (typeof cmd.setClearRgb === 'function') {
                cmd.setClearRgb(summary.clearRgb == null ? 0xFFFFFF : (summary.clearRgb >>> 0));
            }
            cmd.beginFrame();
            if (FORCE_CURSOR_HEARTBEAT && typeof cmd.drawTrianglesU8 === 'function') {
                const cursor = G.__trueosCursorDebug || null;
                cmd.setBlendEnabled(false);
                cmd.drawTrianglesU8(__trueosBuildDebugCursorCross(cursor, summary.viewportW, summary.viewportH));
            }
            cmd.endFrame();
        } else if (FORCE_CURSOR_HEARTBEAT && typeof cmd.drawTrianglesU8 === 'function') {
            if (typeof cmd.setViewport === 'function') {
                cmd.setViewport(summary.viewportW | 0, summary.viewportH | 0);
            }
            if (typeof cmd.setClearRgb === 'function') {
                cmd.setClearRgb(summary.clearRgb == null ? 0xFFFFFF : (summary.clearRgb >>> 0));
            }
            cmd.beginFrame();
            const cursor = G.__trueosCursorDebug || null;
            cmd.setBlendEnabled(false);
            cmd.drawTrianglesU8(__trueosBuildDebugCursorCross(cursor, summary.viewportW, summary.viewportH));
            cmd.endFrame();
        }

        if (traceTexPath) {
            rs.texTraceDone = traceSawDrawOp;
            console.log(`[textrace] end executed=${draws}`);
        }
        __trueosDumpVertexFrameOnce(dumpRows);

        return draws;
    }

    if (typeof G.__trueosMakeGpuCommandEncoder !== 'function') {
        G.__trueosMakeGpuCommandEncoder = function __trueosMakeGpuCommandEncoder(_device, desc = {}) {
            const cmds = [];
            return {
                __id: G.__trueosWebGpuState.nextId++,
                label: String(desc.label || ''),
                __cmds: cmds,
                beginRenderPass(passDesc = {}) {
                    const pass = {
                        __id: G.__trueosWebGpuState.nextId++,
                        __kind: 'render-pass',
                        __desc: passDesc,
                        __ops: [],
                        setPipeline(p) { this.__ops.push(['setPipeline', p]); },
                        setBindGroup(i, bg) { this.__ops.push(['setBindGroup', i, bg]); },
                        setVertexBuffer(slot, b, off = 0, size) { this.__ops.push(['setVertexBuffer', slot, b, off, size]); },
                        setIndexBuffer(b, f = 'uint16', off = 0, size) { this.__ops.push(['setIndexBuffer', b, f, off, size]); },
                        setViewport(x, y, w, h, minD = 0, maxD = 1) { this.__ops.push(['setViewport', x, y, w, h, minD, maxD]); },
                        setScissorRect(x, y, w, h) { this.__ops.push(['setScissorRect', x, y, w, h]); },
                        setStencilReference(v) { this.__ops.push(['setStencilReference', v]); },
                        setBlendConstant(c) { this.__ops.push(['setBlendConstant', c]); },
                        draw(vtxCount, instCount = 1, firstV = 0, firstI = 0) { this.__ops.push(['draw', vtxCount, instCount, firstV, firstI]); },
                        drawIndexed(idxCount, instCount = 1, firstIndex = 0, baseVertex = 0, firstInstance = 0) {
                            this.__ops.push(['drawIndexed', idxCount, instCount, firstIndex, baseVertex, firstInstance]);
                        },
                        executeBundles(bundles = []) { this.__ops.push(['executeBundles', bundles]); },
                        end() {
                            cmds.push(pass);
                        },
                    };
                    return pass;
                },
                copyBufferToBuffer(src, srcOff, dst, dstOff, size) {
                    cmds.push(['copyBufferToBuffer', src, srcOff, dst, dstOff, size]);
                },
                copyBufferToTexture(src, dst, size) {
                    cmds.push(['copyBufferToTexture', src, dst, size]);
                },
                copyTextureToTexture(src, dst, size) {
                    cmds.push(['copyTextureToTexture', src, dst, size]);
                },
                finish(finishDesc = {}) {
                    return {
                        __id: G.__trueosWebGpuState.nextId++,
                        label: String(finishDesc.label || ''),
                        __cmds: cmds.slice(),
                    };
                },
            };
        };
    }

    if (typeof G.__trueosMakeGpuQueue !== 'function') {
        G.__trueosMakeGpuQueue = function __trueosMakeGpuQueue(_device) {
            return {
                __id: G.__trueosWebGpuState.nextId++,
                submit(_cmdBuffers) {
                    const cmdBuffers = Array.isArray(_cmdBuffers) ? _cmdBuffers : [];
                    if (cmdBuffers.length === 0) return;

                    const cmd = G.__trueosCmdStream;
                    if (!cmd || typeof cmd.beginFrame !== 'function' || typeof cmd.endFrame !== 'function') {
                        return;
                    }

                    const rs = G.__trueosRendererState;
                    const summary = __trueosScanSubmit(cmdBuffers, rs.cache);
                    if (!summary.hasWork) return;

                    const executedDraws = __trueosExecuteSubmit(cmd, cmdBuffers, summary);
                    if (executedDraws <= 0) return;

                    rs.frameSeq = (rs.frameSeq + 1) >>> 0;
                    if (TRACE_SUBMIT && (rs.frameSeq % 120) === 1) {
                        console.log(
                            `[submit] frame=${rs.frameSeq} pass=${summary.passCount} ops=${summary.opCount} draws=${summary.drawCount} exec=${executedDraws} vp=${summary.viewportW}x${summary.viewportH}`
                        );
                    }
                    return;
                },
                writeBuffer(buffer, bufferOffset, data, dataOffset = 0, size) {
                    if (!buffer || !(buffer.__data instanceof Uint8Array) || !data) return;
                    const src = (data instanceof Uint8Array)
                        ? data
                        : (ArrayBuffer.isView(data) ? new Uint8Array(data.buffer, data.byteOffset, data.byteLength) : new Uint8Array(data));
                    const bo = Math.max(0, Number(bufferOffset) | 0);
                    const so = Math.max(0, Number(dataOffset) | 0);
                    const n = (size == null) ? (src.byteLength - so) : Math.max(0, Number(size) | 0);
                    const end = Math.min(src.byteLength, so + n);
                    const count = Math.max(0, end - so);
                    if (count <= 0 || bo >= buffer.__data.byteLength) return;
                    const dstEnd = Math.min(buffer.__data.byteLength, bo + count);
                    buffer.__data.set(src.subarray(so, so + (dstEnd - bo)), bo);
                },
                writeTexture(dstDesc, data, layout = {}, size = {}) {
                    const tex = dstDesc?.texture || dstDesc;
                    if (!tex) return;
                    const srcAll = G.__trueosToU8(data);
                    if (!srcAll) return;
                    const width = Number(size.width || size[0] || tex.width || 1) | 0;
                    const height = Number(size.height || size[1] || tex.height || 1) | 0;
                    const bytesPerRow = Number(layout.bytesPerRow || (Math.max(1, width) * 4)) | 0;
                    const srcOffset = Math.max(0, Number(layout.offset || 0) | 0);
                    const rowsPerImage = Math.max(0, Number(layout.rowsPerImage || 0) | 0);
                    let ox = 0;
                    let oy = 0;
                    const origin = dstDesc?.origin;
                    if (Array.isArray(origin)) {
                        ox = Number(origin[0] || 0) | 0;
                        oy = Number(origin[1] || 0) | 0;
                    } else if (origin && typeof origin === 'object') {
                        ox = Number(origin.x || 0) | 0;
                        oy = Number(origin.y || 0) | 0;
                    }
                    const rowPitch = Math.max(1, bytesPerRow);
                    const imageRows = Math.max(1, rowsPerImage || height || 1);
                    const needed = srcOffset + ((Math.max(1, imageRows) - 1) * rowPitch) + (Math.max(1, width) * 4);
                    const srcEnd = Math.min(srcAll.byteLength, needed);
                    if (srcEnd <= srcOffset) return;
                    const src = srcAll.subarray(srcOffset, srcEnd);
                    G.__trueosUploadLinearRgbaToTexture(tex, src, bytesPerRow, width, height, ox, oy);
                    __trueosMarkTextureUpload(tex);
                },
                copyExternalImageToTexture(srcDesc, dstDesc, size = {}) {
                    const tex = dstDesc?.texture || dstDesc;
                    if (!tex) return;

                    const width = Number(size.width || size[0] || tex.width || 1) | 0;
                    const height = Number(size.height || size[1] || tex.height || 1) | 0;
                    const source = srcDesc?.source || srcDesc;

                    let srcBytes = null;
                    let srcStride = Math.max(1, width) * 4;

                    if (source && source.__rgbaData instanceof Uint8Array) {
                        srcBytes = source.__rgbaData;
                        srcStride = Number(source.__rgbaStride || srcStride) | 0;
                    } else if (source && source.data) {
                        srcBytes = G.__trueosToU8(source.data);
                        srcStride = Number(source.bytesPerRow || srcStride) | 0;
                    } else if (source) {
                        const srcU8 = G.__trueosToU8(source);
                        if (srcU8) {
                            srcBytes = srcU8;
                            srcStride = Math.max(1, width) * 4;
                        }
                    }

                    if (!srcBytes && source && typeof source.getContext === 'function') {
                        try {
                            const ctx2d = source.getContext('2d');
                            if (ctx2d && typeof ctx2d.getImageData === 'function') {
                                const img = ctx2d.getImageData(0, 0, Math.max(1, width), Math.max(1, height));
                                srcBytes = G.__trueosToU8(img?.data);
                                srcStride = Math.max(1, width) * 4;
                            }
                        } catch (_) {
                            srcBytes = null;
                        }
                    }

                    if (!srcBytes) return;

                    let ox = 0;
                    let oy = 0;
                    const origin = dstDesc?.origin;
                    if (Array.isArray(origin)) {
                        ox = Number(origin[0] || 0) | 0;
                        oy = Number(origin[1] || 0) | 0;
                    } else if (origin && typeof origin === 'object') {
                        ox = Number(origin.x || 0) | 0;
                        oy = Number(origin.y || 0) | 0;
                    }

                    G.__trueosUploadLinearRgbaToTexture(tex, srcBytes, srcStride, width, height, ox, oy);
                    __trueosMarkTextureUpload(tex);
                    return;
                },
                async onSubmittedWorkDone() {
                    return;
                },
            };
        };
    }
})();
"#;
