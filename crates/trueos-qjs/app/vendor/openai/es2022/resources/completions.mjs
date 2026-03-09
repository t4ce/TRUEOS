/* esm.sh - openai@6.27.0/resources/completions */
import{APIResource as s}from"../core/resource.mjs";var t=class extends s{create(e,r){return this._client.post("/completions",{body:e,...r,stream:e.stream??!1})}};export{t as Completions};
//# sourceMappingURL=completions.mjs.map