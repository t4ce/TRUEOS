# Run from the directory produced by prepare_usb_rgba8.py.
open_project cy_usb_bulkloop.gprj
set_option -top_module Top
set_option -output_base_name truega_usb_rgba8
run syn
run pnr
