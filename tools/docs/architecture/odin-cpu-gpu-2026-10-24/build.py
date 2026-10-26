#!/usr/bin/env python3
"""Build self-contained, source-grounded architecture diagrams; stdlib only."""
from pathlib import Path
from html import escape as esc
import textwrap,json
OUT=Path(__file__).resolve().parent
C={'cpu':'#2363ad','compiler':'#7551ae','gpu':'#16764d','memory':'#007e88','proposal':'#b47710','error':'#bb3e40','gray':'#65758b'}
F={'cpu':'#eef5ff','compiler':'#f4effb','gpu':'#edf8f0','memory':'#ebf8f8','proposal':'#fff6e3','error':'#fff0ef','gray':'#f2f5f8'}
PAGES=[]
class Page:
 def __init__(self,title,sub):
  self.title=title;self.s=['<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1600 1040" role="img" aria-label="'+esc(title)+'">','<defs>']
  for k,v in C.items():self.s.append(f'<marker id="{k}" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="8" markerHeight="8" orient="auto-start-reverse"><path d="M 0 0 L 10 5 L 0 10 z" fill="{v}"/></marker>')
  self.s+=['</defs>','<rect width="1600" height="1040" fill="#fff"/>']
  self.text(40,39,'TRUEOS × ODIN-SON  /  EXECUTION ATLAS',14,'#65758b',700)
  self.text(40,83,title,33,'#16283f',700);self.text(40,117,sub,17)
 def text(self,x,y,t,size=18,color='#30445b',weight=400):self.s.append(f'<text x="{x}" y="{y}" font-family="Arial,sans-serif" font-size="{size}" fill="{color}" font-weight="{weight}">{esc(t)}</text>')
 def lines(self,x,y,ls,size=18,gap=27,color='#30445b'):
  for i,l in enumerate(ls):self.text(x,y+i*gap,l,size,color)
 def card(self,x,y,w,h,title,body,k='cpu',proposed=False):
  dash=' stroke-dasharray="9 5"' if proposed else ''
  self.s.append(f'<rect x="{x}" y="{y}" width="{w}" height="{h}" rx="12" fill="{F[k]}" stroke="{C[k]}" stroke-width="2"{dash}/>')
  self.text(x+18,y+31,title,20,C[k],700)
  ls=[]
  for l in body:ls.extend(textwrap.wrap(l,max(15,int((w-36)/9))) or [''])
  self.lines(x+18,y+61,ls,17,25)
 def edge(self,pts,k='gray',label='',lx=None,ly=None,dashed=False):
  st=' stroke-dasharray="8 6"' if dashed else ''
  self.s.append(f'<polyline points="'+ ' '.join(f'{x},{y}' for x,y in pts)+f'" fill="none" stroke="{C[k]}" stroke-width="2.5" marker-end="url(#{k})"{st}/>')
  if label:self.text(lx if lx is not None else pts[0][0]+8,ly if ly is not None else pts[0][1]-10,label,15,C[k],600)
 def rule(self,y):self.s.append(f'<path d="M40 {y} H1560" stroke="#dce4ec"/>')
 def end(self,sources):
  self.rule(951)
  for x,k,t in [(42,'cpu','CPU / host'),(220,'compiler','Compiler'),(391,'gpu','GPU / driver'),(590,'memory','Memory / data'),(800,'proposal','Dashed: proposed or gap'),(1135,'error','Failure / quarantine')]:
   self.s.append(f'<rect x="{x}" y="970" width="15" height="15" rx="3" fill="{C[k]}"/>');self.text(x+23,983,t,14)
  self.text(40,1018,f'{len(PAGES)+1:02d} / 06    •    {sources}',13)
  self.text(1130,1018,'Code review: 24 Oct 2026 • no new rig run',13)
  self.s.append('</svg>');PAGES.append((self.title,''.join(self.s)))

