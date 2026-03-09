/* esm.sh - openai@6.27.0/internal/utils/uuid */
var a=function(){let{crypto:n}=globalThis;if(n?.randomUUID)return a=n.randomUUID.bind(n),n.randomUUID();let t=new Uint8Array(1),o=n?()=>n.getRandomValues(t)[0]:()=>Math.random()*255&255;return"10000000-1000-4000-8000-100000000000".replace(/[018]/g,r=>(+r^o()&15>>+r/4).toString(16))};export{a as uuid4};
//# sourceMappingURL=uuid.mjs.map