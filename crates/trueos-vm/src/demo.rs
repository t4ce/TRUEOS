use crate::v;

pub fn start() {
    v::vsys::log_info("VMDEMO: begin\n");

    if v::vclock::ntp_current_unix_seconds() != 0 {
        v::vsys::log_info("VMDEMO: ntp ok\n");
    } else {
        v::vsys::log_error("VMDEMO: ntp zero\n");
    }

    // Probe SVG upload ABI entry without allocating: kernel should reject null/empty with -3.
    let rc = v::vgfx::probe_upload_svg_to_texture_async(1);
    if rc == -2 || rc == -3 {
        v::vsys::log_info("VMDEMO: svg abi ok\n");
    } else {
        v::vsys::log_error("VMDEMO: svg abi fail\n");
    }

    v::vsys::log_info("VMDEMO: end\n");
}
