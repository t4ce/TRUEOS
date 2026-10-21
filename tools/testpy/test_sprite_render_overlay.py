"""Host regression for retained-render -> sprite-overlay handoff (no GPU)."""
import subprocess
from test_clip_position3_uv_texture import ROOT, item

def main():
    path = "src/ui4/blueprint_ui4.rs"
    finish = item(path, "finish_sprite_scene")
    # Publication must remain single-producer. The final matching overlay
    # receipt supersedes the already-retired render receipt, never before it.
    start = finish.index("let Some(final_release) = final_release")
    tail = finish[start:]
    assert tail.index("final_release.matches(destination.phys, destination.bytes)") < tail.index("surface.pending_gpu_release = Some(final_release)")
    assert tail.index("surface.pending_gpu_release = Some(final_release)") < tail.index("surface.pending_render_release = None")
    assert "!render_overlay" in finish
    assert "sprite_scene_needs_clear(render_overlay, full_frame_copy)" in finish
    assert "release.matches(destination.phys, destination.bytes)" in finish
    assert "surface.gpu_submission_unretired" in finish
    # The full sprite harness also exercises the shared clear/overlay policy.
    subprocess.run(["python3", str(ROOT / "tools/testpy/test_ui4_bcs_sprite_copy.py")], check=True)

if __name__ == "__main__":
    main()
