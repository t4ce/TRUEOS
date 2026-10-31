#!/usr/bin/env python3
"""Exercise production retirement proof with offline temporary captures only."""
import importlib.util
import hashlib
import json
from pathlib import Path
import tempfile
import threading
import time
import unittest
import subprocess
import sys

ROOT=Path(__file__).resolve().parents[2]
PATH=ROOT/'tools/codex_retirement_gate.py'
spec=importlib.util.spec_from_file_location('gate',PATH)
gate=importlib.util.module_from_spec(spec);spec.loader.exec_module(gate)
SHA='3'*64
IDENTITY={'request':1,'app_gen':1,'instance':'11111111-1111-4111-8111-111111111111','archive_sha256':SHA}
PROOF=('trueos-startup-v1 request=1 vm=0 gen=3 phase=guest_entry event=begin status=pending\n'
       'trueos-startup-v1 request=1 vm=0 gen=3 phase=guest_entry event=end status=ok\n'
       '[apps] [info] vm0 codex_startup: [codex-startup] 1000\t0.000\t0.000\tprocess_context request=1 app_gen=1 instance=11111111-1111-4111-8111-111111111111 archive_sha256='+SHA+' request=1\n'
       'thread-placement-config vm=0 gen=3 mode=1 background_cap=2 background=[4, 10] foreground=19 migration=none\n').encode()
FS=b'vm-recovery: filesystem-drained vm=0 generation=3 fs_pending=0 fs_streams=0 async_fs_pending=0\n'
OFF=b'hv: vm0 lifecycle: offline native_jobs=0 carrier=released wait_queues_retired=44 generation=3\n'

