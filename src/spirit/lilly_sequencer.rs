//! The Lily preview's clip-boundary state machine. All durations are at 1×.
extern crate alloc;

use crate::tyche::SoftRng;
use alloc::collections::VecDeque;

#[path = "lilly_sequences.rs"]
mod catalog;

const QUEUE_CAPACITY: usize = 32;
const REST_POLL_MS: u64 = 50;
const SHARED_FRAME_MS: u16 = 100;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pose {
    Crossed,
    Uncrossed,
    Headset,
    Reading,
    Typing,
}

impl Pose {
    fn arms_only(self) -> bool {
        matches!(self, Self::Crossed | Self::Uncrossed)
    }
}

struct Clip {
    id: &'static str,
    entry: Pose,
    exit: Pose,
    frames: &'static [&'static str],
    durations: &'static [u16],
}

fn clip(id: &str) -> &'static Clip {
    catalog::CLIPS
        .iter()
        .find(|clip| clip.id == id)
        .expect("Lilly clip id")
}

#[cfg(test)]
pub(super) fn frame_paths() -> impl Iterator<Item = &'static str> {
    catalog::CLIPS
        .iter()
        .flat_map(|clip| clip.frames.iter().copied())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Contextual {
    Angry,
    Blush,
    Confused,
    Cry,
    Disapprove,
    Drink,
    Eat,
    Flirt,
    Wave,
    Tired,
    Grimace,
    Silly,
    Scared,
    Nod,
    Idea,
    Happy,
}

impl Contextual {
    pub(crate) fn from_word(word: &str) -> Option<Self> {
        use Contextual::*;
        const NAMES: [(&str, Contextual); 16] = [
            ("Angry", Angry),
            ("Blush", Blush),
            ("Confused", Confused),
            ("Cry", Cry),
            ("Disapprove", Disapprove),
            ("Drink", Drink),
            ("Eat", Eat),
            ("Flirt", Flirt),
            ("Wave", Wave),
            ("Tired", Tired),
            ("Grimace", Grimace),
            ("Silly", Silly),
            ("Scared", Scared),
            ("Nod", Nod),
            ("Idea", Idea),
            ("Happy", Happy),
        ];
        NAMES
            .iter()
            .find_map(|(name, action)| name.eq_ignore_ascii_case(word).then_some(*action))
    }

    fn variants(self) -> &'static [&'static str] {
        match self {
            Self::Angry => &[
                "Angry_up/fists",
                "Angry_up/fists-jump",
                "Threaten_up",
                "Angry_up/fists-clenched",
            ],
            Self::Blush => &[
                "Blush_up/evil",
                "Blush_up/shy",
                "Blush_up/teasing",
                "Blush_up/blush",
            ],
            Self::Confused => &["Confused_up"],
            Self::Cry => &[
                "Cry_up/crying",
                "Cry_up/crying-two-hands",
                "Cry_up/crying-wipe",
            ],
            Self::Disapprove => &[
                "Disapprove_up/head-shake",
                "Disapprove_up/shake-head",
                "Disapprove_up/tsk",
            ],
            Self::Drink => &["Eat_up/drink-tea"],
            Self::Eat => &["Eat_up/cookie", "Eat_up/onigiri"],
            Self::Flirt => &[
                "Flirt_up/come-here",
                "Flirt_up/finger-heart",
                "Flirt_up/hair-tuck",
                "Flirt_up/heart-hands",
                "Flirt_up/lean-in",
                "Blow-Kiss_up",
            ],
            Self::Wave => &[
                "Waving_up/excited",
                "Waving_up/neutral",
                "Waving_up/shy",
                "Waving_up/smile",
                "Waving_up/uncrossed-to-crossed",
                "Waving_up/crossed-to-uncrossed",
            ],
            Self::Tired => &[
                "Tired_up/sleepy",
                "Tired_up/sleepy-rubbing-eyes",
                "Tired_up/yawn",
            ],
            Self::Grimace => &["Silly_up/tongue-out", "Taunt_up/tongue-out", "Wink_up"],
            Self::Silly => &["Silly_up/puffed-cheeks", "Silly_up/roar", "Silly_up/teehee"],
            Self::Scared => &["Scared_up", "Blush_up/flustered"],
            Self::Nod => &["Nod_up"],
            Self::Idea => &["Idea_up", "Freestyle_up/clippy", "Finger-Gun_up"],
            Self::Happy => &["Happy2_up"],
        }
    }
}

