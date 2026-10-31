#!/usr/bin/env python3
"""Check bounded native-FS numeric capture reports without a rig."""
import importlib.util,io,unittest,hashlib,subprocess,tempfile,json
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2]
P=ROOT/'tools/codex_native_fs_report.py'
spec=importlib.util.spec_from_file_location('report',P);m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
def row(**kw):
 d=dict(vm=0,generation=11,launch=6,request=1042,op=3,phase=3,status=0,queued_ms=0,service_ms=5000,phase_ms=5000,age_ms=5000,full=0,missed=0,expired=0,released=99);d.update(kw)
 return b'prefix codex-fs-native '+b' '.join(k.encode()+b'='+str(v).encode() for k,v in d.items())+b'\n'
def report(data,start=0,end=None):return m.read_window(io.BytesIO(data),start,len(data) if end is None else end,len(data))
class Tests(unittest.TestCase):
 def test_row_and_join(self):
  r=report(row(phase=1)+row(age_ms=10000));self.assertEqual(len(r['requests']),1);self.assertEqual(r['requests'][0]['last_phase_name'],'filesystem');self.assertEqual(r['requests'][0]['max_age_ms'],10000)
 def test_summary_retains_qualifiers(self):
  r=report(row()+b'codex-fs-native-summary full=1 missed=2 expired=1 released=100\n');self.assertEqual(r['counter_delta'],dict(full=1,missed=2,expired=1,released=1));self.assertTrue(any('never proves' in q for q in r['qualifiers']))
 def test_reboot_counter_drop_detected_anywhere(self):
  r=report(row(released=10)+row(released=0)+row(released=12));self.assertTrue(r['counter_decrease_detected']);self.assertIsNone(r['counter_delta'])
 def test_hash_exact_bounded_window(self):
  body=row();r=report(b'junk\n'+body+b'unread secret',5,5+len(body));self.assertEqual(r['capture_window']['sha256'],hashlib.sha256(body).hexdigest());self.assertEqual(r['rows'][0]['offset'],5)
 def test_no_unrelated_payload_output(self):
  r=report(b'password=unrelated-secret\n'+row()+b'codex-fs-native secret=bad-path\n');self.assertNotIn('secret',json.dumps(r));self.assertEqual(r['malformed_marker_lines'],1)
 def test_ansi_prefix_suffix(self):self.assertEqual(len(report(b'\x1b[1m'+row().rstrip()+b'\x1b[0m\r\n')['rows']),1)
 def test_read_range_subphases_and_original_meanings(self):
  self.assertEqual([m.PHASES[i] for i in range(1,9)],['queued','service','filesystem','result','publish','published','retire','finished'])
  for phase in range(9,14):
   r=report(row(op=4,phase=phase));self.assertEqual(r['requests'][0]['last_phase_name'],m.PHASES[phase]);self.assertEqual(r['rows'][0]['phase'],phase)
 def test_read_range_subphase_rejects_other_operation(self):self.assertFalse(m.parse_line(row(op=3,phase=13)))
 def test_unknown_read_range_phase_rejected(self):self.assertFalse(m.parse_line(row(op=4,phase=14)))
 def test_unknown_fields_rejected(self):self.assertFalse(m.parse_line(row().rstrip()+b' payload=123'))
 def test_duplicate_fields_rejected(self):self.assertFalse(m.parse_line(row().rstrip()+b' vm=1'))
 def test_invalid_numeric_limits(self):
  for change in [dict(vm=64),dict(launch=0),dict(request=0),dict(request=m.U64_MAX),dict(op=19),dict(phase=0),dict(status=4),dict(age_ms=1<<64)]:self.assertFalse(m.parse_line(row(**change)))
 def test_oversized_line_skipped_and_recovers(self):
  r=report(b'x'*(m.MAX_LINE+1)+b'\n'+row());self.assertEqual(r['oversized_lines_skipped'],1);self.assertEqual(len(r['rows']),1)
 def test_chunk_split_and_crlf(self):
  class Tiny(io.BytesIO):
   def read(self,n):return super().read(min(n,3))
  data=row().replace(b'\n',b'\r\n')+row(request=9);r=m.read_window(Tiny(data),0,len(data),len(data));self.assertEqual([x['request'] for x in r['rows']],[1042,9])
 def test_invalid_bounds_and_shrink(self):
  for start,end,size,data in [(-1,1,2,b'xx'),(2,1,2,b'xx'),(0,3,2,b'xx'),(0,2,2,b'x')]:
   with self.assertRaises(ValueError):m.read_window(io.BytesIO(data),start,end,size)
 def test_empty_has_no_invented_completion(self):
  r=report(b'');self.assertEqual(r['requests'],[]);self.assertIsNone(r['counter_last']);self.assertFalse(any('completed' in str(x) for x in r['requests']))
 def test_cli_exports_numeric_only_and_rejects_bad_bounds(self):
  with tempfile.NamedTemporaryFile() as f:
   f.write(row());f.flush();r=subprocess.run(['python3',str(P),f.name],capture_output=True,text=True);self.assertEqual(r.returncode,0);self.assertEqual(len(json.loads(r.stdout)['rows']),1)
   r=subprocess.run(['python3',str(P),f.name,'--start-offset','-1'],capture_output=True,text=True);self.assertEqual(r.returncode,2);self.assertNotIn(f.name,r.stderr)
if __name__=='__main__':unittest.main()
