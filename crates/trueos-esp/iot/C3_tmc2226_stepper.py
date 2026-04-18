"""MicroPython stepper demo matching rotateTMC2226/main/main.c."""

import time

from machine import Pin, PWM  # type: ignore


FULL_STEPS_PER_REV = 200

P_EN = 21
P_MS1 = 20
P_MS2 = 10
P_PDN = 9
P_STDBY = 8
P_STEP = 6
P_DIR = 5

# MS2  MS1  microsteps  pulses_rev  600Hz_ms  1200Hz_ms  1600Hz_ms  2000Hz_ms
#  0    0       8           1600       2666       1333       1000        800
#  0    1       2            400        666        333        250        200
#  1    0      64          12800      21333      10666       8000       6400
#  1    1      16           3200       5333       2666       2000       1600
STEP_RATE_OPTIONS_HZ = (600, 1200, 1600, 2000)

MICROSTEP_TABLE = {
	(0, 0): {
		"name": "1/8 step",
		"microsteps": 8,
		"pulses_rev": FULL_STEPS_PER_REV * 8,
		"turn_ms": {600: 2666, 1200: 1333, 1600: 1000, 2000: 800},
	},
	(0, 1): {
		"name": "1/2 step",
		"microsteps": 2,
		"pulses_rev": FULL_STEPS_PER_REV * 2,
		"turn_ms": {600: 666, 1200: 333, 1600: 250, 2000: 200},
	},
	(1, 0): {
		"name": "1/64 step",
		"microsteps": 64,
		"pulses_rev": FULL_STEPS_PER_REV * 64,
		"turn_ms": {600: 21333, 1200: 10666, 1600: 8000, 2000: 6400},
	},
	(1, 1): {
		"name": "1/16 step",
		"microsteps": 16,
		"pulses_rev": FULL_STEPS_PER_REV * 16,
		"turn_ms": {600: 5333, 1200: 2666, 1600: 2000, 2000: 1600},
	},
}

MICROSTEP_SELECTION = (1, 1)
MICROSTEP_CONFIG = MICROSTEP_TABLE[MICROSTEP_SELECTION]
MS2_LEVEL, MS1_LEVEL = MICROSTEP_SELECTION
MICROSTEP_NAME = MICROSTEP_CONFIG["name"]
MICROSTEP_FACTOR = MICROSTEP_CONFIG["microsteps"]

STEP_RATE_HZ = 600
if STEP_RATE_HZ not in STEP_RATE_OPTIONS_HZ:
	raise ValueError("Unsupported STEP_RATE_HZ: %s" % STEP_RATE_HZ)
if STEP_RATE_HZ not in MICROSTEP_CONFIG["turn_ms"]:
	raise ValueError("No turn_ms configured for STEP_RATE_HZ: %s" % STEP_RATE_HZ)
STEP_HIGH_US = max(10, 500000 // STEP_RATE_HZ)
STEP_LOW_US = STEP_HIGH_US
STEPS_FOR_COMPLETE = MICROSTEP_CONFIG["pulses_rev"]
TURN_TIME_MS = MICROSTEP_CONFIG["turn_ms"][STEP_RATE_HZ]
IDLE_MS = 2500
ENABLE_WARMUP_US = 12500
MANUAL_STEP_PULSE_US = 250


class StepperDemo:
	def __init__(self):
		self.enable = Pin(P_EN, Pin.OUT, value=1)
		self.ms1 = Pin(P_MS1, Pin.OUT, value=MS1_LEVEL)
		self.ms2 = Pin(P_MS2, Pin.OUT, value=MS2_LEVEL)
		self.pdn = Pin(P_PDN, Pin.OUT, value=0)
		self.standby = Pin(P_STDBY, Pin.OUT, value=0)
		self.step = Pin(P_STEP, Pin.OUT, value=0)
		self.direction = Pin(P_DIR, Pin.OUT, value=0)
		self.step_pwm = None
		self.set_idle_state()

	def stop_step_pwm(self):
		if self.step_pwm is not None:
			self.step_pwm.deinit()
			self.step_pwm = None
		self.step.value(0)

	def set_idle_state(self):
		self.stop_step_pwm()
		self.enable.value(1)

	def start_step_pwm(self):
		if self.step_pwm is not None:
			self.step_pwm.deinit()
		self.step_pwm = PWM(self.step, freq=STEP_RATE_HZ)
		if hasattr(self.step_pwm, "duty_u16"):
			self.step_pwm.duty_u16(32768)
		else:
			self.step_pwm.duty(512)

	def single_step(self):
		self.step.value(1)
		time.sleep_us(MANUAL_STEP_PULSE_US)
		self.step.value(0)
		time.sleep_us(MANUAL_STEP_PULSE_US)

	def warmup_driver(self):
		original_direction = self.direction.value()
		time.sleep_us(ENABLE_WARMUP_US)
		self.direction.value(original_direction)
		self.single_step()
		self.direction.value(0 if original_direction else 1)
		self.single_step()
		self.direction.value(original_direction)
		time.sleep_us(ENABLE_WARMUP_US)

	def turn_once(self):
		self.enable.value(0)
		try:
			self.warmup_driver()
			self.start_step_pwm()
			time.sleep_ms(TURN_TIME_MS)
		finally:
			self.stop_step_pwm()
			self.set_idle_state()

	def run(self):
		print(
			"Start! EN=%d STDBY=%d mode=%s microsteps=%d steps=%d turn_ms=%d"
			% (
				self.enable.value(),
				self.standby.value(),
				MICROSTEP_NAME,
				MICROSTEP_FACTOR,
				STEPS_FOR_COMPLETE,
				TURN_TIME_MS,
			)
		)
		while True:
			print("Turning Once - %d microsteps at %d Hz" % (STEPS_FOR_COMPLETE, STEP_RATE_HZ))
			self.turn_once()
			print("Done stepping")
			time.sleep_ms(IDLE_MS)


def main():
	demo = StepperDemo()
	try:
		demo.run()
	finally:
		demo.set_idle_state()


main()