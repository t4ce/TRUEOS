#![allow(dead_code)]
extern crate self as trueos_time;

use std::sync::atomic::{AtomicU64, Ordering};
static NOW: AtomicU64 = AtomicU64::new(0);
static WARNINGS: AtomicU64 = AtomicU64::new(0);
static AUDIO_STREAM: AtomicU64 = AtomicU64::new(0);

pub struct Instant(u64);
impl Instant {
    pub fn now() -> Self { Self(NOW.load(Ordering::Relaxed)) }
    pub fn as_millis(self) -> u64 { self.0 }
}

#[macro_export]
macro_rules! log_warn {
    (target: $target:expr; $($arg:tt)*) => {{
        let _ = format_args!($($arg)*);
        $crate::WARNINGS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }};
}

mod tyche {
    pub struct SoftRng(u64);
    pub fn soft_rng() -> SoftRng { SoftRng(1) }
    impl SoftRng {
        fn next(&mut self) -> u64 {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1);
            self.0 >> 32
        }
        pub fn usize_below(&mut self, max: usize) -> usize { self.next() as usize % max }
        pub fn bool(&mut self) -> bool { self.usize_below(2) != 0 }
    }
}

mod r {
    pub mod services {
        pub mod audio_playback {
            pub fn source_active(id: u64) -> bool {
                crate::AUDIO_STREAM.load(crate::Ordering::Relaxed) == id
            }
        }
    }
}

mod spirit {
    pub mod lilly {
        include!("lilly_types.rs");

        pub fn resident_frame(path: &str) -> Option<LillyResidentFrame> {
            let offset = path.bytes().fold(0usize, |sum, byte| {
                sum.wrapping_mul(31).wrapping_add(usize::from(byte))
            });
            Some(LillyResidentFrame { offset })
        }
    }
    pub mod codex_handoff {
        use std::sync::atomic::{AtomicBool, Ordering};
        pub static REQUESTED: AtomicBool = AtomicBool::new(false);
        pub static READY: AtomicBool = AtomicBool::new(false);
        pub fn needs_crossed() -> bool { REQUESTED.load(Ordering::Relaxed) }
        pub fn crossed_ready() { READY.store(true, Ordering::Relaxed); }
    }
    #[path = "@TRUEOS_ROOT@/src/spirit/lilly_sequencer.rs"]
    mod lilly_sequencer;
    #[path = "@TRUEOS_ROOT@/src/spirit/lilly_protocol.rs"]
    mod lilly_protocol;

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::{AUDIO_STREAM, NOW, WARNINGS, Ordering, tyche};
        use lilly_sequencer::{Contextual, IdleMode, Playback, Sequencer, SpeechCue, SpeechTurn, TalkMoods};

        const CROSS: &str = "Static_up/static.png";
        const UNCROSS: &str = "Static_up/uncrossed.png";

        fn tick(state: &mut Sequencer, time: &mut u64) -> Playback {
            let clip = state.next(*time, false);
            *time += clip.duration_ms();
            clip
        }

