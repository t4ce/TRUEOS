"""Production recording clock/status for deterministic backend host tests."""
from pathlib import Path
import re
ROOT = Path(__file__).resolve().parents[2]

def model():
    source = (ROOT / 'src/shell3/capture.rs').read_text()
    pieces = []
    for marker in ('struct Clock', 'impl Clock', 'pub(crate) struct RecordingStatus', 'pub(crate) struct Recording', 'impl Recording'):
        match = re.search(r'^' + re.escape(marker) + r' \{.*?^}', source, re.M | re.S)
        pieces.append(match.group())
    return '''mod shell3 {
    pub mod service {pub fn notify_work(){}}
    pub mod capture {
        use alloc::{string::String, sync::Arc, vec::Vec};
        use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
        use spin::Mutex;
        use trueos_time::{Duration, Timer};
        const VIDEO:u8=1;const AUDIO:u8=2;
''' + pieces[0] + '\n' + pieces[1] + '\n#[derive(Default)]\n' + pieces[2] + '\n#[derive(Clone)]\n' + pieces[3] + '\n' + pieces[4] + '''
        pub fn recording(track:u8,seconds:u32,path:&str)->Recording {
            Recording::new(crate::disc::block::DeviceHandle,path.into(),Clock::new(track,seconds),track)
        }
    }
}
'''
