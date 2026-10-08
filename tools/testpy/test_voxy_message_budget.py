#!/usr/bin/env python3
"""Exercise the real client drain against continuously replenished traffic."""
from pathlib import Path
import subprocess
import tempfile

root = Path(__file__).resolve().parents[2]
source = (root.parent / 'voxy/src/client/mod.rs').read_text()
begin = source.index('    fn handle_messages(')
method = source[begin:source.index('    /// Handle new server messages.', begin)]
harness = r'''
#![allow(dead_code, unreachable_code, dropping_copy_types)]
type Error = ();
type Event = u8;
mod selection_progress {
    pub enum Stage { GeneralMessages, PingMessages, CharacterMessages, InGameMessages, TerrainMessages }
    pub fn stage(_: Stage) {}
}
#[derive(Default)] struct Stream { continuous: bool, remaining: usize }
impl Stream { fn try_recv(&mut self) -> Result<Option<u8>, Error> {
    if self.continuous { Ok(Some(1)) }
    else if self.remaining > 0 { self.remaining -= 1; Ok(Some(1)) }
    else { Ok(None) }
}}
#[derive(Default)] struct Client {
    general_stream: Stream, ping_stream: Stream, character_screen_stream: Stream,
    in_game_stream: Stream, terrain_stream: Stream, terrain_decode_queue: Vec<u8>, counts: [usize; 5],
}
impl Client {
    fn handle_server_msg(&mut self, _: &mut Vec<Event>, _: u8) -> Result<(), Error> { self.counts[0] += 1; Ok(()) }
    fn handle_ping_msg(&mut self, _: u8) -> Result<(), Error> { self.counts[1] += 1; Ok(()) }
    fn handle_server_character_screen_msg(&mut self, events: &mut Vec<Event>, _: u8) -> Result<(), Error> { self.counts[2] += 1; events.push(42); Ok(()) }
    fn handle_server_in_game_msg(&mut self, _: &mut Vec<Event>, _: u8) -> Result<(), Error> { self.counts[3] += 1; Ok(()) }
    fn handle_server_terrain_msg(&mut self, _: u8) -> Result<(), Error> { self.counts[4] += 1; Ok(()) }
'''+method+r'''
}
#[test] fn join_ack_is_served_despite_continuous_general_traffic() {
    let mut client = Client::default(); client.general_stream.continuous = true;
    client.character_screen_stream.remaining = 1;
    let mut events = Vec::new();
    assert_eq!(client.handle_messages(&mut events).unwrap(), 9);
    assert_eq!(events, [42]); assert_eq!(client.counts, [8,0,1,0,0]);
}
#[test] fn every_stream_gets_a_bounded_turn_before_returning_to_input() {
    let mut client = Client::default();
    client.general_stream.continuous = true; client.ping_stream.continuous = true;
    client.character_screen_stream.continuous = true; client.in_game_stream.continuous = true;
    client.terrain_stream.continuous = true;
    assert_eq!(client.handle_messages(&mut Vec::new()).unwrap(), 40);
    assert_eq!(client.counts, [8; 5]);
    client.terrain_decode_queue.resize(32, 0);
    assert_eq!(client.handle_messages(&mut Vec::new()).unwrap(), 32);
    assert_eq!(client.counts, [16,16,16,16,8]);
}
'''
with tempfile.TemporaryDirectory(prefix='voxy-message-budget-') as directory:
    path = Path(directory) / 'test.rs'
    path.write_text(harness)
    executable = Path(directory) / 'tests'
    subprocess.run(['rustc', '--edition=2024', '--test', '--cfg', 'target_os="trueos"',
                    '-Aexplicit_builtin_cfgs_in_flags', str(path), '-o', str(executable)], check=True)
    subprocess.run([str(executable)], check=True, timeout=5)
