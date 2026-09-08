"""Host regression for retained-render -> sprite-overlay handoff (no GPU)."""
import subprocess
import tempfile
from pathlib import Path
from test_clip_position3_uv_texture import ROOT, item

def main():
    path = "src/ui4/blueprint_text.rs"
    finish = item(path, "finish_sprite_scene")
    # Publication must remain single-producer. The final matching overlay
    # receipt supersedes the already-retired render receipt, never before it.
    start = finish.index("let Some(final_release) = final_release")
    tail = finish[start:]
    assert tail.index("final_release.matches(destination.phys, destination.bytes)") < tail.index("surface.pending_gpu_release = Some(final_release)")
    assert tail.index("surface.pending_gpu_release = Some(final_release)") < tail.index("surface.pending_render_release = None")
    assert "(!render_overlay)" in finish
    assert "sprite_scene_needs_clear(render_overlay, full_frame_copy)" in finish
    assert "release.matches(destination.phys, destination.bytes)" in finish
    assert "surface.gpu_submission_unretired" in finish
    source = item(path, "sprite_scene_needs_clear") + "\n" + item(path, "sprite_overlay_tests")
    with tempfile.TemporaryDirectory(prefix="trueos-sprite-overlay-") as temporary:
        test = Path(temporary)/"tests.rs"
        test.write_text(source)
        executable = Path(temporary)/"tests"
        subprocess.run(["rustc","--edition=2024","--test",str(test),"-o",str(executable)],check=True,cwd=ROOT)
        subprocess.run([str(executable)],check=True)

if __name__ == "__main__":
    main()