        fn clip_path(clip: Playback) -> &'static str {
            clip.frames[clip.frame_index(0)]
        }

        #[test]
        fn legacy_signals_only_warn_and_collapse_has_priority_over_a_completed_turn() {
            use lilly_protocol as protocol;
            protocol::enqueue_presentation_word("vibe:happy").unwrap();
            protocol::enqueue_presentation_word("Happy").unwrap();
            assert_eq!(WARNINGS.load(Ordering::Relaxed), 2);
            assert_eq!(protocol::next_animation().unwrap().frame_at_elapsed(0), lilly::resident_frame(CROSS).unwrap());
            assert!(protocol::enqueue_contextual_word("vibe:happy").is_err());

            protocol::set_idle_mode(IdleMode::Typing);
            assert_eq!(protocol::idle_mode(), IdleMode::Typing);
            let talk = IdleMode::Talk(TalkMoods::from_bits(TalkMoods::CALM));
            protocol::set_idle_mode(talk);
            assert_eq!(protocol::idle_mode(), talk);
            assert_eq!(protocol::idle_ambient_mode(), IdleMode::Typing);
            protocol::set_idle_mode(IdleMode::Talk(TalkMoods::default()));
            assert_eq!(protocol::idle_mode(), IdleMode::Typing);
            let mut time = 50;
            for path in ["Transitions_up/unfolding-arms/frame_02.png", "Freestyle_up/wireless-keyboard/frame_02.png"] {
                NOW.store(time, Ordering::Relaxed);
                let active = protocol::next_animation().unwrap();
                assert_eq!(active.frame_at_elapsed(100), lilly::resident_frame(path).unwrap());
                time += active.boundary_ms;
            }
            protocol::enqueue_contextual_word("Happy").unwrap();
            assert_eq!(protocol::idle_mode(), IdleMode::Normal);
            codex_handoff::REQUESTED.store(true, Ordering::Relaxed);
            for _ in 0..2 {
                NOW.store(time, Ordering::Relaxed);
                let active = protocol::next_animation().unwrap();
                assert!(!codex_handoff::READY.load(Ordering::Relaxed));
                time += active.boundary_ms;
            }
            NOW.store(time, Ordering::Relaxed);
            let held = protocol::next_animation().unwrap();
            assert_eq!(held.frame_at_elapsed(0), lilly::resident_frame(CROSS).unwrap());
            assert!(codex_handoff::READY.load(Ordering::Relaxed));
            codex_handoff::REQUESTED.store(false, Ordering::Relaxed);
            assert_eq!(protocol::next_animation().unwrap().frame_at_elapsed(450),
                lilly::resident_frame("Happy2_up/frame_02.png").unwrap());
        }

        #[test]
        fn lico_selection_only_releases_sound_it_enabled() {
            let mut state = Sequencer::new(tyche::soft_rng());
            state.set_window_sound(true);
            assert_eq!(state.mode(), IdleMode::Sound);
            state.set_window_sound(true);
            state.set_window_sound(false);
            assert_eq!(state.mode(), IdleMode::Normal);

            let talk = IdleMode::Talk(TalkMoods::from_bits(TalkMoods::CALM));
            state.set_mode(IdleMode::Reading);
            state.set_mode(talk);
            state.set_window_sound(true);
            assert_eq!(state.mode(), talk);
            assert_eq!(state.ambient_mode(), IdleMode::Reading);
            state.set_mode(IdleMode::Talk(TalkMoods::default()));
            assert_eq!(state.mode(), IdleMode::Reading);
            state.set_window_sound(false);
            assert_eq!(state.mode(), IdleMode::Reading);

            state.set_mode(IdleMode::Sound);
            state.set_window_sound(true);
            state.set_window_sound(false);
            assert_eq!(state.mode(), IdleMode::Sound);

            state.set_mode(IdleMode::Normal);
            state.set_window_sound(true);
            state.set_mode(IdleMode::Sound);
            state.set_window_sound(false);
            assert_eq!(state.mode(), IdleMode::Sound);

            state.set_window_sound(true);
            state.set_mode(IdleMode::Reading);
            state.set_window_sound(false);
            assert_eq!(state.mode(), IdleMode::Reading);

            state.set_window_sound(true);
            assert!(state.enqueue(Contextual::Nod));
            assert_eq!(state.mode(), IdleMode::Normal);
            state.set_window_sound(false);
            assert_eq!(state.mode(), IdleMode::Normal);
        }

        #[test]
        fn lico_selection_plays_cat_mouth_then_headset_and_periodic_dance() {
            const HEADSET: &str = "Headset_up/static.png";
            let mut state = Sequencer::new(tyche::soft_rng());
            let mut time = 0;
            state.set_window_sound(true);

            for prefix in [
                "Transitions_up/unfolding-arms/",
                "Silly_up/cat-mouth/",
                "Headset_up/put-on/",
            ] {
                let clip = tick(&mut state, &mut time);
                assert!(clip.frames[1].starts_with(prefix));
            }
            for _ in 0..2 {
                let rest_start = time;
                loop {
                    let start = time;
                    let clip = tick(&mut state, &mut time);
                    if clip.frames.len() > 1 {
                        assert!((3000..=5000).contains(&(start - rest_start)));
                        assert!(clip.frames[1].starts_with("Headset_up/dance/"));
                        break;
                    }
                    assert_eq!(clip_path(clip), HEADSET);
                    assert!(start - rest_start < 5000);
                }
            }
            state.set_window_sound(false);
            let exit = tick(&mut state, &mut time);
            assert!(exit.frames[1].starts_with("Headset_up/take-off/"));
        }

        #[test]
        fn repeat_speed_uses_the_same_food_and_two_different_rates() {
            let mut state = Sequencer::new(tyche::soft_rng());
            let mut time = 0;
            assert!(state.enqueue(Contextual::Nod));
            tick(&mut state, &mut time);
            tick(&mut state, &mut time);
            assert_eq!(clip_path(tick(&mut state, &mut time)), UNCROSS);
            assert!(state.enqueue(Contextual::Eat));
            let a = tick(&mut state, &mut time);
            let b = tick(&mut state, &mut time);
            assert_eq!(a.frames, b.frames);
            assert_eq!(a.frame_index(0), 0);
            assert_eq!(b.frame_index(0), 0);
            let (mut durations, authored) = ([a.duration_ms(), b.duration_ms()], a.duration_ms().min(b.duration_ms()));
            durations.sort();
            assert_eq!(durations, [authored, authored * 3 / 2]);
        }

        #[test]
        fn contextual_groups_across_poses_and_default_idle_cadence() {
            let mut state = Sequencer::new(tyche::soft_rng());
            let mut time = 0;
            assert!(state.enqueue(Contextual::Flirt));
            let mut interiors = Vec::new();
            loop {
                let clip = tick(&mut state, &mut time);
                if clip.frames.len() == 1 { break; }
                if !clip.frames[1].starts_with("Transitions_up/") { interiors.push(clip.frames[1]); }
            }
            assert_eq!(interiors.len(), 2);
            assert_ne!(interiors[0], interiors[1]);
            let settled_at = time - 50;
            while tick(&mut state, &mut time).frames.len() == 1 {}
            assert!(time > settled_at + 3000);
            assert!(time < settled_at + 6500); // Includes one scheduled clip.
        }

        #[test]
        fn selected_talk_moods_chain_two_clips_then_start_a_new_pause() {
            let mut state = Sequencer::new(tyche::soft_rng());
            state.set_mode(IdleMode::Talk(TalkMoods::from_bits(TalkMoods::CALM)));
            let mut time = 0;
            for _ in 0..2 {
                let settled_at = time;
                for _ in 0..10 {
                    assert_eq!(tick(&mut state, &mut time).frames, &[CROSS]);
                }
                assert_eq!(time, settled_at + 500);
                for first_frame in [0, 1] {
                    let clip = tick(&mut state, &mut time);
                    assert_eq!(clip.frames[1], "Talk_up/calm/frame_02.png");
                    assert_eq!(clip.frame_index(0), first_frame);
                }
            }
            state.set_mode(IdleMode::Sound);
            assert_eq!(state.mode(), IdleMode::Talk(TalkMoods::from_bits(TalkMoods::CALM)));
            state.set_mode(IdleMode::Talk(TalkMoods::default()));
            assert_eq!(state.mode(), IdleMode::Sound);
            let mut saw_cat = false;
            loop {
                let clip = tick(&mut state, &mut time);
                if clip.frames.len() == 1 { break; }
                if clip.frames[1].contains("cat-mouth") { saw_cat = true; }
                if clip.frames[1].contains("put-on") { assert!(saw_cat); }
            }
            assert!(saw_cat);
        }

        #[test]
        fn talk_suspends_the_selected_ambient_mode_until_the_last_mood_clears() {
            let mut state = Sequencer::new(tyche::soft_rng());
            let mut time = 0;
            state.set_mode(IdleMode::Typing);
            assert_eq!(tick(&mut state, &mut time).frames.last(), Some(&UNCROSS));
            assert_eq!(tick(&mut state, &mut time).frames[1], "Freestyle_up/wireless-keyboard/frame_02.png");

            let calm = IdleMode::Talk(TalkMoods::from_bits(TalkMoods::CALM));
            state.set_mode(IdleMode::Talk(TalkMoods::from_bits(TalkMoods::CALM | TalkMoods::SHY)));
            assert_eq!(state.ambient_mode(), IdleMode::Typing);
            assert_eq!(tick(&mut state, &mut time).frames[1], "Freestyle_up/wireless-keyboard/frame_07.png");
            assert_eq!(tick(&mut state, &mut time).frames.last(), Some(&CROSS));

            state.set_mode(calm);
            let settled_at = time;
            for _ in 0..4 {
                assert_eq!(tick(&mut state, &mut time).frames, &[CROSS]);
            }
            state.set_mode(IdleMode::Reading);
            assert_eq!(state.ambient_mode(), IdleMode::Reading);
            assert_eq!(state.mode(), calm);
            for _ in 0..6 {
                assert_eq!(tick(&mut state, &mut time).frames, &[CROSS]);
            }
            assert_eq!(time, settled_at + 500);
            assert_eq!(tick(&mut state, &mut time).frames[1], "Talk_up/calm/frame_02.png");

            assert!(state.enqueue(Contextual::Happy));
            assert_eq!(state.ambient_mode(), IdleMode::Reading);
            state.set_mode(IdleMode::Talk(TalkMoods::default()));
            assert_eq!(state.mode(), IdleMode::Reading);
            assert_eq!(tick(&mut state, &mut time).frames[1], "Happy2_up/frame_02.png");
            assert_eq!(tick(&mut state, &mut time).frames[1], "Reading_up/open-book/frame_02.png");
        }

        #[test]
        fn multiple_talk_moods_extend_the_pool_for_uncrossed_arms() {
            let mut state = Sequencer::new(tyche::soft_rng());
            let mut time = 0;
            assert!(state.enqueue(Contextual::Nod));
            assert_eq!(tick(&mut state, &mut time).frames.last(), Some(&UNCROSS));
            assert_eq!(tick(&mut state, &mut time).frames[1], "Nod_up/frame_02.png");

            let moods = TalkMoods::from_bits(TalkMoods::CALM | TalkMoods::SHY);
            state.set_mode(IdleMode::Talk(moods));
            let mut seen = std::collections::BTreeSet::new();
            for _ in 0..12 {
                for _ in 0..10 {
                    assert_eq!(tick(&mut state, &mut time).frames, &[UNCROSS]);
                }
                for _ in 0..2 {
                    let clip = tick(&mut state, &mut time);
                    assert_eq!(clip.frames.first(), Some(&UNCROSS));
                    assert_eq!(clip.frames.last(), Some(&UNCROSS));
                    seen.insert(clip.frames[1]);
                }
            }
            assert_eq!(seen.into_iter().collect::<Vec<_>>(), [
                "Talk_up/calm-2/frame_02.png",
                "Talk_up/shy-2/frame_02.png",
            ]);
        }

        #[test]
        fn ptts_uses_one_wave_and_one_idea_then_restores_the_current_ambient_choice() {
            let mut state = Sequencer::new(tyche::soft_rng());
            let mut time = 0;
            state.set_mode(IdleMode::Reading);
            assert_eq!(tick(&mut state, &mut time).frames[1], "Reading_up/open-book/frame_02.png");

            let turn = SpeechTurn { vm_id: 4, generation: 9, turn: 1 };
            let speaking = IdleMode::Talk(TalkMoods::from_bits(TalkMoods::CALM | TalkMoods::EXCITED));
            state.speech_event(turn, 1, SpeechCue::Thought);
            assert_eq!(state.mode(), speaking);
            assert_eq!(state.ambient_mode(), IdleMode::Reading);
            assert_eq!(tick(&mut state, &mut time).frames[1], "Reading_up/put-away/frame_02.png");
            assert_eq!(tick(&mut state, &mut time).frames[1], "Waving_up/crossed-to-uncrossed/frame_02.png");

            state.observe_speech_stream(1, false, time);
            state.speech_event(turn, 2, SpeechCue::Continue);
            state.observe_speech_stream(1, false, time + 250);
            assert_eq!(state.mode(), speaking);
            for _ in 0..8 {
                for _ in 0..10 {
                    assert_eq!(tick(&mut state, &mut time).frames, &[UNCROSS]);
                }
                for _ in 0..2 {
                    let path = tick(&mut state, &mut time).frames[1];
                    assert!(matches!(path, "Talk_up/calm-2/frame_02.png" | "Talk_up/excited-2/frame_02.png"));
                }
            }

            state.speech_event(turn, 3, SpeechCue::Answer);
            state.speech_event(turn, 3, SpeechCue::Answer);
            assert_eq!(tick(&mut state, &mut time).frames.last(), Some(&CROSS));
            assert_eq!(tick(&mut state, &mut time).frames[1], "Idea_up/frame_02.png");
            assert_eq!(tick(&mut state, &mut time).frames, &[CROSS]);

            state.set_mode(IdleMode::Typing);
            state.speech_event(SpeechTurn { turn: 2, ..turn }, 0, SpeechCue::End);
            assert_eq!(state.mode(), speaking);
            state.speech_event(turn, 4, SpeechCue::Done);
            assert_eq!(state.mode(), IdleMode::Typing);
            state.speech_event(turn, 5, SpeechCue::Continue);
            assert_eq!(state.mode(), IdleMode::Typing);
        }

        #[test]
        fn ptts_drain_releases_talk_without_a_done_sound() {
            use lilly_protocol as protocol;
            let manual = IdleMode::Talk(TalkMoods::from_bits(TalkMoods::SHY));
            protocol::set_idle_mode(manual);
            let turn = SpeechTurn { vm_id: 4, generation: 9, turn: 1 };
            AUDIO_STREAM.store(17, Ordering::Relaxed);
            protocol::speech_event(turn, 17, SpeechCue::Continue);
            let speaking = IdleMode::Talk(TalkMoods::from_bits(TalkMoods::CALM | TalkMoods::EXCITED));
            assert_eq!(protocol::idle_mode(), speaking);
            let wave = protocol::next_animation().unwrap();
            assert_eq!(wave.frame_at_elapsed(450), lilly::resident_frame("Waving_up/crossed-to-uncrossed/frame_02.png").unwrap());
            AUDIO_STREAM.store(0, Ordering::Relaxed);
            NOW.store(wave.boundary_ms, Ordering::Relaxed);
            protocol::next_animation().unwrap();
            assert_eq!(protocol::idle_mode(), speaking);
            NOW.store(wave.boundary_ms + 250, Ordering::Relaxed);
            protocol::next_animation().unwrap();
            assert_eq!(protocol::idle_mode(), manual);
            AUDIO_STREAM.store(18, Ordering::Relaxed);
            protocol::speech_event(turn, 18, SpeechCue::Continue);
            assert_eq!(protocol::next_animation().unwrap().frame_at_elapsed(0), lilly::resident_frame(UNCROSS).unwrap());
        }

        #[test]
        fn ptts_answer_only_preserves_idea_after_the_done_tone() {
            let mut state = Sequencer::new(tyche::soft_rng());
            let mut time = 0;
            let turn = SpeechTurn { vm_id: 4, generation: 9, turn: 1 };
            state.speech_event(turn, 1, SpeechCue::Answer);
            assert_eq!(tick(&mut state, &mut time).frames[1], "Waving_up/crossed-to-uncrossed/frame_02.png");

            state.set_mode(IdleMode::Reading);
            state.speech_event(turn, 2, SpeechCue::Done);
            assert_eq!(state.mode(), IdleMode::Reading);
            assert_eq!(tick(&mut state, &mut time).frames.last(), Some(&CROSS));
            assert_eq!(tick(&mut state, &mut time).frames[1], "Idea_up/frame_02.png");
            assert_eq!(tick(&mut state, &mut time).frames[1], "Reading_up/open-book/frame_02.png");
            state.set_mode(IdleMode::Normal);
            assert_eq!(tick(&mut state, &mut time).frames[1], "Reading_up/put-away/frame_02.png");
            assert_eq!(tick(&mut state, &mut time).frames, &[CROSS]);
        }

        #[test]
        fn ptts_silent_success_keeps_the_wave_but_abort_discards_it() {
            let mut state = Sequencer::new(tyche::soft_rng());
            let mut time = 0;
            let turn = SpeechTurn { vm_id: 4, generation: 9, turn: 1 };
            state.speech_event(turn, 1, SpeechCue::Thought);
            state.speech_event(turn, 0, SpeechCue::Abort);
            assert_eq!(state.mode(), IdleMode::Normal);
            assert_eq!(tick(&mut state, &mut time).frames, &[CROSS]);

            let turn = SpeechTurn { turn: 2, ..turn };
            state.speech_event(turn, 2, SpeechCue::Thought);
            state.speech_event(turn, 0, SpeechCue::End);
            assert_eq!(state.mode(), IdleMode::Normal);
            assert_eq!(tick(&mut state, &mut time).frames[1], "Waving_up/crossed-to-uncrossed/frame_02.png");
        }
    }
}