class Tests(unittest.TestCase):
 def setup_case(self, proof=PROOF, tail=b''):
  d=tempfile.TemporaryDirectory(prefix='trueos-retire-fixture-');self.addCleanup(d.cleanup)
  cap=Path(d.name)/'capture';cap.write_bytes(proof+tail);st=cap.stat()
  summary={'schema':2,'applications':[{'context':IDENTITY}], 'window':{
   'start_offset':0,'end_offset':len(proof),'bytes':len(proof),'sha256':hashlib.sha256(proof).hexdigest(),
   'file_device':st.st_dev,'file_inode':st.st_ino}}
  path=Path(d.name)/'summary.json';path.write_text(json.dumps(summary))
  return cap,path,summary
 def run_case(self,tail=b'',proof=PROOF):
  cap,path,_=self.setup_case(proof,tail)
  return gate.observe(path,SHA,cap,timeout=0)
 def test_exact_ordered_offline(self):
  r=self.run_case(FS+OFF);self.assertTrue(r['success']);self.assertEqual(r['launch_proof']['generation'],3)
 def test_previous_generation_ignored(self):
  r=self.run_case((FS+OFF).replace(b'generation=3',b'generation=2'));self.assertFalse(r['success']);self.assertIsNone(r['offline'])
 def test_wrong_vm_ignored(self):
  r=self.run_case((FS+OFF).replace(b'vm=0',b'vm=1').replace(b'vm0',b'vm1'));self.assertFalse(r['success'])
 def test_stop_accepted_stopped_not_enough(self):
  self.assertFalse(self.run_case(b'hv: vm0 lifecycle: stopped\nterminal-lifecycle: vm=0 phase=process-stop state=stopped\n')['success'])
 def test_fs_missing_refused(self):
  self.assertEqual(self.run_case(OFF)['reason'],'offline_missing_prior_filesystem_drain')
 def test_nonzero_jobs_refused(self):
  self.assertEqual(self.run_case(FS+OFF.replace(b'native_jobs=0',b'native_jobs=1'))['reason'],'offline_owner_not_drained_or_carrier_retained')
 def test_carrier_retained_refused(self):
  self.assertFalse(self.run_case(FS+OFF.replace(b'carrier=released',b'carrier=retained'))['success'])
 def test_nonzero_fs_refused(self):
  self.assertEqual(self.run_case(FS.replace(b'fs_pending=0',b'fs_pending=1')+OFF)['reason'],'filesystem_drain_not_zero')
 def test_new_boot_rejected_even_after_offline(self):
  self.assertEqual(self.run_case(FS+OFF+b'Entering TrueOS kernel\n')['reason'],'newer_boot_after_target_launch')
 def test_new_context_rejected_even_after_offline(self):
  ctx=PROOF.splitlines(keepends=True)[2].replace(b'app_gen=1',b'app_gen=2')
  self.assertEqual(self.run_case(FS+OFF+ctx)['reason'],'newer_or_duplicate_application_context')
 def test_new_generation_rejected(self):
  self.assertEqual(self.run_case(FS+OFF+b'thread-placement-config vm=0 gen=4\n')['reason'],'target_vm_reused_by_another_generation')
 def test_reassigned_after_offline_rejected(self):
  self.assertEqual(self.run_case(FS+OFF+b'hv: vm0 lifecycle: generation=4 assigned slot=10\n')['reason'],'target_vm_reassigned_to_another_generation')
 def test_other_vm_assignment_allowed(self):
  self.assertTrue(self.run_case(FS+OFF+b'hv: vm1 lifecycle: generation=4 assigned slot=10\n')['success'])
 def test_resource_retained_refused(self):
  self.assertEqual(self.run_case(FS+b'hv: vm0 lifecycle: resource release retained reason=problem\n'+OFF)['reason'],'target_resource_release_retained')
 def test_other_service_ignored(self):
  other=b'thread-placement-config vm=1 gen=2\ntrueos-startup-v1 request=2 vm=1 gen=2 phase=guest_entry event=end status=ok\n'
  self.assertTrue(self.run_case(FS+OFF,proof=PROOF+other)['success'])
 def test_partial_line_not_success(self):
  self.assertEqual(self.run_case(FS+OFF+b'new partial')['reason'],'timeout_with_partial_collector_line')
 def test_hash_mismatch(self):
  cap,path,s=self.setup_case(tail=FS+OFF);s['window']['sha256']='0'*64;path.write_text(json.dumps(s))
  self.assertEqual(gate.observe(path,SHA,cap,0)['reason'],'proof_source_hash_mismatch')
 def test_replaced_file(self):
  cap,path,s=self.setup_case(tail=FS+OFF);old=cap.with_suffix('.old');cap.rename(old);cap.write_bytes(old.read_bytes())
  self.assertEqual(gate.observe(path,SHA,cap,0)['reason'],'collector_identity_changed_or_truncated')
 def test_truncated_file(self):
  cap,path,s=self.setup_case();cap.write_bytes(b'')
  self.assertEqual(gate.observe(path,SHA,cap,0)['reason'],'collector_identity_changed_or_truncated')
 def test_bad_archive(self):
  cap,path,s=self.setup_case(tail=FS+OFF)
  self.assertEqual(gate.observe(path,'a'*64,cap,0)['reason'],'archive_mismatch')
 def test_line_bound(self):
  self.assertEqual(self.run_case(b'x'*(gate.MAX_LINE+1)+b'\n')['reason'],'collector_line_limit_exceeded')
 def test_pre_runtime_context_allowed(self):
  self.assertTrue(self.run_case(FS+OFF,proof=b'\n'.join(PROOF.splitlines()[:3])+b'\n')['success'])
 def test_follow_fragmented_append(self):
  cap,path,s=self.setup_case()
  def append():
   time.sleep(.02)
   with cap.open('ab') as f:f.write(FS+OFF[:23]);f.flush()
   time.sleep(.02)
   with cap.open('ab') as f:f.write(OFF[23:]);f.flush()
  t=threading.Thread(target=append);t.start()
  r=gate.observe(path,SHA,cap,.4,poll=.005);t.join();self.assertTrue(r['success'])
 def test_source_context_ambiguity(self):
  self.assertEqual(self.run_case(FS+OFF,proof=PROOF+PROOF.splitlines(keepends=True)[2])['reason'],'source_context_ambiguous_or_mismatch')

 def startup_report(self, summary):
  window={key:value for key,value in summary['window'].items() if key not in ('file_device','file_inode')}
  return {'schema':1,'application_runs':[{'context':IDENTITY}],'capture_window':window}
 def test_bounded_startup_report_normalization(self):
  cap,path,s=self.setup_case(tail=FS+OFF);path.write_text(json.dumps(self.startup_report(s)))
  r=gate.observe(path,SHA,cap,0);self.assertTrue(r['success']);self.assertEqual(r['source_kind'],'startup_report_v1')
 def test_unbounded_parse_lines_report_refused(self):
  cap,path,s=self.setup_case(tail=FS+OFF);report=self.startup_report(s);del report['capture_window'];path.write_text(json.dumps(report))
  self.assertEqual(gate.observe(path,SHA,cap,0)['reason'],'requires_bounded_startup_report')
 def test_startup_report_hash_mismatch(self):
  cap,path,s=self.setup_case(tail=FS+OFF);report=self.startup_report(s);report['capture_window']['sha256']='0'*64;path.write_text(json.dumps(report))
  self.assertEqual(gate.observe(path,SHA,cap,0)['reason'],'proof_source_hash_mismatch')
 def test_startup_report_multiple_applications_refused(self):
  cap,path,s=self.setup_case(tail=FS+OFF);report=self.startup_report(s);report['application_runs']*=2;path.write_text(json.dumps(report))
  self.assertEqual(gate.observe(path,SHA,cap,0)['reason'],'requires_one_application_v2_summary')
 def test_proof_byte_count_mismatch(self):
  cap,path,s=self.setup_case(tail=FS+OFF);s['window']['bytes']+=1;path.write_text(json.dumps(s))
  self.assertEqual(gate.observe(path,SHA,cap,0)['reason'],'proof_window_byte_count_mismatch')
 def cli(self,cap,path,output,extra=()):
  return subprocess.run([sys.executable,str(PATH),'--startup-summary',str(path),'--expected-sha256',SHA,'--capture',str(cap),'--output',str(output),'--timeout','0',*extra],cwd=cap.parent,capture_output=True,text=True)
 def test_cli_portable_explicit_paths(self):
  cap,path,s=self.setup_case(tail=FS+OFF);out=cap.parent/'receipt.json';r=self.cli(cap,path,out)
  self.assertEqual(r.returncode,0,r.stderr);self.assertTrue(json.loads(out.read_text())['success']);self.assertEqual(json.loads(r.stdout)['output'],str(out))
 def test_cli_output_never_overwritten(self):
  cap,path,s=self.setup_case(tail=FS+OFF);out=cap.parent/'receipt.json';out.write_bytes(b'existing receipt\n')
  r=self.cli(cap,path,out);self.assertEqual(r.returncode,2);self.assertEqual(out.read_bytes(),b'existing receipt\n')
 def test_cli_capture_and_output_required(self):
  for missing in ('--capture','--output'):
   args=[sys.executable,str(PATH),'--startup-summary','missing.json','--expected-sha256',SHA]
   args+=['--output','receipt'] if missing=='--capture' else ['--capture','capture']
   r=subprocess.run(args,capture_output=True,text=True)
   self.assertEqual(r.returncode,2);self.assertIn(missing,r.stderr)
 def test_cli_invalid_timeout_and_sha_no_output(self):
  cap,path,s=self.setup_case(tail=FS+OFF);out=cap.parent/'receipt.json'
  for extra in (('--timeout','-1'),('--timeout','nan'),('--expected-sha256','bad')):
   r=self.cli(cap,path,out,extra);self.assertEqual(r.returncode,2);self.assertFalse(out.exists())
 def test_actual_startup_report_cli_feeds_gate(self):
  cap,path,s=self.setup_case(tail=FS+OFF)
  report=subprocess.run([sys.executable,str(ROOT/'tools/codex_startup_report.py'),str(cap),'--end-offset',str(len(PROOF)),'--json'],capture_output=True,text=True)
  self.assertEqual(report.returncode,0,report.stderr);path.write_text(report.stdout)
  out=cap.parent/'receipt.json';r=self.cli(cap,path,out);self.assertEqual(r.returncode,0,r.stdout+r.stderr)
  self.assertEqual(json.loads(out.read_text())['source_kind'],'startup_report_v1')
 def test_frozen_replay_excludes_later_generation(self):
  cap,path,s=self.setup_case(tail=FS+OFF+b'hv: vm0 lifecycle: generation=4 assigned slot=10\n')
  r=gate.observe(path,SHA,cap,0,end_offset=len(PROOF+FS+OFF))
  self.assertTrue(r['success']);self.assertEqual(r['observation_kind'],'frozen_window')
  self.assertEqual(r['window']['sha256'],hashlib.sha256(PROOF+FS+OFF).hexdigest())
  self.assertFalse(gate.observe(path,SHA,cap,0)['success'])
 def test_frozen_end_bounds_refused(self):
  cap,path,s=self.setup_case(tail=FS+OFF)
  for end in (-1,len(PROOF)-1,cap.stat().st_size+1):
   self.assertEqual(gate.observe(path,SHA,cap,0,end_offset=end)['reason'],'invalid_frozen_end_offset')
 def test_frozen_window_cannot_wait_for_future_offline(self):
  cap,path,s=self.setup_case(tail=FS+OFF)
  r=gate.observe(path,SHA,cap,60,end_offset=len(PROOF+FS))
  self.assertFalse(r['success']);self.assertEqual(r['reason'],'frozen_window_missing_retirement_or_partial_line')
 def test_cli_fixed_endpoint_replays_historical_retirement(self):
  cap,path,s=self.setup_case(tail=FS+OFF+b'hv: vm0 lifecycle: generation=4 assigned slot=10\n')
  out=cap.parent/'receipt.json';r=self.cli(cap,path,out,('--end-offset',str(len(PROOF+FS+OFF))))
  self.assertEqual(r.returncode,0,r.stdout+r.stderr);self.assertEqual(json.loads(out.read_text())['observation_kind'],'frozen_window')

if __name__=='__main__':
 unittest.main()
