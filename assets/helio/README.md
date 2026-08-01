# Build-produced Helio programs

`simple-cube.trueos.intel.helio` is generated and validated by
`tools/helio-build/build-simple-cube.sh`. It is kept at this stable path so the
TRUEOS build can embed the exact frontend IR and Intel native shader package
without reconstructing any scene data.

`churn-forward.trueos.intel.helio` is independently generated and validated by
`tools/helio-build/build-churn-forward.sh`. It preserves the working cube
program while adding Helio's GPU-native camera/instance/compaction/indirect
contract and the matching Intel executable and fixed-function state.