p=Page('One application; two execution engines','Current structure, with separate compiler, CPU carrier, GPU submission and completion responsibilities.')
p.card(40,155,400,125,'Odin application / compiler Blueprint',['Source, application state, input, frame orchestration. Compiler itself runs on CPU.'],'compiler')
p.card(560,155,445,125,'Odin CPU target',['Odin AST → ordinary typecheck / BAC → x64 object → Blueprint package.'],'compiler')
p.card(1120,155,440,125,'Odin Xe-LP target',['Tagged bounded math → typed GPU ops → BAC / allocation → SIMD8 EU bytes.'],'compiler')
p.edge([(440,215),(560,215)],'compiler');p.edge([(440,185),(500,185),(500,140),(1340,140),(1340,155)],'compiler','GPU target choice',830,135)
p.card(40,337,400,160,'Guest execution state',['VM identity + run generation; guest registers; EPT; lifecycle. Conceptual vCPU state is distributed. [S1–S4]'])
p.card(560,337,445,160,'Executor carrier / CPU lane',['AP2+ lease → vm_task → VMCS → VMLAUNCH / VMRESUME. Guest code executes CPU instructions.'])
p.edge([(240,280),(240,337)],'cpu');p.edge([(440,410),(560,410)],'cpu','placement / lease',448,392)
p.edge([(785,280),(785,337)],'cpu')
p.card(1120,337,440,160,'Generated pixel bridge (current)',['UI4 upload + render ABI; validate immutable body; hold owned destination surface. [S7–S9]'],'gpu')
p.edge([(1340,280),(1340,337)],'compiler','body upload',1353,310)
p.edge([(1005,420),(1120,420)],'gray','guest ABI',1018,399)
p.card(560,568,445,160,'TRUEOS GPU driver + GuC',['System RCS context; row-batch encoding; descriptors, GPUVM mapping, submit, completion polling.'],'gpu')
p.card(1120,568,440,160,'Intel GPU hardware',['Command stream → dispatch → EU SIMD8 math → bounded store → fence → EOT.'],'gpu')
p.edge([(1340,497),(1340,533),(785,533),(785,568)],'gray','driver request',814,524)
p.edge([(1005,646),(1120,646)],'gray','submit',1025,628)
p.edge([(1340,728),(1340,768),(785,768),(785,728)],'gpu','ordered marker + saved-context retirement',850,790)
p.card(40,568,400,160,'Mediated vGPU broker',['Principal, devices, buffers, queues, timelines. A distinct reusable boundary; pixel bridge uses its existing UI4 lease. [S5, S6]'],'memory')
p.card(40,821,1520,98,'Coordination can be shared; execution contracts stay explicit',['CPU carriers run tasks / guest contexts. GPU EUs execute admitted kernels. A common work graph can connect them; today there is no single demonstrated CPU/GPU submission API.'], 'proposal',True)
p.edge([(785,728),(785,751),(503,751),(503,458),(440,458)],'gpu','release / return',460,779)
p.end('S1–S9, S12 • Lines: gray control; purple compilation; green completion; teal data (later sheets).')

