import time

from machine import I2C, Pin  # type: ignore
from neopixel import NeoPixel  # type: ignore


LED_PIN = 0
LED_COUNT = 256
BRIGHTNESS = 20

I2C_FREQ = 400000
I2C_CANDIDATES = (
	(0, 1, 2),
	(0, 2, 1),
	(1, 1, 2),
	(1, 2, 1),
)
MPR121_ADDR = 0x5A

TOUCH_THRESHOLD = 16
RELEASE_THRESHOLD = 8
POLL_MS = 12

MPR121_TOUCH_STATUS_L = 0x00
MPR121_SOFTRESET = 0x80
MPR121_ECR = 0x5E
MPR121_DEBOUNCE = 0x5B
MPR121_CONFIG1 = 0x5C
MPR121_CONFIG2 = 0x5D

KEY_COUNT = 12

KEY_COLORS = (
	(255, 48, 48),
	(255, 96, 24),
	(255, 160, 0),
	(220, 220, 0),
	(96, 255, 32),
	(0, 255, 96),
	(0, 220, 220),
	(0, 128, 255),
	(64, 64, 255),
	(160, 0, 255),
	(255, 0, 180),
	(255, 0, 96),
)


def scale8(value):
	return (int(value) * BRIGHTNESS) // 255


def rgb(red, green, blue):
	return (scale8(red), scale8(green), scale8(blue))


def dim(color_value, divisor):
	red, green, blue = color_value
	return rgb(red // divisor, green // divisor, blue // divisor)


strip = NeoPixel(Pin(LED_PIN, Pin.OUT), LED_COUNT)


def make_i2c(bus_id, scl_pin, sda_pin):
	return I2C(
		bus_id,
		scl=Pin(scl_pin),
		sda=Pin(sda_pin),
		freq=I2C_FREQ,
	)


def write_reg(i2c, reg, value):
	i2c.writeto_mem(MPR121_ADDR, reg, bytes((value,)))


def read_u16(i2c, reg):
	data = i2c.readfrom_mem(MPR121_ADDR, reg, 2)
	return data[0] | (data[1] << 8)


def init_mpr121(i2c):
	write_reg(i2c, MPR121_SOFTRESET, 0x63)
	time.sleep_ms(5)

	write_reg(i2c, MPR121_ECR, 0x00)

	for electrode in range(KEY_COUNT):
		base = 0x41 + (electrode * 2)
		write_reg(i2c, base, TOUCH_THRESHOLD)
		write_reg(i2c, base + 1, RELEASE_THRESHOLD)

	write_reg(i2c, 0x2B, 0x01)
	write_reg(i2c, 0x2C, 0x01)
	write_reg(i2c, 0x2D, 0x00)
	write_reg(i2c, 0x2E, 0x00)
	write_reg(i2c, 0x2F, 0x01)
	write_reg(i2c, 0x30, 0x05)
	write_reg(i2c, 0x31, 0xFF)
	write_reg(i2c, 0x32, 0x02)
	write_reg(i2c, 0x33, 0x00)
	write_reg(i2c, 0x34, 0x00)
	write_reg(i2c, 0x35, 0x00)
	write_reg(i2c, 0x36, 0x00)
	write_reg(i2c, 0x37, 0x00)
	write_reg(i2c, 0x38, 0x00)
	write_reg(i2c, 0x39, 0x00)
	write_reg(i2c, 0x3A, 0x00)
	write_reg(i2c, MPR121_DEBOUNCE, 0x11)
	write_reg(i2c, MPR121_CONFIG1, 0x10)
	write_reg(i2c, MPR121_CONFIG2, 0x20)
	write_reg(i2c, MPR121_ECR, 0x8C)


def key_ranges():
	ranges = []
	for key_index in range(KEY_COUNT):
		start = (LED_COUNT * key_index) // KEY_COUNT
		end = (LED_COUNT * (key_index + 1)) // KEY_COUNT
		ranges.append((start, end))
	return ranges


KEY_RANGES = key_ranges()


def render(mask):
	for key_index in range(KEY_COUNT):
		start, end = KEY_RANGES[key_index]
		active = ((mask >> key_index) & 1) != 0
		base_color = KEY_COLORS[key_index]
		fill = rgb(*base_color) if active else (0, 0, 0)
		for led_index in range(start, end):
			strip[led_index] = fill

	strip.write()


def scan_for_device(i2c):
	devices = i2c.scan()
	if MPR121_ADDR not in devices:
		print("I2C devices:", [hex(device) for device in devices])
		raise OSError("MPR121 not found at 0x%02X" % MPR121_ADDR)
	return devices


def open_touch_i2c():
	last_error = None
	for bus_id, scl_pin, sda_pin in I2C_CANDIDATES:
		try:
			i2c = make_i2c(bus_id, scl_pin, sda_pin)
			devices = scan_for_device(i2c)
			print(
				"touch i2c ok bus=%d scl=%d sda=%d devices=%s"
				% (bus_id, scl_pin, sda_pin, [hex(device) for device in devices])
			)
			return i2c, bus_id, scl_pin, sda_pin
		except Exception as exc:
			last_error = exc

	raise last_error if last_error is not None else OSError("No I2C candidate worked")


def boot_animation():
	for led_index in range(LED_COUNT):
		strip[led_index] = rgb(0, 0, 24)
	strip.write()
	time.sleep_ms(120)
	for led_index in range(LED_COUNT):
		strip[led_index] = rgb(0, 0, 0)
	strip.write()


def active_electrodes(mask):
	active = []
	for key_index in range(KEY_COUNT):
		if ((mask >> key_index) & 1) != 0:
			active.append(key_index)
	return active


def main():
	i2c, bus_id, scl_pin, sda_pin = open_touch_i2c()
	print(
		"LED gpio=%d I2C bus=%d scl=%d sda=%d addr=%s"
		% (LED_PIN, bus_id, scl_pin, sda_pin, hex(MPR121_ADDR))
	)
	init_mpr121(i2c)
	boot_animation()

	last_mask = -1
	while True:
		mask = read_u16(i2c, MPR121_TOUCH_STATUS_L) & 0x0FFF
		if mask != last_mask:
			print("touch mask", hex(mask), "active", active_electrodes(mask))
			render(mask)
			last_mask = mask
		time.sleep_ms(POLL_MS)


main()