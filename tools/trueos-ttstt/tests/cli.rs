use std::process::Command;

fn trueos_ttstt() -> Command {
    Command::new(env!("CARGO_BIN_EXE_trueos-ttstt"))
}

#[test]
fn top_level_help_describes_both_directions() {
    let output = trueos_ttstt().arg("--help").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("tts"));
    assert!(stdout.contains("stt"));
    assert!(stdout.contains("listen"));
    assert!(stdout.contains("speak"));
    assert!(stdout.contains("stream"));
    assert!(stdout.contains("transcribe"));
}

#[test]
fn version_is_available() {
    let output = trueos_ttstt().arg("--version").output().unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "trueos-ttstt 0.1.0\n"
    );
}

#[test]
fn tts_help_describes_playback_and_optional_file_output() {
    let output = trueos_ttstt().args(["tts", "--help"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("played by default"));
    assert!(stdout.contains("Write a WAV file instead of playing"));
    assert!(!stdout.contains("default: speech.wav"));
}

#[test]
fn stt_help_describes_file_and_continuous_modes() {
    let output = trueos_ttstt().args(["stt", "--help"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("omit it or use - for continuous raw PCM"));
    assert!(stdout.contains("jsonl"));
}

#[test]
fn continuous_stt_rejects_non_streaming_formats_before_loading_a_model() {
    let output = trueos_ttstt()
        .args(["--quiet", "stt", "--format", "srt"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("continuous STT supports --format text or --format jsonl")
    );
}

#[test]
fn missing_tts_model_has_an_actionable_error() {
    let output = trueos_ttstt()
        .args([
            "--quiet",
            "tts",
            "--model-dir",
            "this-model-does-not-exist",
            "hello",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("Kokoro model directory"));
    assert!(stderr.contains("README.md"));
}

#[test]
fn missing_stt_audio_is_reported_before_the_model() {
    let output = trueos_ttstt()
        .args(["--quiet", "stt", "missing.wav"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("audio file missing.wav does not exist")
    );
}

#[test]
fn paths_uses_the_configured_project_state_directory() {
    let output = trueos_ttstt()
        .arg("paths")
        .env("TTSTT_HOME", "/tmp/ttstt-test-state")
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("/tmp/ttstt-test-state/models/kokoro"));
    assert!(stdout.contains("/tmp/ttstt-test-state/models/whisper/ggml-base.bin"));
}
