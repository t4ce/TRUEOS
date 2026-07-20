library IEEE;
use IEEE.std_logic_1164.all;
use IEEE.numeric_std.all;

entity addition is
	port (
		a      : in  std_logic_vector(31 downto 0);
		b      : in  std_logic_vector(31 downto 0);
		result : out std_logic_vector(31 downto 0)
	);
end entity;

architecture rtl of addition is
begin
	result <= std_logic_vector(unsigned(a) + unsigned(b));
end architecture;
