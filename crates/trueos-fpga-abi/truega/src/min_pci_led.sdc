# Basic timing constraint for fabric clock.
# Adjust the period to match the actual oscillator feeding the `clk` pin.
# Examples:
#  - 100 MHz => 10.000 ns
#  - 50  MHz => 20.000 ns
#  - 25  MHz => 40.000 ns

create_clock -name clk -period 10.000 [get_ports {clk}]