/// Talk checkboxes are independent within the mutually exclusive idle modes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct TalkMoods(u8);

impl TalkMoods {
    pub(crate) const ANNOYED: u8 = 1;
    pub(crate) const CALM: u8 = 2;
    pub(crate) const EXCITED: u8 = 4;
    pub(crate) const SHY: u8 = 8;

    pub(crate) const fn from_bits(bits: u8) -> Self {
        Self(bits & 0x0f)
    }

    pub(crate) const fn bits(self) -> u8 {
        self.0
    }
}

const TALK_GROUPS: [&[&str]; 4] = [
    &[
        "Talk_up/annoyed",
        "Talk_up/annoyed-2",
        "Fish-Face_up/annoyed",
        "Facepalm_up",
    ],
    &["Talk_up/calm", "Talk_up/calm-2"],
    &["Talk_up/excited", "Talk_up/excited-2", "Talk_up/wave"],
    &["Talk_up/shy", "Talk_up/shy-2"],
];

const IDLES: [&str; 13] = [
    "Fish-Face_up/pout",
    "Idle_up/Crossed-Arms/idle-1",
    "Idle_up/Crossed-Arms/idle-2",
    "Idle_up/Crossed-Arms/idle-3",
    "Idle_up/Crossed-Arms/idle-4",
    "Idle_up/Crossed-Arms/idle-5",
    "Idle_up/Crossed-Arms/idle-6",
    "Idle_up/Crossed-Arms/idle-7",
    "Idle_up/Crossed-Arms/idle-scheming",
    "Idle_up/Uncrossed-Arms/idle-1",
    "Idle_up/Uncrossed-Arms/idle-2",
    "Idle_up/Uncrossed-Arms/idle-evil-smirk",
    "Idle_up/Uncrossed-Arms/idle-quiet-laugh",
];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum IdleMode {
    #[default]
    Normal,
    Sound,
    Reading,
    Typing,
    Talk(TalkMoods),
}

impl IdleMode {
    fn ambient_clip(self) -> Option<&'static Clip> {
        Some(clip(match self {
            Self::Sound => "Headset_up/dance",
            Self::Reading => "Reading_up",
            Self::Typing => "typing.keys",
            Self::Normal | Self::Talk(_) => return None,
        }))
    }

    fn talk_moods(self) -> u8 {
        match self {
            Self::Talk(moods) => moods.bits(),
            _ => 0,
        }
    }
}

#[derive(Clone, Copy)]
struct Request {
    clip: &'static Clip,
    slow: bool,
    talk: u8,
}

impl Request {
    fn new(clip: &'static Clip) -> Self {
        Self {
            clip,
            slow: false,
            talk: 0,
        }
    }
}

#[derive(Clone, Copy)]
struct End {
    source: &'static str,
    automatic: bool,
    talk: bool,
}

#[derive(Clone, Copy)]
pub(super) struct Playback {
    pub(super) frames: &'static [&'static str],
    durations: &'static [u16],
    first: usize,
    short_start: bool,
    short_end: bool,
    slow: bool,
    pub(super) crossed_for_collapse: bool,
}

impl Playback {
    fn holding(pose: Pose, crossed_for_collapse: bool) -> Self {
        let frames: &[&str] = match pose {
            Pose::Crossed => &[catalog::CROSSED],
            Pose::Uncrossed => &[catalog::UNCROSSED],
            Pose::Headset => &[catalog::HEADSET],
            Pose::Reading => &[catalog::READING],
            Pose::Typing => &[catalog::TYPING],
        };
        Self {
            frames,
            durations: &[REST_POLL_MS as u16],
            first: 0,
            short_start: false,
            short_end: false,
            slow: false,
            crossed_for_collapse,
        }
    }

