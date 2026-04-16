import time

from machine import Pin  # type: ignore
from neopixel import NeoPixel  # type: ignore


LED_COUNT = 256
LED_PIN = 4
BRIGHTNESS = 16
USE_NEWF = True


strip = NeoPixel(Pin(LED_PIN, Pin.OUT), LED_COUNT)

h = 0
s = 255
r = 0
speed = 0
up = False

invert = False
t = 0
isolated = 1
delay_ms_counter = 0


def scale8(value):
	return (int(value) * BRIGHTNESS) // 255


def color(red, green, blue):
	return (scale8(red), scale8(green), scale8(blue))


def wrap(index):
	return index % LED_COUNT


def hsv_to_rgb(hue, sat, val):
	hue &= 0xFFFF
	sat = max(0, min(255, int(sat)))
	val = max(0, min(255, int(val)))

	if sat == 0:
		return color(val, val, val)

	region = (hue * 6) >> 16
	fraction = ((hue * 6) & 0xFFFF) / 65536.0

	p = int(val * (255 - sat) / 255)
	q = int(val * (255 - int(sat * fraction)) / 255)
	tc = int(val * (255 - int(sat * (1.0 - fraction))) / 255)

	if region == 0:
		return color(val, tc, p)
	if region == 1:
		return color(q, val, p)
	if region == 2:
		return color(p, val, tc)
	if region == 3:
		return color(p, q, val)
	if region == 4:
		return color(tc, p, val)
	return color(val, p, q)


def rainbow(first_hue, repetitions, saturation):
	for index in range(LED_COUNT):
		hue = first_hue + ((index * repetitions * 65536) // LED_COUNT)
		strip[index] = hsv_to_rgb(hue, saturation, 255)


def newf():
	global invert, t, isolated, delay_ms_counter

	position = t % LED_COUNT
	for segment in range(8):
		position = (position + (segment * 32)) % LED_COUNT
		strip[position] = color(255, 55, 255)
		if not invert:
			strip[wrap(position - 1)] = color(100, 20, 100)
			strip[wrap(position - 2)] = color(50, 10, 50)
			strip[wrap(position - 3)] = color(25, 5, 25)
			strip[wrap(position - 4)] = color(0, 0, 0)
		else:
			strip[wrap(position + 1)] = color(100, 20, 100)
			strip[wrap(position + 2)] = color(50, 10, 50)
			strip[wrap(position + 3)] = color(25, 5, 25)
			strip[wrap(position + 4)] = color(0, 0, 0)

	if position == LED_COUNT - 1:
		delay_ms_counter += 1
		isolated += 1

	if isolated == 9:
		isolated = 0
		invert = not invert

	if not invert:
		t += 1
	else:
		t -= 1

	time.sleep_ms(delay_ms_counter)
	strip.write()


def loop():
	global h, s, r, speed, up

	if USE_NEWF:
		newf()
		return

	rainbow(h, r, s)
	strip.write()
	h += 64 * speed

	if not up:
		if s == 0:
			up = True
			r += 1
			speed += 4
			if r > 8:
				r = 0
				speed = 0
		else:
			s -= 1
	else:
		if s == 255:
			up = False
		else:
			s += 1


while True:
	loop()