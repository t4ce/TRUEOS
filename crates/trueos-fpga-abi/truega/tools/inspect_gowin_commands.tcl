open_project /home/t4ce/REPOS/TRUEOS/crates/trueos-fpga-abi/truega/min_pci_led.gprj
read_ipc /home/t4ce/REPOS/TRUEOS/crates/trueos-fpga-abi/truega/src/serdes/pcie_controller/pcie_controller.ipc
set truega_pcie [get_ips PCIE_Controller_Top]
puts "IP=$truega_pcie"
generate_target $truega_pcie
