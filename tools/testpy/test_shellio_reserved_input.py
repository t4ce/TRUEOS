#!/usr/bin/env python3
"""Exercise the production terminal-state policy for ShellIO line applications."""
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
source = (ROOT / 'src/hv/mod.rs').read_text()
enum = re.search(r'enum BlueprintTerminalLeaseState \{.*?^\}', source, re.M | re.S).group()
impl = re.search(r'impl BlueprintTerminalLeaseState \{.*?^\}', source, re.M | re.S).group()
harness = '''
#[derive(Clone, Copy)] enum BlueprintConsoleSurface {Text, Terminal}
#[derive(Clone, Copy)]
''' + enum + '\n' + impl + '''
#[test] fn line_app_can_receive_stdin_without_claiming_terminal() {
    use BlueprintTerminalLeaseState::*;
    assert!(Reserved.accepts_console_input(true));
    assert!(!Reserved.accepts_console_input(false));
    assert!(Reserved.suppresses_terminal_output());
    for passthrough in [false,true] {
        assert!(Unsupported.accepts_console_input(passthrough));
        assert!(Active{epoch:1,observed:true,ready:true}.accepts_console_input(passthrough));
        for state in [Releasing{epoch:1}, Parked{ticket:1},
            ReentryRequested{ticket:1,epoch:2}, Claiming{ticket:1,epoch:2,direct:false}] {
            assert!(!state.accepts_console_input(passthrough));
        }
    }
}
'''
# Verify the actual ingress/read gates use this policy, not the paint policy.
for name in ['blueprint_app_command_passthrough_enabled', 'blueprint_forward_app_command',
             'blueprint_console_submit_stdin_inner', 'blueprint_console_read',
             'blueprint_console_readable_len']:
    fn = re.search(r'^(?:pub\(crate\) )?fn '+name+r'\(.*?^}', source, re.M | re.S).group()
    assert '.accepts_console_input(' in fn, name
with tempfile.TemporaryDirectory(prefix='shellio-reserved-') as temp:
    path = Path(temp)
    (path/'test.rs').write_text(harness)
    subprocess.run(['rustc','--edition=2024','--test','-A','dead_code',str(path/'test.rs'),'-o',str(path/'test')],check=True)
    subprocess.run([str(path/'test')],check=True)