    fn frame_ms(self, index: usize) -> u64 {
        let mut ms = self.durations[index];
        if (index == 0 && self.short_start) || (index + 1 == self.frames.len() && self.short_end) {
            ms = ms.min(SHARED_FRAME_MS);
        }
        if self.slow {
            u64::from(ms) * 3 / 2
        } else {
            u64::from(ms)
        }
    }

    pub(super) fn duration_ms(self) -> u64 {
        (self.first..self.frames.len())
            .map(|index| self.frame_ms(index))
            .sum()
    }

    pub(super) fn frame_index(self, mut elapsed_ms: u64) -> usize {
        for index in self.first..self.frames.len() {
            let duration = self.frame_ms(index);
            if elapsed_ms < duration {
                return index;
            }
            elapsed_ms -= duration;
        }
        self.frames.len() - 1
    }
}

/// Routes form a tree, with Uncrossed between the three accessories and Crossed.
fn route_step(from: Pose, to: Pose) -> Option<&'static Clip> {
    use Pose::*;
    Some(clip(match (from, to) {
        (from, to) if from == to => return None,
        (Crossed, Reading) => "Transitions_up/book-open",
        (Crossed, _) => "Transitions_up/unfolding-arms",
        (Uncrossed, Headset) => "Transitions_up/headset-on",
        (Uncrossed, Typing) => "typing.enter",
        (Uncrossed, _) => "Transitions_up/crossing-arms",
        (Headset, _) => "Transitions_up/headset-off",
        (Reading, _) => "Transitions_up/book-close",
        (Typing, _) => "typing.exit",
    }))
}

pub(super) struct Sequencer {
    rng: SoftRng,
    queue: VecDeque<Request>,
    pose: Pose,
    mode: IdleMode,
    window_sound: bool,
    return_pose: Option<Pose>,
    sound_intro: bool,
    sound_entry_after_intro: bool,
    idle_at_ms: Option<u64>,
    previous: Option<End>,
    idle_draws: u16,
    last_idle: Option<usize>,
    tired_draws: u16,
    last_tired: Option<usize>,
}

impl Sequencer {
    pub(super) fn new(rng: SoftRng) -> Self {
        Self {
            rng,
            queue: VecDeque::new(),
            pose: Pose::Crossed,
            mode: IdleMode::Normal,
            window_sound: false,
            return_pose: None,
            sound_intro: false,
            sound_entry_after_intro: false,
            idle_at_ms: None,
            previous: None,
            idle_draws: 0,
            last_idle: None,
            tired_draws: 0,
            last_tired: None,
        }
    }

    pub(super) fn mode(&self) -> IdleMode {
        self.mode
    }

    pub(super) fn set_mode(&mut self, mode: IdleMode) {
        self.window_sound = false;
        let mode = match mode {
            IdleMode::Talk(moods) if moods.bits() == 0 => IdleMode::Normal,
            mode => mode,
        };
        if self.mode == mode {
            return;
        }
        if let Some(ambient) = mode.ambient_clip() {
            if self.return_pose.is_none() {
                let pose = self.queued_pose();
                self.return_pose = Some(if pose.arms_only() {
                    pose
                } else if self.pose.arms_only() {
                    self.pose
                } else {
                    Pose::Crossed
                });
            }
            self.sound_intro = mode == IdleMode::Sound && self.pose != ambient.entry;
        } else {
            self.sound_intro = false;
        }
        self.queue
            .retain(|request| request.talk == 0 || request.talk & mode.talk_moods() != 0);
        self.sound_entry_after_intro = false;
        self.mode = mode;
        self.idle_at_ms = None;
    }

    pub(super) fn set_window_sound(&mut self, selected: bool) {
        if selected {
            if self.mode != IdleMode::Sound {
                self.set_mode(IdleMode::Sound);
                self.window_sound = true;
            }
        } else if self.window_sound {
            self.set_mode(IdleMode::Normal);
        }
    }

