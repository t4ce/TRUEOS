/* esm.sh - openai@6.27.0/internal/utils/env */
import __Process$ from "/node/process.mjs";
var n=e=>{if(typeof(0,__Process$)<"u")return __Process$.env?.[e]?.trim()??void 0;if(typeof globalThis.Deno<"u")return globalThis.Deno.env?.get?.(e)?.trim()};export{n as readEnv};
//# sourceMappingURL=env.mjs.map