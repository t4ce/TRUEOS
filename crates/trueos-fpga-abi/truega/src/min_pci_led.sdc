# The Tang Mega 138K Pro oscillator on P16 is 50 MHz. The PCIe reference
# clock wrapper multiplies it to 200 MHz and CLKDIV routes a 100 MHz TLP clock.
create_clock -name board_clk -period 20.000 [get_ports {clk}]
create_generated_clock -name tlp_clk -source [get_ports {clk}] -master_clock board_clk -divide_by 1 -multiply_by 2 -duty_cycle 50 [get_pins {u_clock/uut_div2/CLKOUT}]
