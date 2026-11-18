use crate::r::services::mouse_motion_service as motion;
use crate::usb2::hid::{
    HID_CURSOR_EVENT_FLAG_DEVICE_LOST, HID_CURSOR_EVENT_FLAG_POINTER_GROUP_RETIRED,
    HID_CURSOR_EVENT_FLAG_VISUAL_ONLY, TrueosHidCursorEvent, take_cursor_events,
};
use alloc::vec::Vec;

#[test]
fn ten_moving_marker_cursors_stay_visible() {
    check_moving_markers(10);
}

#[test]
fn sixteen_moving_marker_cursors_stay_visible() {
    check_moving_markers(16);
}

fn check_moving_markers(count: usize) {
    let owner = motion::MouseControlPrincipal::KernelApp(0x53);
    let text_color = Rgba8::new(0, 0, 0, 255);
    let running_color = Rgba8::new(32, 128, 255, 255);
    let (width, height) = output_dimensions().unwrap();
    crate::microfont_mask_service::warm_fixture_atlas();
    let defaults = microflake::Settings::default();
    let settings = microflake::Settings {
        motif: microflake::Motif::Flower,
        palette: microflake::Palette::Cobalt,
        seed: "moving-markers",
        background_alpha: 0,
        ..defaults
    };
    let mut tile = [0; microflake::TILE_BYTES];
    microflake::render_tile(&settings, 0, &mut tile);
    let mut cursors = Vec::new();
    for index in 0..count {
        let nickname = format!("§Pilot {}", char::from(b'A' + index as u8));
        let cursor = motion::request_motif_cursor(
            owner,
            "Spirit/agent",
            &nickname,
            &tile,
            running_color,
            text_color,
        )
        .unwrap();
        let y = 30 + 9 * index as i32;
        motion::submit_program(
            owner,
            cursor.handle,
            &[
                motion::MouseControlCommand {
                    opcode: motion::MOUSE_CONTROL_OPCODE_TELEPORT,
                    flags: motion::MOUSE_CONTROL_FLAG_CLEAR_QUEUE,
                    x: 120,
                    y,
                    ..motion::MouseControlCommand::default()
                },
                motion::MouseControlCommand {
                    opcode: motion::MOUSE_CONTROL_OPCODE_STROKE,
                    path: motion::MOUSE_CONTROL_PATH_CUBIC,
                    easing: motion::MOUSE_CONTROL_EASING_LINEAR,
                    duration_ms: 40,
                    x: 168,
                    y: y + 8,
                    control1_x: 134,
                    control1_y: y - 4,
                    control2_x: 150,
                    control2_y: y + 4,
                    ..motion::MouseControlCommand::default()
                },
                motion::MouseControlCommand {
                    opcode: motion::MOUSE_CONTROL_OPCODE_STROKE,
                    path: motion::MOUSE_CONTROL_PATH_CUBIC,
                    easing: motion::MOUSE_CONTROL_EASING_LINEAR,
                    duration_ms: 48,
                    x: 200,
                    y,
                    control1_x: 185,
                    control1_y: y + 12,
                    control2_x: 194,
                    control2_y: y + 4,
                    ..motion::MouseControlCommand::default()
                },
            ],
        )
        .unwrap();
        cursors.push((cursor, nickname));
    }
    assert!(take_cursor_events().is_empty());

    let mut broker = InputBroker::new();
    let mut sequence = 0;
    let mut rendered_positions = vec![Vec::new(); count];
    let mut previous_rects = Vec::new();
    for now_ms in (0..=112).step_by(8) {
        if now_ms == 32 {
            microflake::render_tile(&settings, 3, &mut tile);
            for (cursor, nickname) in &cursors {
                motion::update_motif_cursor(owner, cursor.handle, nickname, &tile).unwrap();
            }
        }
        motion::fixture_tick(now_ms);
        let events = take_cursor_events();
        if now_ms <= 40 {
            assert_eq!(
                events.len(),
                count,
                "each cursor emits a report at {now_ms}ms"
            );
        }
        for event in events {
            assert_ne!(event.flags & HID_CURSOR_EVENT_FLAG_VISUAL_ONLY, 0);
            sequence += 1;
            broker.process_cursor(event, sequence);
            assert!(
                broker
                    .cursors
                    .iter()
                    .any(|route| route.source.slot_id == event.slot_id
                        && route.visible_after_motion
                        && route.visual_only),
                "broker lost slot {} at {now_ms}ms with flags {:#x}",
                event.slot_id,
                event.flags
            );
        }
        assert_eq!(broker.cursors.len(), count, "visible cursors at {now_ms}ms");
        let mut current_rects = Vec::new();
        for (index, (cursor, nickname)) in cursors.iter().enumerate() {
            let route = broker
                .cursors
                .iter()
                .find(|route| route.source.slot_id == cursor.slot_id)
                .unwrap();
            let expected_position = motion::cursor_position(owner, cursor.handle).unwrap();
            assert_eq!((route.x as i32, route.y as i32), expected_position);
            let visual = motion::cursor_visual_metadata(cursor.slot_id).unwrap();
            assert!(visual.visible && visual.visual_only);
            let glyph = visual.glyph.as_ref().unwrap();
            assert_eq!(glyph.text.as_ref(), nickname);
            assert_eq!(usize::from(glyph.prefix_width), microflake::TILE_SIZE);
            assert_eq!(usize::from(glyph.height), microflake::TILE_SIZE + 1);
            let rects = crate::render::draw_test_glyph(
                route.x,
                route.y,
                glyph,
                visual.color.unwrap(),
                visual.text_color.unwrap(),
                width,
                height,
            );
            let origin_x = route.x - u32::from(glyph.width / 2);
            let motif_end_x = origin_x + microflake::TILE_SIZE as u32;
            assert!(rects.iter().any(|rect| rect.x < motif_end_x
                && rect.color != running_color
                && rect.color != text_color));
            assert!(rects.iter().any(|rect| rect.x >= motif_end_x));
            assert!(rects.iter().all(|rect| rect.x >= origin_x
                && rect.x + rect.width <= origin_x + u32::from(glyph.width)));
            if rendered_positions[index].last() != Some(&expected_position) {
                rendered_positions[index].push(expected_position);
            }
            current_rects.push(rects);
        }
        check_slot4_damage(&previous_rects, &current_rects, width, height);
        previous_rects = current_rects;
    }
    for (index, positions) in rendered_positions.iter().enumerate() {
        assert!(
            positions.len() >= 10,
            "label {index} did not move repeatedly: {positions:?}"
        );
        assert_eq!(positions.last(), Some(&(200, 30 + 9 * index as i32)));
    }

    // The broker must still recognize real lifecycle reports once motion ends.
    for ((cursor, _), flag) in cursors.iter().zip([
        HID_CURSOR_EVENT_FLAG_POINTER_GROUP_RETIRED,
        HID_CURSOR_EVENT_FLAG_DEVICE_LOST,
    ]) {
        sequence += 1;
        broker.process_cursor(
            TrueosHidCursorEvent {
                flags: flag,
                slot_id: cursor.slot_id,
                hid_kind: crate::r::cursor::HID_KIND_VIRTUAL_CURSOR,
                ..TrueosHidCursorEvent::default()
            },
            sequence,
        );
        assert!(
            broker
                .cursors
                .iter()
                .all(|route| route.source.slot_id != cursor.slot_id)
        );
    }
    assert_eq!(broker.cursors.len(), count - 2);
    for (cursor, _) in cursors {
        motion::release_cursor(owner, cursor.handle).unwrap();
    }
}

