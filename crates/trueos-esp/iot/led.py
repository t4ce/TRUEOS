import math
import time

from machine import Pin  # type: ignore
from neopixel import NeoPixel  # type: ignore


LED_PIN = 8
LED_COUNT = 64
FADE_STEPS = 256
FRAME_DELAY_MS = 20


strip = NeoPixel(Pin(LED_PIN, Pin.OUT), LED_COUNT)
red_blue_lut = bytearray(FADE_STEPS)
green_lut = bytearray(FADE_STEPS)


def build_fade_tables():
	for index in range(FADE_STEPS):
		theta = (2.0 * math.pi * index) / FADE_STEPS
		phase = 0.5 * (1.0 + math.sin(theta))
		red_blue_lut[index] = int(255.0 * phase)
		green_lut[index] = int(55.0 * phase)


def main():
	build_fade_tables()
	fade_index = 0

	while True:
		strip[0] = (
			red_blue_lut[fade_index],
			green_lut[fade_index],
			red_blue_lut[fade_index],
		)
		strip.write()

		fade_index = (fade_index + 1) & (FADE_STEPS - 1)
		time.sleep_ms(FRAME_DELAY_MS)


main()
