!!! only "make iso" when invoking "make" here. No other make targets usually!!!
# Build, reboot, connect, verify logs.
cargo fmt
!make iso or make run if user ask for it
# verify (logs) needs up to 60sec
nc 192.168.178.94 1