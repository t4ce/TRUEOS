#[embassy_executor::task]
pub async fn ui2_text_scene_demo_task() {
    let window_id = crate::r::ui2::create_text_scene_window(
        "SceneCmd Text Demo",
        crate::r::ui2::Ui2Rect {
            x: 92.0,
            y: 118.0,
            w: 468.0,
            h: 260.0,
        },
        34,
        255,
    );
    let _ = crate::r::ui2::set_window_left_scrollbar_visible(window_id, false);
    let _ = crate::r::ui2::set_window_bottom_scrollbar_visible(window_id, false);

    let _ = crate::r::ui2::ui2_text_scene_try_send(crate::r::ui2::Ui2TextSceneCmd::Clear);
    let rows = [
        "row 01: semantic SceneCmd channel",
        "row 02: UI2 drains commands first",
        "row 03: retained rows become scene state",
        "row 04: compositor owns final frame",
        "row 05: no raw GPU command escape hatch",
        "row 06: backpressure stays bounded",
        "row 07: one consumer keeps ordering simple",
        "row 08: draw happens only during compose",
        "row 09: rows stay visible across frames",
        "row 10: UI2 keeps final veto",
    ];
    for (index, text) in rows.iter().enumerate() {
        let row = crate::r::ui2::Ui2TextSceneRow::new(text, 255);
        if !crate::r::ui2::ui2_text_scene_try_send(crate::r::ui2::Ui2TextSceneCmd::SetRow {
            index: index as u8,
            row,
        }) {
            crate::log!(
                "ui2-text-scene-demo: drop window={} index={}\n",
                window_id,
                index
            );
            break;
        }
    }

    crate::log!(
        "ui2-text-scene-demo: window={} queued_rows={}\n",
        window_id,
        rows.len()
    );
}