    pub(super) fn shift_deadlines(&mut self, paused_ms: u64) {
        if let Some(deadline) = self.idle_at_ms.as_mut() {
            *deadline = deadline.saturating_add(paused_ms);
        }
    }

    fn queued_pose(&self) -> Pose {
        self.queue
            .back()
            .map_or(self.pose, |request| request.clip.exit)
    }

    pub(super) fn enqueue(&mut self, action: Contextual) -> bool {
        use Contextual::*;
        let pair = matches!(action, Blush | Flirt | Silly | Wave | Drink | Eat);
        if self.queue.len() + if pair { 2 } else { 1 } > QUEUE_CAPACITY {
            return false;
        }
        self.return_pose = None;
        if self.mode.ambient_clip().is_some() {
            self.set_mode(IdleMode::Normal);
        }
        self.idle_at_ms = None;
        let variants = action.variants();
        if matches!(action, Drink | Eat) {
            let selected = clip(variants[self.rng.usize_below(variants.len())]);
            let slow_first = self.rng.bool();
            self.queue.push_back(Request {
                slow: slow_first,
                ..Request::new(selected)
            });
            self.queue.push_back(Request {
                slow: !slow_first,
                ..Request::new(selected)
            });
        } else if pair {
            let first = self.rng.usize_below(variants.len());
            let mut second = self.rng.usize_below(variants.len() - 1);
            if second >= first {
                second += 1;
            }
            self.queue.push_back(Request::new(clip(variants[first])));
            self.queue.push_back(Request::new(clip(variants[second])));
        } else {
            let selected = match action {
                Tired => {
                    let index = rotation(
                        &mut self.rng,
                        variants.len(),
                        &mut self.tired_draws,
                        &mut self.last_tired,
                    );
                    clip(variants[index])
                }
                Angry | Grimace => clip(variants[self.rng.usize_below(variants.len())]),
                _ => {
                    let pose = self.queued_pose();
                    let matching = variants
                        .iter()
                        .map(|id| clip(id))
                        .filter(|clip| clip.entry == pose);
                    let count = matching.clone().count();
                    if count == 0 {
                        clip(variants[self.rng.usize_below(variants.len())])
                    } else {
                        matching.skip(self.rng.usize_below(count)).next().unwrap()
                    }
                }
            };
            self.queue.push_back(Request::new(selected));
        }
        true
    }

    fn talk_request(&mut self, moods: u8) -> Request {
        let mut selected = self.rng.usize_below(moods.count_ones() as usize);
        for (index, variants) in TALK_GROUPS.iter().enumerate() {
            let bit = 1 << index;
            if moods & bit == 0 {
                continue;
            }
            if selected == 0 {
                return Request {
                    talk: bit,
                    ..Request::new(clip(variants[self.rng.usize_below(variants.len())]))
                };
            }
            selected -= 1;
        }
        unreachable!("a checked mood was drawn")
    }

    fn idle_target(&self, from: Pose) -> Option<Pose> {
        let target = self
            .mode
            .ambient_clip()
            .map(|clip| clip.entry)
            .or(self.return_pose);
        if self.sound_intro && target != Some(from) {
            Some(Pose::Uncrossed)
        } else {
            target
        }
    }

    fn transition(&mut self, step: &'static Clip, collapse: bool) -> Playback {
        let variant = if step.id == "Transitions_up/crossing-arms" && self.rng.bool() {
            clip("Transitions_up/neutral-to-crossed")
        } else {
            step
        };
        self.play(Request::new(variant), true, collapse)
    }