fn check_slot4_damage(
    previous: &[Vec<crate::intel::LiveOverlayRect>],
    current: &[Vec<crate::intel::LiveOverlayRect>],
    width: u32,
    height: u32,
) {
    let render = |groups: &[Vec<crate::intel::LiveOverlayRect>]| {
        let mut pixels = vec![[0u8; 4]; (width * height) as usize];
        for rect in groups.iter().flatten() {
            let alpha = u16::from(rect.color.a);
            let source = [rect.color.r, rect.color.g, rect.color.b, 255];
            for y in rect.y..rect.y + rect.height {
                for x in rect.x..rect.x + rect.width {
                    let dest = &mut pixels[(y * width + x) as usize];
                    for (source, dest) in source.into_iter().zip(dest) {
                        *dest =
                            ((u16::from(source) * alpha + u16::from(*dest) * (255 - alpha) + 127)
                                / 255) as u8;
                    }
                }
            }
        }
        pixels
    };
    let damage = crate::render::glyph_damage(previous, current);
    let (old, new) = (render(previous), render(current));
    for (index, (old, new)) in old.iter().zip(&new).enumerate() {
        if old != new {
            let (x, y) = (index as u32 % width, index as u32 / width);
            assert!(
                damage.rects().iter().any(|rect| {
                    x >= rect.x
                        && x - rect.x < rect.width
                        && y >= rect.y
                        && y - rect.y < rect.height
                }),
                "Slot 4 would skip a changed pixel at {x},{y}"
            );
        }
    }
}
