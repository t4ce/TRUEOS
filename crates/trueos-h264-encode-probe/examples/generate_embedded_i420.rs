use std::{env, fs, path::PathBuf};

const WIDTH: usize = 512;
const HEIGHT: usize = 512;
const FRAME_COUNT: usize = 30;

const SCENARIO: [u8; FRAME_COUNT] = [
    0, 11, 22, 33, 44, 55, 6, 17, 28, 39, 50, 1, 12, 23, 34, 45, 56, 7, 18, 29, 40, 51, 2, 13, 24,
    35, 46, 57, 8, 19,
];

const RGB_SPECTRUM: [[u8; 3]; 16] = [
    [255, 255, 255],
    [255, 255, 0],
    [128, 255, 0],
    [0, 255, 0],
    [0, 255, 128],
    [0, 255, 255],
    [0, 128, 255],
    [0, 0, 255],
    [128, 0, 255],
    [255, 0, 255],
    [255, 0, 128],
    [255, 0, 0],
    [255, 128, 0],
    [128, 128, 128],
    [32, 32, 32],
    [0, 0, 0],
];

fn main() {
    let output = env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .expect("usage: generate_embedded_i420 <output.bin>");
    let mut sequence = Vec::with_capacity(WIDTH * HEIGHT * 3 / 2 * FRAME_COUNT);
    for frame_index in 0..FRAME_COUNT {
        append_frame(&mut sequence, frame_index);
    }
    assert_eq!(sequence.len(), 11_796_480);
    fs::write(&output, &sequence).expect("write embedded I420 asset");
    eprintln!(
        "wrote {} bytes ({} 512x512 I420 frames) to {}",
        sequence.len(),
        FRAME_COUNT,
        output.display()
    );
}

fn append_frame(output: &mut Vec<u8>, frame_index: usize) {
    let chroma_width = WIDTH / 2;
    let chroma_height = HEIGHT / 2;
    let mut y = vec![16; WIDTH * HEIGHT];
    let mut cb = vec![128; chroma_width * chroma_height];
    let mut cr = vec![128; chroma_width * chroma_height];
    let scenario = usize::from(SCENARIO[frame_index]);
    let phase = (scenario + frame_index) % RGB_SPECTRUM.len();
    let motion_x = (scenario * 7 + frame_index * 5) % (WIDTH - 64);
    let motion_y = (scenario * 3 + frame_index * 7) % (HEIGHT - 64);

    for chroma_y in 0..chroma_height {
        let pixel_y = chroma_y * 2;
        for chroma_x in 0..chroma_width {
            let pixel_x = chroma_x * 2;
            let in_motion = pixel_x >= motion_x
                && pixel_x < motion_x + 64
                && pixel_y >= motion_y
                && pixel_y < motion_y + 64;
            let rgb = if in_motion {
                let checker = ((pixel_x - motion_x) / 4 + (pixel_y - motion_y) / 4) & 1;
                if checker == 0 {
                    RGB_SPECTRUM[(phase + frame_index / 4) % RGB_SPECTRUM.len()]
                } else {
                    RGB_SPECTRUM[15]
                }
            } else if pixel_y < 192 {
                let bar = pixel_x * RGB_SPECTRUM.len() / WIDTH;
                RGB_SPECTRUM[(bar + phase) % RGB_SPECTRUM.len()]
            } else if pixel_y < 320 {
                let ramp = ((pixel_x * 255) / (WIDTH - 1)) as u8;
                [ramp, ramp, ramp]
            } else {
                let tile = (pixel_x / 8) + (pixel_y / 8) + phase;
                RGB_SPECTRUM[tile % RGB_SPECTRUM.len()]
            };
            let (cell_y, cell_cb, cell_cr) = rgb_to_limited_ycbcr(rgb);
            for row in pixel_y..pixel_y + 2 {
                let row_start = row * WIDTH;
                y[row_start + pixel_x] = cell_y;
                y[row_start + pixel_x + 1] = cell_y;
            }
            let chroma_offset = chroma_y * chroma_width + chroma_x;
            cb[chroma_offset] = cell_cb;
            cr[chroma_offset] = cell_cr;
        }
    }

    let cross_x = (WIDTH / 2 + frame_index) % WIDTH;
    let cross_y = (HEIGHT / 2 + frame_index * 2) % HEIGHT;
    for row in 0..HEIGHT {
        y[row * WIDTH + cross_x] = if row & 1 == 0 { 235 } else { 16 };
    }
    for col in 0..WIDTH {
        y[cross_y * WIDTH + col] = if col & 1 == 0 { 235 } else { 16 };
    }

    output.extend_from_slice(&y);
    output.extend_from_slice(&cb);
    output.extend_from_slice(&cr);
}

fn rgb_to_limited_ycbcr([red, green, blue]: [u8; 3]) -> (u8, u8, u8) {
    let red = i32::from(red);
    let green = i32::from(green);
    let blue = i32::from(blue);
    let y = 16 + ((66 * red + 129 * green + 25 * blue + 128) >> 8);
    let cb = 128 + ((-38 * red - 74 * green + 112 * blue + 128) >> 8);
    let cr = 128 + ((112 * red - 94 * green - 18 * blue + 128) >> 8);
    (y.clamp(16, 235) as u8, cb.clamp(16, 240) as u8, cr.clamp(16, 240) as u8)
}
