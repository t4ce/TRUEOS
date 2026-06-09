import time
import socket

try:
	import network  # type: ignore
except ImportError:
	network = None

from machine import I2C, Pin  # type: ignore
from neopixel import NeoPixel  # type: ignore


LED_PIN = 0
LED_COUNT = 24
BRIGHTNESS = 127

WIFI_SSID = ""
WIFI_PASSWORD = ""
TRUEOS_UDP_HOST = "255.255.255.255"
TRUEOS_UDP_PORT = 9696
UDP_SEND_MS = 128
PIANO_BASE_NOTE = 36

I2C_FREQ = 400000
I2C_CANDIDATES = (
	(0, 1, 2),
	(0, 2, 1),
	(1, 1, 2),
	(1, 2, 1),
)
MPR121_ADDRS = (0x5A, 0x5B)

TOUCH_THRESHOLD = 16
RELEASE_THRESHOLD = 8
POLL_MS = 12

MPR121_TOUCH_STATUS_L = 0x00
MPR121_SOFTRESET = 0x80
MPR121_ECR = 0x5E
MPR121_DEBOUNCE = 0x5B
MPR121_CONFIG1 = 0x5C
MPR121_CONFIG2 = 0x5D

BOARD_KEY_COUNT = 12
KEY_COUNT = len(MPR121_ADDRS) * BOARD_KEY_COUNT
TRUEOS_PIANO_KEY_COUNT = 96
TRUEOS_PIANO_MASK_BYTES = TRUEOS_PIANO_KEY_COUNT // 8
TRUEOS_PIANO_STATE_FRAME_LEN = 14 + TRUEOS_PIANO_MASK_BYTES
TRUEOS_PIANO_MAGIC = b"TPNO"
TRUEOS_PIANO_STATE_VERSION = 2

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


def write_reg(i2c, addr, reg, value):
	i2c.writeto_mem(addr, reg, bytes((value,)))


def read_u16(i2c, addr, reg):
	data = i2c.readfrom_mem(addr, reg, 2)
	return data[0] | (data[1] << 8)


def init_mpr121(i2c, addr):
	write_reg(i2c, addr, MPR121_SOFTRESET, 0x63)
	time.sleep_ms(5)

	write_reg(i2c, addr, MPR121_ECR, 0x00)

	for electrode in range(BOARD_KEY_COUNT):
		base = 0x41 + (electrode * 2)
		write_reg(i2c, addr, base, TOUCH_THRESHOLD)
		write_reg(i2c, addr, base + 1, RELEASE_THRESHOLD)

	write_reg(i2c, addr, 0x2B, 0x01)
	write_reg(i2c, addr, 0x2C, 0x01)
	write_reg(i2c, addr, 0x2D, 0x00)
	write_reg(i2c, addr, 0x2E, 0x00)
	write_reg(i2c, addr, 0x2F, 0x01)
	write_reg(i2c, addr, 0x30, 0x05)
	write_reg(i2c, addr, 0x31, 0xFF)
	write_reg(i2c, addr, 0x32, 0x02)
	write_reg(i2c, addr, 0x33, 0x00)
	write_reg(i2c, addr, 0x34, 0x00)
	write_reg(i2c, addr, 0x35, 0x00)
	write_reg(i2c, addr, 0x36, 0x00)
	write_reg(i2c, addr, 0x37, 0x00)
	write_reg(i2c, addr, 0x38, 0x00)
	write_reg(i2c, addr, 0x39, 0x00)
	write_reg(i2c, addr, 0x3A, 0x00)
	write_reg(i2c, addr, MPR121_DEBOUNCE, 0x11)
	write_reg(i2c, addr, MPR121_CONFIG1, 0x10)
	write_reg(i2c, addr, MPR121_CONFIG2, 0x20)
	write_reg(i2c, addr, MPR121_ECR, 0x8C)


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
		base_color = KEY_COLORS[key_index % len(KEY_COLORS)]
		fill = rgb(*base_color) if active else (0, 0, 0)
		for led_index in range(start, end):
			strip[led_index] = fill

	strip.write()


def scan_for_devices(i2c):
	devices = i2c.scan()
	missing = [addr for addr in MPR121_ADDRS if addr not in devices]
	if missing:
		print("I2C devices:", [hex(device) for device in devices])
		raise OSError("MPR121 missing: %s" % [hex(addr) for addr in missing])
	return devices