p=Page('Where the vCPU abstraction lives today','A guest execution context, an executor lane, a CPU SIMD element and a GPU SIMD element have different identities.')
p.card(40,160,465,190,'Logical guest / vCPU-like state',['VM ID + run generation; guest GPRs and extended state; guest RIP/control state in VMCS; EPT-backed address space and lifecycle. [S2–S4]'])
p.card(570,160,470,190,'Carrier lease + placement',['LaneRole::VmHull, slot, core kind, WorkerSpawner, LaneLease. Reserved AP2+ placement. A lane also supports other roles. [S1, S2]'])
p.card(1105,160,455,190,'Physical execution substrate',['Per-slot VMXON / VMCS backing; lane-local VPID assignment; current CPU executor polls vm_task. [S3, S4, S12]'])
p.edge([(505,255),(570,255)],'gray');p.edge([(1040,255),(1105,255)],'gray')
p.card(40,400,465,155,'Run / suspend / resume',['Guest instructions → VM exit → host handler. Cooperative yield / wait boundaries return to executor; eligible resume re-enters VM.'])
p.card(570,400,470,155,'Affinity invariant',['Current run validates its lane tuple before entry. Async migration cannot silently use another lane’s VMCS. [S3, S4]'])
p.card(1105,400,455,155,'Retire / retain / quarantine',['Pause can retain carrier + VMCS. Teardown invalidates VPID before lease reuse; failed invalidation quarantines lane.'])
p.edge([(270,350),(270,400)],'cpu');p.edge([(805,350),(805,400)],'cpu');p.edge([(1330,350),(1330,400)],'cpu')
p.rule(600);p.text(40,637,'Terms to keep separate in design discussions',23,'#16283f',700)
rows=[('VM / vCPU-like context','Owns execution state and lifecycle; currently spread across VM, VMX and lane modules.'),('Executor carrier lane','Host scheduling resource / CPU slot lease. Its role can be Hull, worker or blocking work.'),('CPU SIMD lane','One element in an AVX/SIMD operation; not a VM identity or scheduling lease.'),('GPU SIMD8 lane','One data element of an EU instruction. The pixel body calculates eight pixels in parallel.'),('vGPU queue / GPU context','Virtual admission / completion identity versus physical firmware scheduling context.')]
for i,(a,b) in enumerate(rows):
 y=677+i*45;p.text(50,y,a,18,C['cpu'],700);p.text(345,y,b,17)
p.text(40,929,'Interpretation: name the logical CPU execution contract explicitly; a new guest-visible vCPU API remains a design choice.',17,C['proposal'])
p.end('S1–S4, S12 • Conceptual vCPU label describes responsibility, not an existing public Vcpu type.')

p=Page('Three GPU compiler paths, three different contracts','The handwritten encoder remains valuable. Backend reuse and runtime integration are separate questions.')
p.card(40,160,465,125,'A · Native pixel math body',['Ordinary f32 / bool math; bounded source loops unrolled by frontend.'],'compiler')
p.card(570,160,470,125,'B · Native scalar-call kernel',['Two f32 arguments → one f32 result. CPU stub embeds .gpu.xelp code.'],'compiler')
p.card(1105,160,455,125,'C · Legacy boids command DSL',['gpu_add_scaled produces four-word commands; optional fixed-register EU body export.'],'compiler')
p.card(40,343,465,155,'Shared BAC machinery',['Typed GPU ops → BAC/SSA → target lowering → register allocation → SIMD8 arithmetic body; r1/r2 input, r4 output.'],'compiler')
p.card(570,343,470,155,'Shared BAC + kernel tail',['Same math compiler, plus result-store / fence / EOT convention. Runtime call is generated into CPU code.'],'compiler')
p.card(1105,343,455,155,'Separate legacy lowering',['Fixed r16..r20 boids arithmetic export; legacy body has no shared scalar-path allocation. Four-word DSL ≠ native EU encoding.'],'compiler')
for x in [272,805,1332]:p.edge([(x,285),(x,343)],'compiler')
p.card(40,555,465,185,'Current integration + recorded proof',['UI4 bridge + runtime envelope; eight useful pixel lanes; GPU writes surface. ADL-S matching image hashes recorded on 22 Oct 2026. [S7–S10]'],'gpu')
p.card(570,555,470,185,'Current ABI gap',['Compiler requests trueos_cabi_vgpu_call_f32_v1, marked retired by current loader. Host exposes handle-based ...vgpu_scalar_f32_v1 instead. [S6, S11]'],'proposal',True)
p.card(1105,555,455,185,'Transport still needs tracing',['Optional EU body is a fragment. Legacy CPU object retains its command dispatch route; vFile:compute-boids is not established by this review.'],'proposal',True)
for x in [272,805,1332]:p.edge([(x,498),(x,555)],'gray')
p.card(40,792,1520,134,'What “scalar source” means here',['SIMD8 arithmetic is already emitted. Pixel body inputs vary across all eight elements; scalar-call ABI exposes one active result lane. Wider batching needs invocation IDs, memory access and an explicit ABI. 16-byte uncompacted encoding says how bytes are stored, not how many useful data elements execute.'],'gpu')
p.end('S6–S11, S13 • A hardware result for A does not establish B or C end-to-end.')

