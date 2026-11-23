#!/usr/bin/env python3
"""Exercise Audio Link choices with the production Shell3 picker and input."""

import json
from pathlib import Path
import subprocess
import tempfile

from test_clip_position3_uv_texture import constant, item


ROOT = Path(__file__).resolve().parents[2]
HELPER = "src/shell3/helper.rs"
AUDIO_LINK = "src/shell3/system/audio_link.rs"
SINK = "src/r/services/audio_playback/sink.rs"


def main() -> None:
    helper = (ROOT / HELPER).read_text().replace(item(HELPER, "admit"), "")
    sink = "\n".join(
        [constant(SINK, "MAX_PRODUCERS")]
        + [item(SINK, name) for name in ("OutputBucket", "ProducerOwner", "VideoAudioOwner", "RouteSource")]
    )
    choices = "\n".join(item(AUDIO_LINK, name) for name in ("Choice", "source_label", "choices"))
    picker = json.dumps(str(ROOT / "src/shell3/system/picker.rs"))
    harness = r'''
#![allow(dead_code)]
extern crate alloc;

mod hv {
    pub(crate) fn blueprint_current_run_archive(id: u8, generation: u64) -> Option<String> {
        (id == 7 && generation == 4).then(|| "/apps/player.bp".into())
    }
}

mod shell3 {
    pub(super) struct MatrixTarget;
    mod tui {
        pub(super) struct Frontend;
        pub(super) fn native_write(_: &super::MatrixTarget, _: &[u8]) {}
    }
    #[allow(unused_imports)]
    mod helper { @HELPER@ }
    #[path = @PICKER@]
    mod picker;
    mod audio_link {
        use super::picker::{Event, Picker};
        use alloc::{format, string::String, vec, vec::Vec};
        @SINK@
        @CHOICES@

        const PLAYER: RouteSource = RouteSource {
            id: 44,
            owner: ProducerOwner::Vm { id: 7, generation: 4 },
            http: false,
            analog: true,
            active: true,
        };

        #[test]
        fn picker_shows_each_bucket_and_selects_the_drawn_stream() {
            let items = choices([Some(PLAYER), None], true);
            let mut picker = Picker::default();
            let labels: Vec<_> = items.iter().map(|(_, label)| label.as_str()).collect();
            let lines = picker.frame("Audio Link", "Mainboard jack: Disconnected", &labels, 12);
            assert!(lines[2].contains("Disconnected"));
            assert!(lines[4].contains("HTTP server [On]"));
            assert!(lines[5].contains("HTTP [ ] #44 player.bp (VM 7)"));
            assert!(lines[6].contains("Jack [x] #44 player.bp (VM 7)"));
            let Some(Event::Choose(index)) = picker.input(b"\x1b[<0;12;7M", items.len(), 12, 0) else {
                panic!("click did not select the jack row");
            };
            assert!(matches!(items[index].0, Choice::Route(44, OutputBucket::Analog)));
            let Some(Event::Choose(index)) = picker.input(b"\x1b[A\r", items.len(), 12, 0) else {
                panic!("keyboard did not select the HTTP route");
            };
            assert!(matches!(items[index].0, Choice::Route(44, OutputBucket::Http)));
            assert!(matches!(picker.input(b"q", items.len(), 12, 0), Some(Event::Quit)));
        }

        #[test]
        fn recent_clips_remain_assignable_with_http_off() {
            let recent = RouteSource { active: false, ..PLAYER };
            let items = choices([None, Some(recent)], false);
            assert!(matches!(items[0].0, Choice::SetHttp(true)));
            assert!(items[0].1.contains("HTTP server [Off]"));
            assert!(items[1].1.contains("HTTP [ ] Recent clip #44"));
            assert!(matches!(items[1].0, Choice::Route(44, OutputBucket::Http)));
        }

        #[test]
        fn video_soundtrack_has_the_same_output_choices_as_a_blueprint() {
            let video = RouteSource {
                id: 45,
                owner: ProducerOwner::Video(VideoAudioOwner { slot: 0, generation: 11 }),
                http: true,
                analog: true,
                active: true,
            };
            let items = choices([Some(PLAYER), Some(video)], true);
            assert_eq!(items[3].1, "HTTP [x] #45 Video");
            assert_eq!(items[4].1, "Jack [x] #45 Video");
            assert!(matches!(items[3].0, Choice::Route(45, OutputBucket::Http)));
            assert!(matches!(items[4].0, Choice::Route(45, OutputBucket::Analog)));
        }
    }
}
'''
    for name, source in (("PICKER", picker), ("HELPER", helper), ("SINK", sink), ("CHOICES", choices)):
        harness = harness.replace("@" + name + "@", source)
    with tempfile.TemporaryDirectory(prefix="trueos-audio-link-tui-") as directory:
        rust = Path(directory) / "harness.rs"
        test = Path(directory) / "tests"
        rust.write_text(harness)
        subprocess.run(
            ["rustc", "--edition=2024", "--test", "--target", "x86_64-unknown-linux-gnu",
             str(rust), "-o", str(test)],
            cwd=ROOT,
            check=True,
        )
        subprocess.run([str(test)], cwd=ROOT, check=True)


if __name__ == "__main__":
    main()