def open_touch_i2c():
	last_error = None
	for bus_id, scl_pin, sda_pin in I2C_CANDIDATES:
		try:
			i2c = make_i2c(bus_id, scl_pin, sda_pin)
			devices = scan_for_devices(i2c)
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


def ticks_ms():
	return time.ticks_ms() if hasattr(time, "ticks_ms") else int(time.time() * 1000)


def ticks_diff(newer, older):
	return time.ticks_diff(newer, older) if hasattr(time, "ticks_diff") else newer - older


def connect_wifi():
	if not WIFI_SSID or network is None:
		return

	wlan = network.WLAN(network.STA_IF)
	wlan.active(True)
	if not wlan.isconnected():
		print("wifi connecting", WIFI_SSID)
		wlan.connect(WIFI_SSID, WIFI_PASSWORD)
		deadline = ticks_ms() + 15000
		while not wlan.isconnected() and ticks_diff(deadline, ticks_ms()) > 0:
			time.sleep_ms(100)

	print("wifi", wlan.ifconfig() if wlan.isconnected() else "not connected")


def open_udp():
	sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
	try:
		sock.setsockopt(socket.SOL_SOCKET, socket.SO_BROADCAST, 1)
	except Exception:
		pass
	return sock


def read_touch_mask(i2c):
	mask = 0
	for board_index, addr in enumerate(MPR121_ADDRS):
		board_mask = read_u16(i2c, addr, MPR121_TOUCH_STATUS_L) & 0x0FFF
		mask |= board_mask << (board_index * BOARD_KEY_COUNT)
	return mask


def make_piano_state_frame(seq, t_ms, mask):
	frame = bytearray(TRUEOS_PIANO_STATE_FRAME_LEN)
	frame[0:4] = TRUEOS_PIANO_MAGIC
	frame[4] = TRUEOS_PIANO_STATE_VERSION
	frame[5] = 0
	frame[6] = PIANO_BASE_NOTE & 0x7F
	frame[7] = TRUEOS_PIANO_KEY_COUNT
	frame[8] = seq & 0xFF
	frame[9] = (seq >> 8) & 0xFF
	frame[10] = t_ms & 0xFF
	frame[11] = (t_ms >> 8) & 0xFF
	frame[12] = (t_ms >> 16) & 0xFF
	frame[13] = (t_ms >> 24) & 0xFF
	for byte_index in range(TRUEOS_PIANO_MASK_BYTES):
		frame[14 + byte_index] = (mask >> (byte_index * 8)) & 0xFF
	return frame


def main():
	connect_wifi()
	udp = open_udp()
	i2c, bus_id, scl_pin, sda_pin = open_touch_i2c()
	print(
		"LED gpio=%d I2C bus=%d scl=%d sda=%d addrs=%s udp=%s:%d"
		% (
			LED_PIN,
			bus_id,
			scl_pin,
			sda_pin,
			[hex(addr) for addr in MPR121_ADDRS],
			TRUEOS_UDP_HOST,
			TRUEOS_UDP_PORT,
		)
	)
	for addr in MPR121_ADDRS:
		init_mpr121(i2c, addr)
	boot_animation()

	last_mask = -1
	seq = 0
	last_send = ticks_ms() - UDP_SEND_MS
	while True:
		mask = read_touch_mask(i2c)
		if mask != last_mask:
			old_mask = 0 if last_mask < 0 else last_mask
			changed = old_mask ^ mask
			downs = changed & mask
			ups = changed & old_mask
			print(
				"touch mask",
				hex(mask),
				"downs",
				hex(downs),
				"ups",
				hex(ups),
				"active",
				active_electrodes(mask),
			)
			render(mask)
			last_mask = mask

		now = ticks_ms()
		if ticks_diff(now, last_send) >= UDP_SEND_MS:
			frame = make_piano_state_frame(seq, now & 0xFFFFFFFF, mask)
			try:
				udp.sendto(frame, (TRUEOS_UDP_HOST, TRUEOS_UDP_PORT))
			except Exception as exc:
				print("udp send failed", exc)
			seq = (seq + 1) & 0xFFFF
			last_send = now

		time.sleep_ms(POLL_MS)


main()