p=Page('A frame today: control crosses the CPU boundary per row batch','Sequence diagram • vertical order is causal; spacing is schematic and represents no measured time.')
xs=[160,470,800,1110,1430]
names=['Odin app (CPU)','UI4 bridge (CPU)','RCS driver (CPU)','GuC / GPU','Surface + display']
for x,n in zip(xs,names):
 p.card(x-116,155,232,72,n,[], 'gpu' if x==1110 else 'memory' if x==1430 else 'cpu')
 p.s.append(f'<path d="M{x} 227 V835" stroke="#cbd6e3" stroke-dasharray="4 6"/>')
def arrow(y,a,b,label,k='gray'):p.edge([(xs[a],y),(xs[b],y)],k,label,min(xs[a],xs[b])+12,y-11)
arrow(270,0,1,'1 · upload bounded math body (ordered chunks)')
arrow(325,1,2,'2 · validate + retain immutable program')
arrow(380,0,1,'3 · begin frame; render(origin, step)')
arrow(430,1,2,'4 · hold owned back buffer + reserve shared context')
p.s.append('<rect x="658" y="462" width="565" height="264" rx="10" fill="none" stroke="#16764d" stroke-width="2" stroke-dasharray="7 5"/>')
p.text(680,487,'REPEAT per row batch · current host-driven loop',16,C['gpu'],700)
arrow(530,2,3,'5 · encode descriptors / payload; submit')
p.card(989,551,242,87,'SIMD8 work',['Envelope + math + stores'],'gpu')
arrow(683,3,2,'6 · ordered cookie + retirement; CPU poll', 'gpu')
arrow(765,2,1,'7 · all batches retired → surface release','gpu')
arrow(813,1,0,'8 · render returns status','gpu')
arrow(863,0,4,'9 · publish released frame to compositor','memory')
p.text(40,917,'A single application render call currently expands into multiple submits and waits. No CPU callback occurs between math instructions.',18)
p.end('S7–S9 • Generic GpuFence Future exists (S5); this direct pixel path performs synchronous polling.')

p=Page('Memory ownership and the four meanings of synchronization','An integrated GPU can access system RAM; address translation, visibility and ownership still need explicit contracts.')
p.card(40,160,465,150,'CPU address / app-owned allocation',['CPU virtual address → physical backing pages. An Odin pointer is not a valid GPU virtual address by itself.'],'memory')
p.card(570,160,470,150,'GPUVM / driver-managed mapping',['GPU virtual address → mapped backing. Generic vVideoMem can pin guest pages; current pixel output uses an owned UI4 surface.'],'memory')
p.card(1105,160,455,150,'GPU control + result storage',['Runtime owns batch, descriptors, context and marker storage. Stage admitted code; retain backing through retirement.'],'memory')
p.edge([(505,235),(570,235)],'memory','resolve / map',505,216);p.edge([(1040,235),(1105,235)],'memory')
p.card(40,372,320,128,'Writable by producer',['Prepare source / frame; release prior CPU writes before submit.'],'memory')
p.card(440,372,320,128,'GPU owns work',['Read/write reservations and frame lease remain held.'],'gpu')
p.card(840,372,320,128,'Retirement proven',['Ordered completion + saved context; release to consumer.'],'gpu')
p.card(1240,372,320,128,'Consumer acquires',['Visibility + ownership permit CPU read or UI4 publish.'],'memory')
for a,b in [(360,440),(760,840),(1160,1240)]:p.edge([(a,435),(b,435)],'gpu')
p.card(440,551,720,92,'Ambiguous completion / timeout',['Retain carrier and surface backing. Dropping a waiter does not cancel hardware.'],'error')
p.edge([(600,500),(600,551)],'error')
p.rule(678)
items=[('1 · Instruction dependency','Register / ALU / SEND readiness inside an EU thread; conservative @1 in generated math.'),('2 · Device memory visibility','Data-cache / memory fence orders writes; EU EOT terminates a thread.'),('3 · Queue and context retirement','Ordered marker plus context proof permits host resource reuse and completion accounting.'),('4 · Application dependency','A consumer waits for its exact producer; a frame publishes only after its release token exists.')]
for i,(a,b) in enumerate(items):
 y=718+i*55;p.text(48,y,a,19,C['gpu'],700);p.text(420,y,b,17)
