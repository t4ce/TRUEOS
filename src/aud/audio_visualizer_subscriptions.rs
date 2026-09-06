// Separate the legacy preview switch from window-owned subscriptions. All tap
// transitions occur under the same lock; stopping a preview cannot silence a
// different window. FFT allocation stays lazy and outside the HDA callback.
#[derive(Default)]
struct AudioVisualizerUsers {
    preview: bool,
    windows: usize,
}

impl AudioVisualizerUsers {
    fn enabled(&self) -> bool {
        self.preview || self.windows != 0
    }
}

static AUDIO_VISUALIZER_USERS: Mutex<AudioVisualizerUsers> = Mutex::new(AudioVisualizerUsers {
    preview: false,
    windows: 0,
});

pub(crate) fn set_enabled(enabled: bool) {
    let mut users = AUDIO_VISUALIZER_USERS.lock();
    users.preview = enabled;
    audio_visualizer_tap::set_enabled(users.enabled());
}

/// Held by exactly one live audio selection; dropping the last user stops the tap.
pub(crate) struct AudioVisualizerSubscription {
    _private: (),
}

impl AudioVisualizerSubscription {
    pub(crate) fn acquire() -> Self {
        let mut users = AUDIO_VISUALIZER_USERS.lock();
        users.windows += 1;
        audio_visualizer_tap::set_enabled(true);
        Self { _private: () }
    }
}

impl Drop for AudioVisualizerSubscription {
    fn drop(&mut self) {
        let mut users = AUDIO_VISUALIZER_USERS.lock();
        users.windows -= 1;
        audio_visualizer_tap::set_enabled(users.enabled());
    }
}