    fn play(&mut self, request: Request, automatic: bool, collapse: bool) -> Playback {
        let clip = request.clip;
        let talk = request.talk != 0;
        let first = usize::from(self.previous.is_some_and(|end| {
            end.source == clip.frames[0] && (end.automatic || automatic || (end.talk && talk))
        }));
        let next = self.queue.front();
        let target = if collapse {
            Some(Pose::Crossed)
        } else if clip.id == "Silly_up/cat-mouth" && self.mode == IdleMode::Sound {
            Some(Pose::Headset)
        } else {
            next.map(|request| request.clip.entry)
                .or_else(|| self.idle_target(clip.exit))
        };
        let last = clip.frames[clip.frames.len() - 1];
        let short_end = automatic
            || target
                .and_then(|target| route_step(clip.exit, target))
                .is_some_and(|step| step.frames[0] == last)
            || (talk && next.is_some_and(|next| next.talk != 0 && next.clip.frames[0] == last));
        self.pose = clip.exit;
        self.previous = Some(End {
            source: last,
            automatic,
            talk,
        });
        self.idle_at_ms = None;
        Playback {
            frames: clip.frames,
            durations: clip.durations,
            first,
            short_start: automatic,
            short_end,
            slow: request.slow,
            crossed_for_collapse: false,
        }
    }

    pub(super) fn next(&mut self, now_ms: u64, collapse: bool) -> Playback {
        if collapse {
            self.sound_entry_after_intro = false;
            self.idle_at_ms = None;
            if let Some(step) = route_step(self.pose, Pose::Crossed) {
                return self.transition(step, true);
            }
            self.previous = None;
            return Playback::holding(Pose::Crossed, true);
        }

        // A committed Cat Mouth lead-in completes its headset entry ahead of
        // other requests unless the Sound mode was dismissed in the meantime.
        if self.sound_entry_after_intro
            && self.mode == IdleMode::Sound
            && self.pose == Pose::Uncrossed
        {
            self.sound_entry_after_intro = false;
            return self.transition(clip("Transitions_up/headset-on"), false);
        }

        if let Some(request) = self.queue.front().copied() {
            if let Some(step) = route_step(self.pose, request.clip.entry) {
                return self.transition(step, false);
            }
            self.queue.pop_front();
            return self.play(request, false, false);
        }

        if let Some(target) = self.idle_target(self.pose) {
            if let Some(step) = route_step(self.pose, target) {
                return self.transition(step, false);
            }
            if self.sound_intro && self.pose == Pose::Uncrossed {
                self.sound_intro = false;
                self.sound_entry_after_intro = true;
                return self.play(Request::new(clip("Silly_up/cat-mouth")), false, false);
            }
            self.sound_intro = false;
            if self.mode.ambient_clip().is_none() {
                self.return_pose = None;
            }
        }

        self.previous = None;
        let talk = self.mode.talk_moods();
        let ambient = self.mode.ambient_clip();
        if talk == 0 && ambient.is_none() && !self.pose.arms_only() {
            return Playback::holding(self.pose, false);
        }
        let deadline = *self.idle_at_ms.get_or_insert_with(|| {
            if talk != 0 {
                now_ms + 1500 + self.rng.usize_below(1001) as u64
            } else {
                now_ms + 3000 + self.rng.usize_below(2001) as u64
            }
        });
        if now_ms < deadline {
            return Playback::holding(self.pose, false);
        }
        if let Some(clip) = ambient {
            return self.play(Request::new(clip), false, false);
        }
        if talk != 0 {
            let first = self.talk_request(talk);
            let second = self.talk_request(talk);
            self.queue.extend([first, second]);
        } else {
            let index = rotation(
                &mut self.rng,
                IDLES.len(),
                &mut self.idle_draws,
                &mut self.last_idle,
            );
            self.queue.push_back(Request::new(clip(IDLES[index])));
        }
        self.next(now_ms, false)
    }
}

fn rotation(rng: &mut SoftRng, len: usize, drawn: &mut u16, last: &mut Option<usize>) -> usize {
    let full = (1 << len) - 1;
    if *drawn == full {
        *drawn = 0;
    }
    let mut choices = full & !*drawn;
    if choices.count_ones() > 1 {
        if let Some(last) = *last {
            choices &= !(1 << last);
        }
    }
    let wanted = rng.usize_below(choices.count_ones() as usize);
    let index = (0..len)
        .filter(|index| choices & (1 << index) != 0)
        .nth(wanted)
        .unwrap();
    *drawn |= 1 << index;
    *last = Some(index);
    index
}