p.text(40,933,'Workgroup barriers are another contract; the bounded straight-line math body does not implement general workgroup synchronization.',16,C['gray'])
p.end('S5–S9, S12 • Shared backing reduces transfer needs; it does not merge ownership or address spaces.')

p=Page('A possible common work model over CPU and GPU backends','DESIGN PROPOSAL • A shared dependency / ownership protocol can coexist with distinct execution engines.')
p.card(40,160,450,175,'Common Work descriptor',['Owner + generation; task/kernel identity; typed buffer views; read/write effects; dependencies; completion + error identity.'],'proposal',True)
p.card(575,160,450,175,'Admission / ready graph',['Validate target capabilities, dependencies and reservations. Choose an explicit target; publish ready work in batches.'],'proposal',True)
p.card(1110,160,450,175,'Exact completion record',['Success / failure belongs to this work item. Wake consumers; release ownership only after backend retirement.'],'proposal',True)
p.edge([(490,245),(575,245)],'proposal',dashed=True)
p.card(40,414,450,180,'CPU execution adapter',['Guest context or host task → carrier lease → executor / VMX. Preserve guest identity, memory isolation, yield and lane-affinity rules.'],'cpu')
p.card(575,414,450,180,'GPU execution adapter',['Kernel ABI + grid → GPUVM views → admitted command batches → GuC / EUs. Preserve visibility, exact retirement and error quarantine.'],'gpu')
p.card(1110,414,450,180,'Independent CPU work / consumers',['Ready work runs while GPU is outstanding. Dependent CPU reads / display publication wait on the matching completion.'],'cpu')
p.edge([(725,335),(725,373),(265,373),(265,414)],'proposal',dashed=True)
p.edge([(800,335),(800,414)],'proposal',dashed=True)
p.edge([(265,594),(265,636),(1067,636),(1067,245),(1110,245)],'proposal','common completion protocol',630,661,True)
p.edge([(800,594),(800,636)],'proposal',dashed=True)
p.edge([(1335,335),(1335,414)],'proposal','wake dependent work',1350,381,True)
p.rule(700);p.text(40,741,'What to optimize and measure',24,'#16283f',700)
p.lines(40,780,[
 'Cold work = compile + admit / prepare + map / visibility + submit + GPU execution + retire / acquire.',
 'Warm work can reuse admitted code. Batch independent elements to amortize submission; split only for real dependencies / limits.',
 'Compare against scalar and vectorized CPU execution on identical data, including end-to-end latency and useful CPU overlap.',
 'SIMD width, independent hardware threads, workgroup count and instruction-level parallelism are separate tuning dimensions.',
 'A device-resident worklist needs its own validated protocol and memory operations; the current math-body subset does not provide it.'
],18,31)
p.end('Proposal based on S1–S13 • No new runtime, scheduler, offload policy or hardware capability is claimed implemented.')

