use crate::v;

const DEMO_SVG: &[u8] = b"<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 10 10\"><circle cx=\"5\" cy=\"5\" r=\"5\" fill=\"red\"/></svg>";

pub fn start() {
    v::vsys::log_info("VMDEMO: begin\n");

    if v::vclock::ntp_current_unix_seconds() != 0 {
        v::vsys::log_info("VMDEMO: ntp ok\n");
    } else {
        v::vsys::log_error("VMDEMO: ntp zero\n");
    }

    let rc = v::vgfx::upload_svg_to_texture_async(1, DEMO_SVG);
    if rc == 0 {
        v::vsys::log_info("VMDEMO: svg queued\n");
    } else {
        v::vsys::log_error("VMDEMO: svg queue failed\n");
    }

    let status = v::vgfx::texture_status(1);
    if status >= 0 {
        v::vsys::log_info("VMDEMO: tex status read\n");
    } else {
        v::vsys::log_error("VMDEMO: tex status failed\n");
    }

    v::vsys::log_info("VMDEMO: end\n");
}
