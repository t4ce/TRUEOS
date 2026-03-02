#![cfg(feature = "trueos")]

pub const CANVAS_RENDERER_SHIM_JS: &[u8] = br#"
(function () {
    const G = (typeof globalThis !== 'undefined') ? globalThis : this;

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
                    // Bridge point: this queue currently validates flow and preserves API shape.
                    // Draw translation into cmd_stream is intentionally staged in follow-up work.
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
                    const src = G.__trueosToU8(data);
                    if (!src) return;
                    const width = Number(size.width || size[0] || tex.width || 1) | 0;
                    const height = Number(size.height || size[1] || tex.height || 1) | 0;
                    const bytesPerRow = Number(layout.bytesPerRow || (Math.max(1, width) * 4)) | 0;
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
                    G.__trueosUploadLinearRgbaToTexture(tex, src, bytesPerRow, width, height, ox, oy);
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