SOURCES=[
('S1','CPU carrier roles and leases','src/hv/lane.rs',24,'LaneRole, LaneLease, pick_carrier_lane; role and placement are distinct from guest state.'),
('S2','Hull placement contract','src/hv/guest_work.rs',6,'Executor-backed Hull carriers; AP2+ default; VMX backing is indexed per CPU slot.'),
('S3','VM task and VM entry loop','src/hv/mod.rs',4090,'vm_task carries lane lease; VM entry loop validates assignment and re-enters the guest.'),
('S4','Guest register state and VPID contract','src/hv/vmx.rs',239,'GuestRegisters plus extended state. See also tools/docs/vpid_runtime_validation.md for invalidation / affinity.'),
('S5','GPU async admission / completion executor','src/gpu/executor.rs',1,'KernelSubmission, GpuFence, waiter wakeup, exact retirement. This is not itself a GPU instruction interpreter.'),
('S6','vGPU scalar broker and C ABI','src/r/io/vgpu_cabi.rs',30,'Handle-based trueos_cabi_vgpu_scalar_f32_v1; resolves to broker and native scalar driver.'),
('S7','UI4 generated compute boundary','src/ui4/blueprint_ui4/native_compute_api.rs',28,'Chunk upload, owner/frame checks, feature gating; synchronous render_wire call and release handling.'),
('S8','Generated pixel dispatch + math envelope','src/intel/gpgpu/operations/generated_pixel.rs',148,'Host row-batch submit/poll loop. Envelope at line 400 owns coordinates, packing, store, fence / EOT.'),
('S9','Direct RCS submission and retirement','src/intel/gpgpu/rcs/commands.rs',892,'Poll helpers and saved-context retirement. Also rcs/runtime.rs:316 documents marker + saved-head proof.'),
('S10','Recorded physical Mandelbrot evidence','docs/experiments/odin-runtime-mandelbrot.md',117,'22 Oct 2026 ADL-S checksums for constant, 25- and 16-iteration images; report and JSON, not a new run.'),
('S11','Retired compiler import','src/hv/blueprint/prebind.rs',85,'Old pointer-based trueos_cabi_vgpu_call_f32_v1 is explicitly rejected as retired.'),
('S12','CPU executor polling','src/runtime.rs',23,'Local CPU executor polling; run_ap_forever; supporting VPID contract in tools/docs/vpid_runtime_validation.md.'),
('S13','Odin native GPU lowering and encoder contracts','../odin-son/odinc/gpu_native.odin',10,'Compiler body / scalar stub. See also typecheck/gpu_native.odin:212, bac/intel_xelp/README.md and encode.odin.'),
]
for i,(title,svg) in enumerate(PAGES,1):(OUT/f'{i:02d}.svg').write_text(svg)
nav=''.join(f'<button data-page="{i}" onclick="showPage({i})">{i+1:02d} · {esc(t)}</button>' for i,(t,_) in enumerate(PAGES))
sheets=''.join(f'<section class="sheet" data-page="{i}">{svg}</section>' for i,(_,svg) in enumerate(PAGES))
refs=''.join(f'<tr><td>{sid}</td><td><b>{esc(title)}</b><br><a href="../../../{path}">{esc(path)}:{line}</a></td><td>{esc(note)}</td></tr>' for sid,title,path,line,note in SOURCES)
css='''*{box-sizing:border-box}body{margin:0;background:#e8edf3;color:#20354e;font:16px Arial,sans-serif}header{padding:22px 30px;background:#14263c;color:white}header h1{font-size:26px;margin:0 0 9px}header p{margin:0;max-width:1250px;line-height:1.5;color:#d6e1ed}.toolbar{position:sticky;top:0;z-index:3;background:white;padding:12px 25px;box-shadow:0 2px 8px #0002;display:flex;gap:12px;align-items:center;flex-wrap:wrap}button,select{font:inherit;border:1px solid #b8c8d8;background:#fff;color:#20354e;border-radius:6px;padding:8px 12px;cursor:pointer}button.active{background:#203d61;color:white}nav{padding:18px 25px;display:flex;gap:7px;flex-wrap:wrap}nav button{font-size:14px}.viewport{overflow:auto;padding:16px 24px 32px}.sheet{display:none;width:1600px;background:white;box-shadow:0 5px 22px #26394e22}.sheet.active{display:block}svg{display:block;width:100%;height:auto}article{background:#fff;max-width:1550px;margin:0 25px 35px;padding:28px 34px;line-height:1.6;border-radius:8px}article h2{margin-top:0}table{border-collapse:collapse;width:100%;font-size:14px}td,th{padding:12px;text-align:left;border-bottom:1px solid #dce4ec;vertical-align:top}td:first-child{white-space:nowrap}a{color:#225c9f}details{margin:18px 0}summary{cursor:pointer;font-weight:bold}.note{border-left:4px solid #b47710;padding:8px 18px;background:#fff6e3}@page{size:A3 landscape;margin:8mm}@media print{body{background:white}header,.toolbar,nav,.screen-only{display:none}.viewport{padding:0;overflow:visible}.sheet,.sheet.active{display:block!important;width:100%!important;box-shadow:none;break-after:page}svg{width:100%;height:272mm}article{margin:0;padding:8px;break-before:page;border-radius:0}details{display:block}table{font-size:12px}td,th{padding:8px}}'''
js='''let selected=0;function showPage(i){selected=i;document.querySelectorAll('.sheet').forEach((e,j)=>e.classList.toggle('active',j===i));document.querySelectorAll('nav button').forEach((e,j)=>e.classList.toggle('active',j===i));document.getElementById('pageSelect').value=i;fit()}function fit(){let w=Math.min(1600,window.innerWidth-48);document.querySelectorAll('.sheet').forEach(e=>e.style.width=w+'px');document.getElementById('zoom').value=Math.round(w/16);document.getElementById('zoomValue').textContent=Math.round(w/16)+'%'}function zoom(v){document.querySelectorAll('.sheet').forEach(e=>e.style.width=(v*16)+'px');document.getElementById('zoomValue').textContent=v+'%'}document.addEventListener('keydown',e=>{if(e.key==='ArrowRight')showPage((selected+1)%6);if(e.key==='ArrowLeft')showPage((selected+5)%6)});showPage(0);'''
html=f'''<!doctype html><html lang="en"><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>TRUEOS × Odin-son — CPU / GPU execution atlas</title><style>{css}</style><header><h1>CPU / GPU execution atlas</h1><p>Six source-grounded diagrams: the CPU execution context, executor carriers, Odin compiler paths, GPU submission, memory ownership, and a proposed common work model. Snapshot: TRUEOS c95ddd528; odin-son 2b07b8a; 24 October 2026.</p></header><div class="toolbar"><select id="pageSelect" aria-label="Diagram" onchange="showPage(+this.value)">{''.join(f'<option value="{i}">{i+1}. {esc(t)}</option>' for i,(t,_) in enumerate(PAGES))}</select><label>Zoom <input id="zoom" aria-label="Zoom" type="range" min="45" max="180" oninput="zoom(this.value)"></label><span id="zoomValue"></span><button onclick="fit()">Fit width</button><button onclick="window.print()">Print all / save PDF</button><a href="execution-atlas.pdf">PDF</a><a href="01.svg" download>Overview SVG</a></div><nav aria-label="Diagrams">{nav}</nav><main><div class="viewport">{sheets}</div><article><h2>How to read the architecture</h2><p><b>One coordination model, two execution engines</b> is a useful proposed architecture. Today, CPU guest execution uses executor-backed VMX carriers, and GPU work uses mediated capabilities plus driver / firmware execution. The GPU admission executor is host-side code; it does not run the EU math. Odin's compiler runs on the CPU, even when generating GPU code.</p><p>The CPU side already contains vCPU-like state. Its representation is distributed across guest registers, VMCS/EPT, VM identity, run generation and lane assignments. A carrier lane is the host resource that runs this state; a stable logical vCPU identity could be documented or factored above it. A full public multi-vCPU model, migration policy or unified scheduler is not established by this review.</p><p>There are also <b>two GPU ABI layers</b> to keep distinct: a complete scalar-call kernel and a pixel arithmetic body with a runtime-owned envelope. Their active-lane and result conventions differ. The legacy boids DSL adds a third historical route. These are not interchangeable meanings of “two runtimes.”</p><p class="note"><b>Current integration gap:</b> odin-son still emits <code>trueos_cabi_vgpu_call_f32_v1</code> with a pointer-based signature. Current TRUEOS rejects that import as retired and exposes the handle-based <code>trueos_cabi_vgpu_scalar_f32_v1</code>. This static finding does not invalidate the separate recorded pixel-body execution. No runtime implementation was changed for these diagrams.</p><h2>What the recorded GPU run establishes</h2><p>The checked-in 22 October report records ADL-S execution of runtime-compiled Mandelbrot bodies with matching independent image hashes. The 25-iteration body is 14,496 bytes and the 16-iteration body is 9,168 bytes. It establishes this bounded generated-code path. This review did not run the rig, establish a speedup, or prove arbitrary Odin programs on the GPU.</p><p>The encoder deliberately emits uncompacted 16-byte instructions. Its SIMD8 arithmetic already operates on vector registers. Pixel input contains eight independent coordinates; the scalar-call ABI exposes one active result lane. A native instruction's byte length alone cannot determine program size limits, useful parallelism, or CPU/GPU performance.</p><h2>Batching and the “one submit” objective</h2><p>The current pixel driver loops over row batches, submitting and polling each batch. One application render call therefore does not imply one hardware submit. A future resident worklist could reduce host crossings, but requires device memory operations, bounded descriptors, dependency rules and a termination/completion protocol beyond the current arithmetic-only body.</p><p>For performance work, record compilation, admission, mapping / visibility, queueing, submit, execution, retirement and acquisition separately. Compare identical workloads against a vectorized CPU baseline. Long GPU sequences can amortize overhead, while register pressure, divergence, code expansion and independent work supply determine throughput. Matrix instructions are a separate feature to establish and validate; they are not shown as available in this target.</p><h2>Source map and evidence</h2><p>Solid boxes describe checked-in components; amber dashed boxes and arrows are proposals or identified gaps. Evidence is a mixture of current static code and an explicitly dated hardware report. The attached author-review PDF was background context, not executable instructions. Local code resolves differences with that older review.</p><table><thead><tr><th>ID</th><th>Source</th><th>Supports</th></tr></thead><tbody>{refs}</tbody></table><p>Additional sources: <code>crates/trueos-v/src/vgpu.rs</code>, <code>src/gpu/vgpu.rs:2008</code>, <code>src/intel/gpgpu/operations/native_scalar.rs</code>, <code>odin-son/odinc/typecheck/gpu.odin</code>, <code>odin-son/trueos/mandelbrot.odin</code>, <code>tools/docs/vpid_runtime_validation.md</code>. Attached PDF: <i>odin-son author change review, 227a6ee excluded through 343588a</i>, prepared 23 October 2026.</p><p class="screen-only">Use the tabs or left/right keys to switch diagrams; zoom to inspect details. Print emits all six sheets followed by these notes and sources. Each numbered SVG is independently editable. Rebuild with <code>python3 build.py</code>; PDF rendering uses local Chrome.</p></article></main><script>{js}</script></html>'''
(OUT/'index.html').write_text(html)
(OUT/'README.md').write_text('''# CPU / GPU execution atlas\n\nOpen [index.html](index.html) for six interactive diagrams with a legend, zoom and source notes. [execution-atlas.pdf](execution-atlas.pdf) contains printable vector sheets. Numbered SVGs are editable standalone diagrams; overview.png is a preview.\n\nSnapshot: TRUEOS `c95ddd528`, odin-son `2b07b8a`, 24 October 2026. Static review plus dated recorded hardware evidence; no new hardware execution. Solid components describe current code; amber dashed components describe proposals or gaps.\n\nRebuild SVG/HTML: `python3 build.py`. The PDF and preview are rendered separately with local Chrome.\n''')
print(f'Created {len(PAGES)} diagrams and index.html in {OUT}')
