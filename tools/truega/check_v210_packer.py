"""Compare the simulator output with the input pattern after FFmpeg decoding."""

import array
import pathlib
import subprocess
import sys

WIDTH, HEIGHT = 3840, 2160
PAIRS = WIDTH * HEIGHT // 2


def main(path):
    expected_size = WIDTH * HEIGHT * 8 // 3
    if path.stat().st_size != expected_size:
        raise SystemExit(f"Expected one {expected_size}-byte v210 frame")

    raw = subprocess.check_output([
        "ffmpeg", "-nostdin", "-v", "error", "-xerror",
        "-f", "v210", "-video_size", f"{WIDTH}x{HEIGHT}", "-i", str(path),
        "-frames:v", "1", "-pix_fmt", "yuv422p10le", "-f", "rawvideo", "pipe:1",
    ])
    samples = array.array("H", raw)
    if sys.byteorder == "big":
        samples.byteswap()
    if len(samples) != PAIRS * 4:
        raise SystemExit(f"Unexpected sample count: {len(samples)}")

    even_y = [64 + i for i in range(877)]
    odd_y = [64 + (i * 3 + 13) % 877 for i in range(877)]
    cb = [64 + (i * 5 + 27) % 897 for i in range(897)]
    cr = [64 + (i * 7 + 53) % 897 for i in range(897)]
    for i in range(PAIRS):
        actual = (samples[2 * i], samples[2 * i + 1],
                  samples[2 * PAIRS + i], samples[3 * PAIRS + i])
        expected = (even_y[i % 877], odd_y[i % 877], cb[i % 897], cr[i % 897])
        if actual != expected:
            raise SystemExit(f"Pair {i}: decoded {actual}, expected {expected}")
    print(f"Verified {len(samples):,} 10-bit samples in one {WIDTH}x{HEIGHT} frame")


if __name__ == "__main__":
    main(pathlib.Path(sys.argv[1]))
