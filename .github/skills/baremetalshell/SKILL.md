---
name: baremetalshell
description: Connects to our Kernels Shell via Netshell backend.
---
Define the functionality provided by this skill, including detailed instructions and examples

konsole -e sh -c 'stty -echo -icanon cols 200 rows 60; nc 192.168.178.78 4245; stty sane'

send enter to start

cruicial commands

"acpi reboot" -> pxe will reload the kernel. 
IF "make iso" was executed on this dev host before this cmd is called:
it conceptually completes a full iteration automated so you can reconnect with this skill again.

type cmd to see the list of available commands