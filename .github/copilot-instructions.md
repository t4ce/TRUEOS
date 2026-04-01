!!! only "make iso" when invoking "make" here. No other make targets !!!
# Build, reboot, connect, verify logs.
cargo fmt
!make iso
# verify (logs) needs up to 60sec
nc 192.168.178.94 1