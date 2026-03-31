import math
import random
import time

from machine import Pin  # type: ignore
from neopixel import NeoPixel  # type: ignore


LED_PIN = 8
LED_COUNT = 64
DEFAULT_BRIGHTNESS = 64
FRAME_DELAY_MS = 10


class Strobe:
	def __init__(self):
		self.active = False
		self.until = 0


class Bouncer:
	def __init__(self):
		self.t = 0.0
		self.v = 0.6
		self.dir = 1
		self.seg = 0
		self.strobe = Strobe()


class AwesomeDots:
	CFG_ROTATORS = 9
	CFG_BOUNCERS = 6
	CFG_STACK = 3
	CFG_SPACING = 1.0

	CFG_BASE_SPEED = 35.0
	CFG_LFO_DEPTH = 0.75
	CFG_LFO_HZ = 0.13

	CFG_STROBE_CHANCE_PER_SEC = 0.60
	CFG_STROBE_MS_MIN = 30
	CFG_STROBE_MS_MAX = 140

	def __init__(self, leds, num_leds):
		self.leds = leds
		self.n = num_leds
		self.inited = False
		self.last_ms = 0
		self.t_sec = 0.0
		self.rot_phase = 0.0
		self.brightness = 255
		self.use_override_speed = False
		self.base_speed_override = self.CFG_BASE_SPEED

		self.bouncers = [Bouncer() for _ in range(self.CFG_BOUNCERS)]
		self.rot_strobes = [Strobe() for _ in range(self.CFG_ROTATORS)]
		self.rot_pos = [0 for _ in range(self.CFG_ROTATORS)]

	def begin(self, brightness=DEFAULT_BRIGHTNESS):
		self.brightness = max(0, min(255, int(brightness)))
		seed_value = time.ticks_us() if hasattr(time, "ticks_us") else 0
		random.seed(seed_value)

		for index in range(self.CFG_BOUNCERS):
			bouncer = self.bouncers[index]
			bouncer.t = self.frand()
			bouncer.v = 0.35 + 0.9 * self.frand()
			bouncer.dir = 1 if random.getrandbits(1) == 0 else -1
			bouncer.seg = random.randrange(self.CFG_ROTATORS)
			bouncer.strobe.active = False
			bouncer.strobe.until = 0

		for strobe in self.rot_strobes:
			strobe.active = False
			strobe.until = 0

		self.last_ms = self.now_ms()
		self.t_sec = 0.0
		self.rot_phase = 0.0
		self.inited = True

	def tick(self):
		if not self.inited:
			self.begin()

		now_ms = self.now_ms()
		dt_ms = self.ticks_diff(now_ms, self.last_ms)
		self.last_ms = now_ms
		if dt_ms < 0:
			dt_ms = 0
		if dt_ms > 100:
			dt_ms = 100
		dt = dt_ms / 1000.0

		self.t_sec += dt
		self.clear_all()

		speed_mul = self.speed_lfo(self.t_sec)
		rot_speed = self.current_base_speed() * speed_mul

		self.rot_phase += rot_speed * dt
		self.rot_phase %= float(self.n)

		self.compute_rotators(self.rot_pos, self.CFG_ROTATORS)

		for index in range(self.CFG_ROTATORS):
			strobe = self.rot_strobes[index]
			self.maybe_strobe(strobe, now_ms, dt)
			color = self.rgb(80, 80, 255) if strobe.active else self.rgb(0, 0, 255)
			self.draw_stacked_dot(self.rot_pos[index], color)

		for bouncer in self.bouncers:
			velocity = bouncer.v * speed_mul
			bouncer.t += bouncer.dir * velocity * dt

			if bouncer.t >= 1.0:
				bouncer.t = 1.0
				bouncer.dir = -1
			if bouncer.t <= 0.0:
				bouncer.t = 0.0
				bouncer.dir = 1

			self.maybe_strobe(bouncer.strobe, now_ms, dt)

			s0 = bouncer.seg % self.CFG_ROTATORS
			s1 = (bouncer.seg + 1) % self.CFG_ROTATORS

			p0 = self.rot_pos[s0]
			p1 = self.rot_pos[s1]

			gap = self.forward_dist(p0, p1)
			margin = max(1, self.CFG_STACK)
			usable = gap - (2 * margin)
			if usable < 1:
				usable = 1

			offset = margin + int(math.floor(bouncer.t * usable))
			pos = self.wrap_index(p0 + offset)

			color = self.rgb(255, 200, 80) if bouncer.strobe.active else self.rgb(255, 0, 0)
			self.draw_stacked_dot(pos, color)

		self.leds.write()

	def set_speed(self, pixels_per_sec):
		self.base_speed_override = float(pixels_per_sec)
		self.use_override_speed = True

	def clear_speed_override(self):
		self.use_override_speed = False

	def current_base_speed(self):
		if self.use_override_speed:
			return self.base_speed_override
		return self.CFG_BASE_SPEED

	def frand(self):
		return random.getrandbits(30) / float(1 << 30)

	def rgb(self, red, green, blue):
		if self.brightness >= 255:
			return (int(red), int(green), int(blue))
		return (
			(int(red) * self.brightness) // 255,
			(int(green) * self.brightness) // 255,
			(int(blue) * self.brightness) // 255,
		)

	def wrap_index(self, index):
		return index % self.n

	def clear_all(self):
		for index in range(self.n):
			self.leds[index] = (0, 0, 0)

	def speed_lfo(self, seconds):
		x = math.sin(2.0 * math.pi * self.CFG_LFO_HZ * seconds)
		return 1.0 + (self.CFG_LFO_DEPTH * x)

	def maybe_strobe(self, strobe, now_ms, dt_sec):
		if strobe.active:
			if self.ticks_diff(now_ms, strobe.until) >= 0:
				strobe.active = False
			return

		chance = self.CFG_STROBE_CHANCE_PER_SEC * dt_sec
		if self.frand() < chance:
			strobe.active = True
			duration = random.randint(self.CFG_STROBE_MS_MIN, self.CFG_STROBE_MS_MAX)
			strobe.until = self.ticks_add(now_ms, duration)

	def draw_stacked_dot(self, center, color):
		self.leds[self.wrap_index(center)] = color
		for offset in range(1, self.CFG_STACK):
			dim = 255 // (offset + 1)
			side_color = self.rgb(0, 0, dim)
			self.leds[self.wrap_index(center + offset)] = side_color
			self.leds[self.wrap_index(center - offset)] = side_color

	def forward_dist(self, start, end):
		distance = end - start
		if distance < 0:
			distance += self.n
		return distance

	def compute_rotators(self, out_pos, count):
		base = self.n / float(count)
		spacing = base * self.CFG_SPACING
		phase = self.rot_phase

		for index in range(count):
			position = phase + (index * spacing)
			out_pos[index] = self.wrap_index(int(math.floor(position)))

	def now_ms(self):
		if hasattr(time, "ticks_ms"):
			return time.ticks_ms()
		return int(time.time() * 1000)

	def ticks_add(self, value, delta):
		if hasattr(time, "ticks_add"):
			return time.ticks_add(value, delta)
		return value + delta

	def ticks_diff(self, newer, older):
		if hasattr(time, "ticks_diff"):
			return time.ticks_diff(newer, older)
		return newer - older


def main():
	strip = NeoPixel(Pin(LED_PIN, Pin.OUT), LED_COUNT)
	effect = AwesomeDots(strip, LED_COUNT)
	effect.begin(DEFAULT_BRIGHTNESS)

	while True:
		effect.tick()
		time.sleep_ms(FRAME_DELAY_MS)


main()