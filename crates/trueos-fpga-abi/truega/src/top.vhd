library IEEE;
use IEEE.std_logic_1164.all;
use IEEE.numeric_std.all;

entity top is
	port (
		clk          : in  std_logic;
		pcie_perst_n : in  std_logic;

		usr_led0 : out std_logic;
		usr_led1 : out std_logic;
		usr_led2 : out std_logic;
		usr_led3 : out std_logic;
		usr_led4 : out std_logic
	);
end entity;

architecture rtl of top is
	component Truega_Pcie_Clock is
		port (
			clkin   : in  std_logic;
			tlp_clk : out std_logic;
			lock    : out std_logic
		);
	end component;

	component SerDes_Top is
		port (
			PCIE_Controller_Top_pcie_tl_rx_sop_o        : out std_logic;
			PCIE_Controller_Top_pcie_tl_rx_eop_o        : out std_logic;
			PCIE_Controller_Top_pcie_tl_rx_data_o       : out std_logic_vector(255 downto 0);
			PCIE_Controller_Top_pcie_tl_rx_valid_o      : out std_logic_vector(7 downto 0);
			PCIE_Controller_Top_pcie_tl_rx_bardec_o     : out std_logic_vector(5 downto 0);
				PCIE_Controller_Top_pcie_tl_rx_err_o        : out std_logic_vector(7 downto 0);
				PCIE_Controller_Top_pcie_tl_tx_wait_o       : out std_logic;
				PCIE_Controller_Top_pcie_tl_int_ack_o       : out std_logic;
			PCIE_Controller_Top_pcie_ltssm_o            : out std_logic_vector(4 downto 0);
			PCIE_Controller_Top_pcie_tl_tx_creditsp_o   : out std_logic_vector(31 downto 0);
			PCIE_Controller_Top_pcie_tl_tx_creditsnp_o  : out std_logic_vector(31 downto 0);
			PCIE_Controller_Top_pcie_tl_tx_creditscpl_o : out std_logic_vector(31 downto 0);
			PCIE_Controller_Top_pcie_tl_cfg_busdev_o    : out std_logic_vector(12 downto 0);
			PCIE_Controller_Top_pcie_linkup_o           : out std_logic;
			PCIE_Controller_Top_pcie_tl_drp_clk_o       : out std_logic;
			PCIE_Controller_Top_pcie_tl_drp_rddata_o    : out std_logic_vector(31 downto 0);
			PCIE_Controller_Top_pcie_tl_drp_resp_o      : out std_logic;
			PCIE_Controller_Top_pcie_tl_drp_rd_valid_o  : out std_logic;
			PCIE_Controller_Top_pcie_tl_drp_ready_o     : out std_logic;

			debug_refclk_det_o : out std_logic;
			debug_rx_lock_o    : out std_logic;

			PCIE_Controller_Top_pcie_rstn_i          : in  std_logic;
			PCIE_Controller_Top_pcie_tl_clk_i        : in  std_logic;
			PCIE_Controller_Top_pcie_tl_rx_wait_i    : in  std_logic;
			PCIE_Controller_Top_pcie_tl_rx_masknp_i  : in  std_logic;
			PCIE_Controller_Top_pcie_tl_tx_sop_i     : in  std_logic;
			PCIE_Controller_Top_pcie_tl_tx_eop_i     : in  std_logic;
				PCIE_Controller_Top_pcie_tl_tx_data_i    : in  std_logic_vector(255 downto 0);
				PCIE_Controller_Top_pcie_tl_tx_valid_i   : in  std_logic_vector(7 downto 0);
				PCIE_Controller_Top_pcie_tl_int_status_i : in  std_logic;
				PCIE_Controller_Top_pcie_tl_int_req_i    : in  std_logic;
				PCIE_Controller_Top_pcie_tl_int_msinum_i : in  std_logic_vector(4 downto 0);
			PCIE_Controller_Top_pcie_tl_drp_addr_i   : in  std_logic_vector(23 downto 0);
			PCIE_Controller_Top_pcie_tl_drp_wrdata_i : in  std_logic_vector(31 downto 0);
			PCIE_Controller_Top_pcie_tl_drp_strb_i   : in  std_logic_vector(7 downto 0);
			PCIE_Controller_Top_pcie_tl_drp_wr_i     : in  std_logic;
			PCIE_Controller_Top_pcie_tl_drp_rd_i     : in  std_logic
		);
		end component;

	component truega_completion_irq is
		port (
			clk                : in  std_logic;
			reset_n            : in  std_logic;
			retire_i           : in  std_logic;
			interrupt_enable_i : in  std_logic;
			bar_ack_i          : in  std_logic;
			controller_ack_i   : in  std_logic;
			status_o           : out std_logic;
			request_o          : out std_logic;
			msinum_o           : out std_logic_vector(4 downto 0)
		);
	end component;

	-- Generated from tools/tga-gen/src/firmware.rs by the Ubuntu firmware build.
	-- This is three fixed circuits plus a slot mux, not a processor or interpreter.
	component truega_functions is
		port (
			clk                  : in  std_logic;
			reset_n              : in  std_logic;
			start                : in  std_logic;
			function_id          : in  std_logic_vector(15 downto 0);
			input_data           : in  std_logic_vector(767 downto 0);
			led_state            : in  std_logic_vector(4 downto 0);
			next_led             : out std_logic_vector(4 downto 0);
			output_data          : out std_logic_vector(767 downto 0);
			required_input_bytes : out std_logic_vector(15 downto 0);
			output_bytes         : out std_logic_vector(15 downto 0);
			valid                : out std_logic;
			busy                 : out std_logic;
			done                 : out std_logic;
			error                : out std_logic
		);
	end component;

	-- Generated from the same catalogue and function RTL as the host Rust interface.
	-- This is a constant ROM exposed for exact bundle admission, not executable state.
	component truega_firmware_manifest is
		port (
			word_index : in  std_logic_vector(4 downto 0);
			data       : out std_logic_vector(31 downto 0)
		);
	end component;

	-- Optional BAR2 row streamer. It reuses the exact Q8 row and SiLU circuits
	-- but retires a complete output row instead of one inline block pair.
	component truega_lfm25_row_streamer is
		port (
			clk                    : in  std_logic;
			reset_n                : in  std_logic;
			write_i                : in  std_logic;
			write_addr_dw_i        : in  std_logic_vector(16 downto 0);
			write_data_i           : in  std_logic_vector(31 downto 0);
			start_i                : in  std_logic;
			mode_i                 : in  std_logic_vector(1 downto 0);
			busy_o                 : out std_logic;
			done_o                 : out std_logic;
			error_o                : out std_logic;
			error_code_o           : out std_logic_vector(31 downto 0);
			gate_q30_o             : out std_logic_vector(63 downto 0);
			up_q30_o               : out std_logic_vector(63 downto 0);
			result_q30_o           : out std_logic_vector(63 downto 0);
			accepted_write_count_o : out std_logic_vector(31 downto 0)
		);
	end component;

	-- Fixed ten-way LFM2.5 decode circuit selector.  The instance is deliberately
	-- disabled until every resident engine and its model-feed data plane is joined.
	-- With ENABLE=0 it publishes zero capability words, so host admission remains
	-- fail-closed even though the final BAR/MSI envelope is already physical.
	component truega_lfm25_decode_dispatch is
		generic (
			ENABLE : integer := 0
		);
		port (
			clk                         : in  std_logic;
			reset_n                     : in  std_logic;
			command_i                   : in  std_logic_vector(31 downto 0);
			position_i                  : in  std_logic_vector(31 downto 0);
			session_epoch_i             : in  std_logic_vector(31 downto 0);
			doorbell_i                  : in  std_logic;
			doorbell_value_i            : in  std_logic_vector(31 downto 0);
			capability_magic_o          : out std_logic_vector(31 downto 0);
			capability_bits_o           : out std_logic_vector(31 downto 0);
			state_o                     : out std_logic_vector(31 downto 0);
			result0_o                   : out std_logic_vector(31 downto 0);
			result1_o                   : out std_logic_vector(31 downto 0);
			argmax_score_q30_o          : out std_logic_vector(63 downto 0);
			execute_start_o             : out std_logic;
			execute_operation_o         : out std_logic_vector(3 downto 0);
			execute_layer_o             : out std_logic_vector(7 downto 0);
			execute_position_o          : out std_logic_vector(31 downto 0);
			execute_input_slot_o        : out std_logic_vector(7 downto 0);
			execute_residual_slot_o     : out std_logic_vector(7 downto 0);
			execute_session_epoch_o     : out std_logic_vector(31 downto 0);
			execute_session_begin_o     : out std_logic;
			engine_done_i               : in  std_logic;
			engine_error_i              : in  std_logic;
			engine_error_code_i         : in  std_logic_vector(31 downto 0);
			engine_result_slot_i        : in  std_logic_vector(7 downto 0);
			engine_result_position_i    : in  std_logic_vector(31 downto 0);
			engine_argmax_token_i       : in  std_logic_vector(31 downto 0);
			engine_argmax_rows_i        : in  std_logic_vector(31 downto 0);
			engine_argmax_score_q30_i   : in  std_logic_vector(63 downto 0);
			retire_o                    : out std_logic
		);
	end component;

	type word_arr_t is array (0 to 7) of std_logic_vector(31 downto 0);
	type call_data_arr_t is array (0 to 23) of std_logic_vector(31 downto 0);
	subtype byte_t is std_logic_vector(7 downto 0);
	constant PKT_MAX_WORDS : integer := 8;
	constant BAR0_LED_DW : std_logic_vector(9 downto 0) := "0000000000";
	constant BAR0_RESET_DW : std_logic_vector(9 downto 0) := "0000000100";
	constant BAR0_STATUS_DW : std_logic_vector(9 downto 0) := "0000000101";
	constant BAR0_MAGIC_DW : std_logic_vector(9 downto 0) := "0000001000";
	constant BAR0_DBG_SEEN_DW : std_logic_vector(9 downto 0) := "0000010000";
	constant BAR0_DBG_LAST_ADDR_DW : std_logic_vector(9 downto 0) := "0000010001";
	constant BAR0_DBG_LAST_READ_DATA_DW : std_logic_vector(9 downto 0) := "0000010010";
	constant BAR0_DBG_LAST_REQ_DW : std_logic_vector(9 downto 0) := "0000010011";
	constant BAR0_DBG_LAST_CPLD0_DW : std_logic_vector(9 downto 0) := "0000010100";
	constant BAR0_DBG_LAST_CPLD1_DW : std_logic_vector(9 downto 0) := "0000010101";
	constant BAR0_DBG_LAST_CPLD2_DW : std_logic_vector(9 downto 0) := "0000010110";
	constant BAR0_DBG_LAST_CPLD_DATA_DW : std_logic_vector(9 downto 0) := "0000010111";
	constant BAR0_DBG_RX_CAPTURE_COUNT_DW : std_logic_vector(9 downto 0) := "0000011000";
	constant BAR0_DBG_WRITE_COUNT_DW : std_logic_vector(9 downto 0) := "0000011001";
	constant BAR0_DBG_WORD30_WRITE_COUNT_DW : std_logic_vector(9 downto 0) := "0000011010";
	constant BAR0_DBG_WORD30_LAST_PAYLOAD_DW : std_logic_vector(9 downto 0) := "0000011011";
	constant BAR0_DBG_WORD30_STORAGE_DW : std_logic_vector(9 downto 0) := "0000011100";
	constant BAR0_DBG_RX_FIFO_STATE_DW : std_logic_vector(9 downto 0) := "0000011101";
	constant BAR0_DBG_RX_ERROR_COUNT_DW : std_logic_vector(9 downto 0) := "0000011110";
	constant BAR0_CALL_DOORBELL_DW : integer := 16#080# / 4;
	constant BAR0_CALL_IRQ_ACK_DW : integer := 16#084# / 4;
	constant BAR0_CALL_IRQ_RETIRE_COUNT_DW : integer := 16#088# / 4;
	constant BAR0_CALL_IRQ_REQUEST_COUNT_DW : integer := 16#08C# / 4;
	constant BAR0_CALL_IRQ_CONTROLLER_ACK_COUNT_DW : integer := 16#090# / 4;
	constant BAR0_CALL_IRQ_STATE_DW : integer := 16#094# / 4;
	constant BAR0_STREAM_CAPABILITY_DW : integer := 16#098# / 4;
	constant BAR0_STREAM_CONTROL_DW : integer := 16#09C# / 4;
	constant BAR0_STREAM_ROW_DW : integer := 16#0A0# / 4;
	constant BAR0_STREAM_DOORBELL_DW : integer := 16#0A4# / 4;
	constant BAR0_STREAM_STATE_DW : integer := 16#0A8# / 4;
	constant BAR0_STREAM_GATE_LO_DW : integer := 16#0AC# / 4;
	constant BAR0_STREAM_GATE_HI_DW : integer := 16#0B0# / 4;
	constant BAR0_STREAM_UP_LO_DW : integer := 16#0B4# / 4;
	constant BAR0_STREAM_UP_HI_DW : integer := 16#0B8# / 4;
	constant BAR0_STREAM_RESULT_LO_DW : integer := 16#0BC# / 4;
	constant BAR0_STREAM_RESULT_HI_DW : integer := 16#0C0# / 4;
	constant BAR0_STREAM_ERROR_DW : integer := 16#0C4# / 4;
	constant BAR0_STREAM_COMPLETION_COUNT_DW : integer := 16#0C8# / 4;
	constant BAR0_STREAM_ACCEPTED_WRITE_COUNT_DW : integer := 16#0CC# / 4;
	constant BAR0_STREAM_RX_CAPTURE_COUNT_DW : integer := 16#0D0# / 4;
	constant BAR0_STREAM_DECODED_WRITE_COUNT_DW : integer := 16#0D4# / 4;
	constant BAR0_STREAM_RX_ERROR_COUNT_DW : integer := 16#0D8# / 4;
	constant BAR0_DECODE_CAPABILITY_MAGIC_DW : integer := 16#0DC# / 4;
	constant BAR0_DECODE_CAPABILITY_BITS_DW : integer := 16#0E0# / 4;
	constant BAR0_DECODE_COMMAND_DW : integer := 16#0E4# / 4;
	constant BAR0_DECODE_POSITION_DW : integer := 16#0E8# / 4;
	constant BAR0_DECODE_SESSION_EPOCH_DW : integer := 16#0EC# / 4;
	constant BAR0_DECODE_DOORBELL_DW : integer := 16#0F0# / 4;
	constant BAR0_DECODE_STATE_DW : integer := 16#0F4# / 4;
	constant BAR0_DECODE_RESULT0_DW : integer := 16#0F8# / 4;
	constant BAR0_DECODE_RESULT1_DW : integer := 16#0FC# / 4;
	constant BAR0_CALL_BASE_DW : integer := 16#100# / 4;
	constant BAR0_FIRMWARE_MANIFEST_BASE_DW : integer := 16#200# / 4;
	-- TGF2's read-only publication follows the generated manifest.  Keep the
	-- words physically decoded but fail closed until the complete fixed decode
	-- engine and feed frontend are joined into this image.
	constant BAR0_FEED_CAPABILITY_BASE_DW : integer := 16#280# / 4;
	constant FIRMWARE_MANIFEST_WORD_COUNT : integer := 32;
	constant FEED_CAPABILITY_WORD_COUNT : integer := 5;
	constant CALL_WORD_COUNT : integer := 64;
	constant CALL_MAGIC_WORD : integer := 0;
	constant CALL_ABI_FUNCTION_WORD : integer := 1;
	constant CALL_STATE_WORD : integer := 4;
	constant CALL_INPUT_LEN_WORD : integer := 6;
	constant CALL_OUTPUT_CAP_WORD : integer := 7;
	constant CALL_OUTPUT_LEN_WORD : integer := 8;
	constant CALL_ERROR_WORD : integer := 9;
	constant CALL_INPUT_WORD : integer := 16;
	constant CALL_OUTPUT_WORD : integer := 40;
	constant DEBUG_TARGET_PACKAGE_WORD : integer := 30;
	constant PROTOCOL_MAGIC : std_logic_vector(31 downto 0) := x"54534154";
	constant WORK_PACKAGE_MAGIC : std_logic_vector(31 downto 0) := x"4B505754";
	constant WORK_ABI_VERSION : std_logic_vector(15 downto 0) := x"0001";
	constant WORK_STATE_IDLE : std_logic_vector(31 downto 0) := x"00000000";
	constant WORK_STATE_HOST_READY : std_logic_vector(31 downto 0) := x"00000001";
	constant WORK_STATE_FPGA_BUSY : std_logic_vector(31 downto 0) := x"00000002";
	constant WORK_STATE_COMPLETE : std_logic_vector(31 downto 0) := x"00000003";
	constant WORK_STATE_FAILED : std_logic_vector(31 downto 0) := x"00000004";
	constant CALL_DOORBELL_MAGIC : std_logic_vector(31 downto 0) := x"4C4C4143";
	constant CALL_ERROR_BAD_PACKAGE : std_logic_vector(31 downto 0) := x"BAD00001";
	constant CALL_ERROR_BAD_LENGTH : std_logic_vector(31 downto 0) := x"BAD00002";
	constant CALL_ERROR_BAD_FUNCTION : std_logic_vector(31 downto 0) := x"BAD00003";
	constant CALL_ERROR_FUNCTION_FAILED : std_logic_vector(31 downto 0) := x"BAD00004";
	constant STREAM_CAPABILITY_MAGIC : std_logic_vector(31 downto 0) := x"32524754";
	constant STREAM_DOORBELL_MAGIC : std_logic_vector(31 downto 0) := x"4D525453";
	constant STREAM_STATE_IDLE : std_logic_vector(31 downto 0) := x"00000000";
	constant STREAM_STATE_BUSY : std_logic_vector(31 downto 0) := x"00000001";
	constant STREAM_STATE_COMPLETE : std_logic_vector(31 downto 0) := x"00000002";
	constant STREAM_STATE_FAILED : std_logic_vector(31 downto 0) := x"00000003";
	constant STREAM_ERROR_BAD_DOORBELL : std_logic_vector(31 downto 0) := x"BAD20001";
	constant DECODE_STATE_COMPLETE : std_logic_vector(31 downto 0) := x"00000002";
	constant DECODE_STATE_FAILED : std_logic_vector(31 downto 0) := x"00000003";
	constant DECODE_OP_LM_HEAD_ARGMAX : std_logic_vector(7 downto 0) := x"09";
	constant LED_DEBUG_ON : std_logic_vector(31 downto 0) := x"D06D0001";
	constant LED_DEBUG_OFF : std_logic_vector(31 downto 0) := x"D06D0000";
	constant BAR_READ_BANK_NONE : std_logic_vector(2 downto 0) := "000";
	constant BAR_READ_BANK_CONTROL : std_logic_vector(2 downto 0) := "001";
	constant BAR_READ_BANK_CALL_HEADER : std_logic_vector(2 downto 0) := "010";
	constant BAR_READ_BANK_CALL_INPUT : std_logic_vector(2 downto 0) := "011";
	constant BAR_READ_BANK_CALL_OUTPUT : std_logic_vector(2 downto 0) := "100";
	constant BAR_READ_BANK_MANIFEST : std_logic_vector(2 downto 0) := "101";
	constant BAR_READ_BANK_STREAM : std_logic_vector(2 downto 0) := "110";
	constant BAR_READ_BANK_DEBUG : std_logic_vector(2 downto 0) := "111";

	signal pcie_linkup : std_logic;
	signal tlp_clk : std_logic;
	signal pll_lock : std_logic;
	signal pcie_core_reset_n : std_logic;
	signal pcie_reset_hold : unsigned(15 downto 0) := (others => '0');

	signal tl_rx_sop    : std_logic;
	signal tl_rx_eop    : std_logic;
	signal tl_rx_data   : std_logic_vector(255 downto 0);
	signal tl_rx_valid  : std_logic_vector(7 downto 0);
	signal tl_rx_bardec : std_logic_vector(5 downto 0);
	signal tl_rx_err    : std_logic_vector(7 downto 0);
	signal tl_tx_wait   : std_logic;
	signal tl_tx_data   : std_logic_vector(255 downto 0) := (others => '0');
	signal tl_tx_valid  : std_logic_vector(7 downto 0) := (others => '0');
	signal tl_tx_sop    : std_logic := '0';
	signal tl_tx_eop    : std_logic := '0';
	signal tl_cfg_busdev : std_logic_vector(12 downto 0);
	signal call_irq_controller_ack : std_logic;
	signal call_irq_status : std_logic;
	signal call_irq_request : std_logic;
	signal call_irq_msinum : std_logic_vector(4 downto 0);
	signal call_irq_retire : std_logic := '0';
	signal call_irq_bar_ack : std_logic := '0';
	signal call_irq_request_prev : std_logic := '0';
	signal call_irq_controller_ack_prev : std_logic := '0';
	signal call_irq_request_count : unsigned(31 downto 0) := (others => '0');
	signal call_irq_controller_ack_count : unsigned(31 downto 0) := (others => '0');
	signal stream_irq_enable : std_logic := '0';
	signal decode_irq_enable : std_logic := '0';
	signal decode_irq_retire : std_logic;

	-- Active-high logical state (the board outputs are inverted below).  Seed a
	-- visible one-hot heartbeat so a configured, idle image is never all-dark.
	signal led_reg : std_logic_vector(4 downto 0) := "00001";
	-- Normal image: the five LEDs belong to the fused heartbeat function. A
	-- transport diagnostic can still enable the sticky PCIe milestones through
	-- the explicit LED_DEBUG_ON BAR write.
	signal debug_led_mode : std_logic := '0';
	-- The complete 256-byte work package is a fixed register file. The two 96-byte
	-- envelopes are physically backed so compiled slots can consume and produce their
	-- declared byte shapes without a device-side command processor or DMA requester.
	signal call_magic : std_logic_vector(31 downto 0) := WORK_PACKAGE_MAGIC;
	signal call_abi_function : std_logic_vector(31 downto 0) := x"00000001";
	signal call_id_low : std_logic_vector(31 downto 0) := (others => '0');
	signal call_id_high : std_logic_vector(31 downto 0) := (others => '0');
	signal call_state : std_logic_vector(31 downto 0) := WORK_STATE_IDLE;
	signal call_flags : std_logic_vector(31 downto 0) := (others => '0');
	signal call_input_len : std_logic_vector(31 downto 0) := (others => '0');
	signal call_output_cap : std_logic_vector(31 downto 0) := (others => '0');
	signal call_output_len : std_logic_vector(31 downto 0) := (others => '0');
	signal call_error : std_logic_vector(31 downto 0) := (others => '0');
	signal call_input_words : call_data_arr_t := (others => (others => '0'));
	signal call_output_words : call_data_arr_t := (others => (others => '0'));
	signal call_pending : std_logic := '0';
	signal call_active : std_logic := '0';
	signal call_active_function : std_logic_vector(15 downto 0) := (others => '0');
	signal call_active_output_bytes : std_logic_vector(15 downto 0) := (others => '0');
	signal call_retire_count : unsigned(31 downto 0) := (others => '0');
	signal function_start : std_logic := '0';
	signal function_input_data : std_logic_vector(767 downto 0);
	signal function_output_data : std_logic_vector(767 downto 0);
	signal function_required_input_bytes : std_logic_vector(15 downto 0);
	signal function_output_bytes : std_logic_vector(15 downto 0);
	signal function_next_led : std_logic_vector(4 downto 0);
	signal function_valid : std_logic;
	signal function_required_input_bytes_q : std_logic_vector(15 downto 0) := (others => '0');
	signal function_output_bytes_q : std_logic_vector(15 downto 0) := (others => '0');
	signal function_valid_q : std_logic := '0';
	signal function_busy : std_logic;
	signal function_done : std_logic;
	signal function_error : std_logic;
	signal firmware_manifest_word : std_logic_vector(31 downto 0);
	signal stream_write : std_logic := '0';
	signal stream_write_addr_dw : std_logic_vector(16 downto 0) := (others => '0');
	signal stream_write_data : std_logic_vector(31 downto 0) := (others => '0');
	signal stream_start : std_logic := '0';
	signal stream_control : std_logic_vector(31 downto 0) := (others => '0');
	signal stream_row : std_logic_vector(31 downto 0) := (others => '0');
	signal stream_state : std_logic_vector(31 downto 0) := STREAM_STATE_IDLE;
	signal stream_error_code : std_logic_vector(31 downto 0) := (others => '0');
	signal stream_gate_q30 : std_logic_vector(63 downto 0) := (others => '0');
	signal stream_up_q30 : std_logic_vector(63 downto 0) := (others => '0');
	signal stream_result_q30 : std_logic_vector(63 downto 0) := (others => '0');
	signal stream_completion_count : unsigned(31 downto 0) := (others => '0');
	signal stream_engine_busy : std_logic;
	signal stream_engine_done : std_logic;
	signal stream_engine_error : std_logic;
	signal stream_engine_error_code : std_logic_vector(31 downto 0);
	signal stream_engine_gate_q30 : std_logic_vector(63 downto 0);
	signal stream_engine_up_q30 : std_logic_vector(63 downto 0);
	signal stream_engine_result_q30 : std_logic_vector(63 downto 0);
	signal stream_accepted_write_count : std_logic_vector(31 downto 0);
	signal lfm25_decode_capability_magic : std_logic_vector(31 downto 0);
	signal lfm25_decode_capability_bits : std_logic_vector(31 downto 0);
	signal lfm25_decode_command : std_logic_vector(31 downto 0) := (others => '0');
	signal lfm25_decode_position : std_logic_vector(31 downto 0) := (others => '0');
	signal lfm25_decode_session_epoch : std_logic_vector(31 downto 0) := (others => '0');
	signal lfm25_decode_doorbell : std_logic := '0';
	signal lfm25_decode_doorbell_value : std_logic_vector(31 downto 0) := (others => '0');
	signal lfm25_decode_state : std_logic_vector(31 downto 0);
	signal lfm25_decode_result0 : std_logic_vector(31 downto 0);
	signal lfm25_decode_result1 : std_logic_vector(31 downto 0);
	signal lfm25_decode_argmax_score_q30 : std_logic_vector(63 downto 0);
	signal tx_pending : std_logic := '0';
	signal tx_pending_data : std_logic_vector(255 downto 0) := (others => '0');
	signal tx_pending_valid : std_logic_vector(7 downto 0) := (others => '0');
	signal tx_pending_sop : std_logic := '0';
	signal tx_pending_eop : std_logic := '0';

	-- Admit one receive beat into a registered snapshot and hold RX_WAIT while
	-- that beat is compacted.  A TLP may span multiple beats: SOP starts the
	-- packet, EOP hands the accumulated protocol dwords to the decoder, and a
	-- continuation beat is accepted after the compactor releases RX_WAIT.
	signal capture_pending : std_logic := '0';
	signal rx_snapshot_data : std_logic_vector(255 downto 0) := (others => '0');
	signal rx_snapshot_valid : std_logic_vector(7 downto 0) := (others => '0');
	signal rx_snapshot_bardec : std_logic_vector(5 downto 0) := (others => '0');
	signal rx_snapshot_err : std_logic_vector(7 downto 0) := (others => '0');
	signal rx_snapshot_sop : std_logic := '0';
	signal rx_snapshot_eop : std_logic := '0';
	signal rx_packet_active : std_logic := '0';
	signal rx_packet_bardec : std_logic_vector(5 downto 0) := (others => '0');
	signal pkt_cnt_rev   : unsigned(4 downto 0) := (others => '0');
	signal pkt_words_rev : word_arr_t := (others => (others => '0'));
	signal continuation_pending : std_logic := '0';
	signal continuation_data : std_logic_vector(255 downto 0) := (others => '0');
	signal continuation_valid : std_logic_vector(7 downto 0) := (others => '0');
	signal continuation_eop : std_logic := '0';
	signal continuation_lane : unsigned(2 downto 0) := (others => '0');
	signal decode_pending : std_logic := '0';
	signal transaction_pending : std_logic := '0';
	signal transaction_write : std_logic := '0';
	signal transaction_read : std_logic := '0';
	signal transaction_addr_dw : std_logic_vector(16 downto 0) := (others => '0');
	signal transaction_bardec : std_logic_vector(5 downto 0) := (others => '0');
	signal transaction_payload_dw : std_logic_vector(31 downto 0) := (others => '0');
	signal transaction_payload_dw1 : std_logic_vector(31 downto 0) := (others => '0');
	signal transaction_write_count : std_logic_vector(1 downto 0) := (others => '0');
	signal transaction_req_id : std_logic_vector(15 downto 0) := (others => '0');
	signal transaction_req_tag : std_logic_vector(7 downto 0) := (others => '0');
	signal stream_write_pending : std_logic := '0';
	signal stream_write_pending_addr_dw : std_logic_vector(16 downto 0) := (others => '0');
	signal stream_write_pending_data : std_logic_vector(31 downto 0) := (others => '0');
	-- BAR reads cross two explicit timing boundaries before a completion is
	-- presented to the PCIe core.  The first stage reduces the address to a
	-- small bank and bank-local index.  The second stage selects only within
	-- that bank.  This prevents the full BAR register file and debug surface
	-- from becoming one address-to-TX priority chain.
	signal bar_read_select_pending : std_logic := '0';
	signal bar_read_data_select_pending : std_logic := '0';
	signal bar_read_completion_pending : std_logic := '0';
	signal bar_read_bank : std_logic_vector(2 downto 0) := BAR_READ_BANK_NONE;
	signal bar_read_selected_bank : std_logic_vector(2 downto 0) := BAR_READ_BANK_NONE;
	signal bar_read_word_index : std_logic_vector(5 downto 0) := (others => '0');
	signal bar_read_addr_dw : std_logic_vector(9 downto 0) := (others => '0');
	signal bar_read_req_id : std_logic_vector(15 downto 0) := (others => '0');
	signal bar_read_req_tag : std_logic_vector(7 downto 0) := (others => '0');
	signal bar_read_control_data_dw : std_logic_vector(31 downto 0) := (others => '0');
	signal bar_read_stream_data_dw : std_logic_vector(31 downto 0) := (others => '0');
	signal bar_read_debug_data_dw : std_logic_vector(31 downto 0) := (others => '0');
	signal bar_read_call_header_data_dw : std_logic_vector(31 downto 0) := (others => '0');
	signal bar_read_call_input_data_dw : std_logic_vector(31 downto 0) := (others => '0');
	signal bar_read_call_output_data_dw : std_logic_vector(31 downto 0) := (others => '0');
	signal bar_read_manifest_data_dw : std_logic_vector(31 downto 0) := (others => '0');
	signal bar_read_data_dw : std_logic_vector(31 downto 0) := (others => '0');
	signal tl_rx_backpressure : std_logic;
	signal rx_nonposted_busy : std_logic := '0';
	signal tl_rx_masknp : std_logic;

	-- Gowin Analyzer probe surface for first BAR read-completion bring-up.
	-- Pulses show the live cycle; sticky bits survive long enough to inspect.
	signal dbg_rx_bar0_eop      : std_logic := '0';
	signal dbg_hit_write        : std_logic := '0';
	signal dbg_hit_read         : std_logic := '0';
	signal dbg_magic_read       : std_logic := '0';
	signal dbg_queue_cpld       : std_logic := '0';
	signal dbg_tx_fire          : std_logic := '0';
	signal dbg_cpld_blocked     : std_logic := '0';
	signal dbg_seen_rx_bar0_eop : std_logic := '0';
	signal dbg_seen_hit_write   : std_logic := '0';
	signal dbg_seen_hit_read    : std_logic := '0';
	signal dbg_seen_magic_read  : std_logic := '0';
	signal dbg_seen_queue_cpld  : std_logic := '0';
	signal dbg_seen_tx_fire     : std_logic := '0';
	signal dbg_last_addr_dw     : std_logic_vector(9 downto 0) := (others => '0');
	signal dbg_last_payload_dw  : std_logic_vector(31 downto 0) := (others => '0');
	signal dbg_last_req_id      : std_logic_vector(15 downto 0) := (others => '0');
	signal dbg_last_req_tag     : std_logic_vector(7 downto 0) := (others => '0');
	signal dbg_last_read_data   : std_logic_vector(31 downto 0) := (others => '0');
	signal dbg_last_rx_rev_dw0  : std_logic_vector(31 downto 0) := (others => '0');
	signal dbg_last_rx_rev_dw1  : std_logic_vector(31 downto 0) := (others => '0');
	signal dbg_last_rx_rev_dw2  : std_logic_vector(31 downto 0) := (others => '0');
	signal dbg_last_rx_rev_dw3  : std_logic_vector(31 downto 0) := (others => '0');
	signal dbg_last_cpld_dw0    : std_logic_vector(31 downto 0) := (others => '0');
	signal dbg_last_cpld_dw1    : std_logic_vector(31 downto 0) := (others => '0');
	signal dbg_last_cpld_dw2    : std_logic_vector(31 downto 0) := (others => '0');
	signal dbg_last_cpld_data   : std_logic_vector(31 downto 0) := (others => '0');
	signal dbg_rx_capture_count : unsigned(31 downto 0) := (others => '0');
	signal dbg_write_count : unsigned(31 downto 0) := (others => '0');
	signal dbg_word30_write_count : unsigned(31 downto 0) := (others => '0');
	signal dbg_word30_last_payload : std_logic_vector(31 downto 0) := (others => '0');
	signal dbg_rx_error_count : unsigned(31 downto 0) := (others => '0');

	attribute syn_keep : boolean;
	-- Keep the receive snapshot as a physical timing boundary before the variable
	-- valid-lane compactor on the 100 MHz transaction clock.
	attribute syn_keep of capture_pending   : signal is true;
	attribute syn_keep of rx_snapshot_data  : signal is true;
	attribute syn_keep of rx_snapshot_valid : signal is true;
	attribute syn_keep of dbg_rx_bar0_eop      : signal is true;
	attribute syn_keep of dbg_hit_write        : signal is true;
	attribute syn_keep of dbg_hit_read         : signal is true;
	attribute syn_keep of dbg_magic_read       : signal is true;
	attribute syn_keep of dbg_queue_cpld       : signal is true;
	attribute syn_keep of dbg_tx_fire          : signal is true;
	attribute syn_keep of dbg_cpld_blocked     : signal is true;
	attribute syn_keep of dbg_seen_rx_bar0_eop : signal is true;
	attribute syn_keep of dbg_seen_hit_write   : signal is true;
	attribute syn_keep of dbg_seen_hit_read    : signal is true;
	attribute syn_keep of dbg_seen_magic_read  : signal is true;
	attribute syn_keep of dbg_seen_queue_cpld  : signal is true;
	attribute syn_keep of dbg_seen_tx_fire     : signal is true;
	attribute syn_keep of dbg_last_addr_dw     : signal is true;
	attribute syn_keep of dbg_last_payload_dw  : signal is true;
	attribute syn_keep of dbg_last_req_id      : signal is true;
	attribute syn_keep of dbg_last_req_tag     : signal is true;
	attribute syn_keep of dbg_last_read_data   : signal is true;
	attribute syn_keep of dbg_last_rx_rev_dw0  : signal is true;
	attribute syn_keep of dbg_last_rx_rev_dw1  : signal is true;
	attribute syn_keep of dbg_last_rx_rev_dw2  : signal is true;
	attribute syn_keep of dbg_last_rx_rev_dw3  : signal is true;
	attribute syn_keep of dbg_last_cpld_dw0    : signal is true;
	attribute syn_keep of dbg_last_cpld_dw1    : signal is true;
	attribute syn_keep of dbg_last_cpld_dw2    : signal is true;
	attribute syn_keep of dbg_last_cpld_data   : signal is true;

	function byte_swap32(x : std_logic_vector(31 downto 0)) return std_logic_vector is
	begin
		-- Gowin's TLP user bus exposes header dwords in protocol bit order but
		-- payload dwords in PCIe byte-lane order. Convert the payload boundary so
		-- every BAR register remains a normal host-native little-endian u32.
		return x(7 downto 0) & x(15 downto 8) & x(23 downto 16) & x(31 downto 24);
	end function;

	function payload_byte(x : std_logic_vector(31 downto 0)) return byte_t is
	begin
		-- Nominal path: host writes u32 values 0..15, i.e. value in byte0.
		if x(31 downto 8) = x"000000" then
			return x(7 downto 0);
		end if;

		-- Fallback for observed swapped captures.
		if x(23 downto 0) = x"000000" then
			return x(31 downto 24);
		end if;

		return x(7 downto 0);
	end function;

	function make_seen_word(
		rx_bar0_eop : std_logic;
		hit_write : std_logic;
		hit_read : std_logic;
		magic_read : std_logic;
		queue_cpld : std_logic;
		tx_fire : std_logic;
		cpld_blocked : std_logic;
		linkup : std_logic
	) return std_logic_vector is
		variable seen_word : std_logic_vector(31 downto 0) := (others => '0');
	begin
		seen_word(0) := rx_bar0_eop;
		seen_word(1) := hit_write;
		seen_word(2) := hit_read;
		seen_word(3) := magic_read;
		seen_word(4) := queue_cpld;
		seen_word(5) := tx_fire;
		seen_word(6) := cpld_blocked;
		seen_word(7) := linkup;
		return seen_word;
	end function;

begin
	-- PERST is already high during a live SRAM reload. Do not release the newly
	-- configured PCIe hard block on the same edge that the local PLL first reports
	-- lock: cold boot happens to receive a long host reset, while live reload would
	-- otherwise receive none. Assert reset asynchronously and release it only after
	-- 65,536 stable 100 MHz TLP clocks (~0.65 ms).
	process(tlp_clk, pcie_perst_n, pll_lock)
	begin
		if (pcie_perst_n = '0') or (pll_lock = '0') then
			pcie_reset_hold <= (others => '0');
			pcie_core_reset_n <= '0';
		elsif rising_edge(tlp_clk) then
			if pcie_reset_hold /= x"FFFF" then
				pcie_reset_hold <= pcie_reset_hold + 1;
				pcie_core_reset_n <= '0';
			else
				pcie_core_reset_n <= '1';
			end if;
		end if;
	end process;

	-- Register the hard-IP receive pins on every TLP clock.  Keeping these wide
	-- registers out of the transaction-state priority chain prevents the current
	-- control state from becoming their clock-enable path.  The PCIe
	-- controller holds an accepted beat while backpressure is asserted, so the
	-- main state machine can consume this snapshot on the following cycle.
	process(tlp_clk)
	begin
		if rising_edge(tlp_clk) then
			if (pcie_perst_n = '0') or (pll_lock = '0') then
				rx_snapshot_data <= (others => '0');
				rx_snapshot_valid <= (others => '0');
				rx_snapshot_bardec <= (others => '0');
				rx_snapshot_err <= (others => '0');
				rx_snapshot_sop <= '0';
				rx_snapshot_eop <= '0';
			else
				rx_snapshot_data <= tl_rx_data;
				rx_snapshot_valid <= tl_rx_valid;
				rx_snapshot_bardec <= tl_rx_bardec;
				rx_snapshot_err <= tl_rx_err;
				rx_snapshot_sop <= tl_rx_sop;
				rx_snapshot_eop <= tl_rx_eop;
			end if;
		end if;
	end process;
	-- Present a queued completion continuously. The controller samples the beat
	-- on a rising TLP clock edge where VALID is asserted and WAIT is deasserted;
	-- therefore VALID/data must not be pulsed based on the previous WAIT value.
	tl_tx_data <= tx_pending_data when tx_pending = '1' else (others => '0');
	tl_tx_valid <= tx_pending_valid when tx_pending = '1' else (others => '0');
	tl_tx_sop <= tx_pending_sop when tx_pending = '1' else '0';
	tl_tx_eop <= tx_pending_eop when tx_pending = '1' else '0';
	-- The controller holds its current RX beat while this is high. Keep one
	-- transaction in flight all the way through a read completion; this removes
	-- RX-buffer reuse from the correctness boundary of the inline BAR protocol.
	tl_rx_backpressure <= capture_pending or decode_pending or
		transaction_pending or rx_nonposted_busy;
	-- A BAR read is a non-posted request: keep later non-posted requests out of
	-- the controller until our completion has actually entered its TX buffer.
	-- Include the live SOP cycle so the mask is visible when the first request
	-- is accepted, as required by the Gowin transaction-layer handshake.
	tl_rx_masknp <= rx_nonposted_busy or
		(tl_rx_sop and tl_rx_eop and pcie_linkup and tl_rx_bardec(0));

	u_clock: Truega_Pcie_Clock
		port map(
			clkin   => clk,
			tlp_clk => tlp_clk,
			lock    => pll_lock
		);

	gen_function_input: for i in 0 to 23 generate
		function_input_data((i + 1) * 32 - 1 downto i * 32) <= call_input_words(i);
	end generate;

	u_functions: truega_functions
		port map(
			clk                  => tlp_clk,
			reset_n              => pcie_core_reset_n,
			start                => function_start,
			function_id          => call_abi_function(31 downto 16),
			input_data           => function_input_data,
			led_state            => led_reg,
			next_led             => function_next_led,
			output_data          => function_output_data,
			required_input_bytes => function_required_input_bytes,
			output_bytes         => function_output_bytes,
			valid                => function_valid,
			busy                 => function_busy,
			done                 => function_done,
			error                => function_error
		);

	u_firmware_manifest: truega_firmware_manifest
		port map(
			word_index => bar_read_word_index(4 downto 0),
			data       => firmware_manifest_word
			);

	u_lfm25_row_streamer: truega_lfm25_row_streamer
		port map(
			clk                    => tlp_clk,
			reset_n                => pcie_core_reset_n,
			write_i                => stream_write,
			write_addr_dw_i        => stream_write_addr_dw,
			write_data_i           => stream_write_data,
			start_i                => stream_start,
			mode_i                 => stream_control(1 downto 0),
			busy_o                 => stream_engine_busy,
			done_o                 => stream_engine_done,
			error_o                => stream_engine_error,
			error_code_o           => stream_engine_error_code,
			gate_q30_o             => stream_engine_gate_q30,
			up_q30_o               => stream_engine_up_q30,
			result_q30_o           => stream_engine_result_q30,
			accepted_write_count_o => stream_accepted_write_count
		);

	u_lfm25_decode_dispatch: truega_lfm25_decode_dispatch
		generic map(
			ENABLE => 0
		)
		port map(
			clk                       => tlp_clk,
			reset_n                   => pcie_core_reset_n,
			command_i                 => lfm25_decode_command,
			position_i                => lfm25_decode_position,
			session_epoch_i           => lfm25_decode_session_epoch,
			doorbell_i                => lfm25_decode_doorbell,
			doorbell_value_i          => lfm25_decode_doorbell_value,
			capability_magic_o        => lfm25_decode_capability_magic,
			capability_bits_o         => lfm25_decode_capability_bits,
			state_o                   => lfm25_decode_state,
			result0_o                 => lfm25_decode_result0,
			result1_o                 => lfm25_decode_result1,
			argmax_score_q30_o        => lfm25_decode_argmax_score_q30,
			execute_start_o           => open,
			execute_operation_o       => open,
			execute_layer_o           => open,
			execute_position_o        => open,
			execute_input_slot_o      => open,
			execute_residual_slot_o   => open,
			execute_session_epoch_o   => open,
			execute_session_begin_o   => open,
			engine_done_i             => '0',
			engine_error_i            => '0',
			engine_error_code_i       => (others => '0'),
			engine_result_slot_i      => (others => '0'),
			engine_result_position_i  => (others => '0'),
			engine_argmax_token_i     => (others => '0'),
			engine_argmax_rows_i      => (others => '0'),
			engine_argmax_score_q30_i => (others => '0'),
			retire_o                  => decode_irq_retire
		);

	u_completion_irq: truega_completion_irq
		port map(
			clk                => tlp_clk,
			reset_n            => pcie_core_reset_n,
			retire_i           => call_irq_retire or decode_irq_retire,
			interrupt_enable_i => call_flags(0) or stream_irq_enable or decode_irq_enable,
			bar_ack_i          => call_irq_bar_ack,
			controller_ack_i   => call_irq_controller_ack,
			status_o           => call_irq_status,
			request_o          => call_irq_request,
			msinum_o           => call_irq_msinum
		);

	u_serdes: SerDes_Top
		port map(
			PCIE_Controller_Top_pcie_tl_rx_sop_o        => tl_rx_sop,
			PCIE_Controller_Top_pcie_tl_rx_eop_o        => tl_rx_eop,
			PCIE_Controller_Top_pcie_tl_rx_data_o       => tl_rx_data,
			PCIE_Controller_Top_pcie_tl_rx_valid_o      => tl_rx_valid,
			PCIE_Controller_Top_pcie_tl_rx_bardec_o     => tl_rx_bardec,
				PCIE_Controller_Top_pcie_tl_rx_err_o        => tl_rx_err,
				PCIE_Controller_Top_pcie_tl_tx_wait_o       => tl_tx_wait,
				PCIE_Controller_Top_pcie_tl_int_ack_o       => call_irq_controller_ack,
			PCIE_Controller_Top_pcie_ltssm_o            => open,
			PCIE_Controller_Top_pcie_tl_tx_creditsp_o   => open,
			PCIE_Controller_Top_pcie_tl_tx_creditsnp_o  => open,
			PCIE_Controller_Top_pcie_tl_tx_creditscpl_o => open,
			PCIE_Controller_Top_pcie_tl_cfg_busdev_o    => tl_cfg_busdev,
			PCIE_Controller_Top_pcie_linkup_o           => pcie_linkup,
			PCIE_Controller_Top_pcie_tl_drp_clk_o       => open,
			PCIE_Controller_Top_pcie_tl_drp_rddata_o    => open,
			PCIE_Controller_Top_pcie_tl_drp_resp_o      => open,
			PCIE_Controller_Top_pcie_tl_drp_rd_valid_o  => open,
			PCIE_Controller_Top_pcie_tl_drp_ready_o     => open,

			debug_refclk_det_o => open,
			debug_rx_lock_o    => open,

			PCIE_Controller_Top_pcie_rstn_i          => pcie_core_reset_n,
			PCIE_Controller_Top_pcie_tl_clk_i        => tlp_clk,
			PCIE_Controller_Top_pcie_tl_rx_wait_i    => tl_rx_backpressure,
			PCIE_Controller_Top_pcie_tl_rx_masknp_i  => tl_rx_masknp,
			PCIE_Controller_Top_pcie_tl_tx_sop_i     => tl_tx_sop,
			PCIE_Controller_Top_pcie_tl_tx_eop_i     => tl_tx_eop,
				PCIE_Controller_Top_pcie_tl_tx_data_i    => tl_tx_data,
				PCIE_Controller_Top_pcie_tl_tx_valid_i   => tl_tx_valid,
				PCIE_Controller_Top_pcie_tl_int_status_i => call_irq_status,
				PCIE_Controller_Top_pcie_tl_int_req_i    => call_irq_request,
				PCIE_Controller_Top_pcie_tl_int_msinum_i => call_irq_msinum,
			PCIE_Controller_Top_pcie_tl_drp_addr_i   => (others => '0'),
			PCIE_Controller_Top_pcie_tl_drp_wrdata_i => (others => '0'),
			PCIE_Controller_Top_pcie_tl_drp_strb_i   => (others => '0'),
			PCIE_Controller_Top_pcie_tl_drp_wr_i     => '0',
			PCIE_Controller_Top_pcie_tl_drp_rd_i     => '0'
		);

	process(tlp_clk)
		variable dw : word_arr_t;
		variable next_words_rev : word_arr_t;
		variable next_cnt_rev : integer range 0 to PKT_MAX_WORDS;
		variable hit_write : boolean;
		variable hit_read : boolean;
		variable val8 : byte_t;
		variable addr_dw : std_logic_vector(16 downto 0);
		variable payload_dw : std_logic_vector(31 downto 0);
		variable payload_dw1 : std_logic_vector(31 downto 0);
		variable write_count : std_logic_vector(1 downto 0);
		variable req_id : std_logic_vector(15 downto 0);
		variable req_tag : std_logic_vector(7 downto 0);
		variable read_data_dw : std_logic_vector(31 downto 0);
		variable addr_index : integer range 0 to 131071;

		procedure clear_words(variable words : inout word_arr_t) is
		begin
			for i in 0 to PKT_MAX_WORDS - 1 loop
				words(i) := (others => '0');
			end loop;
		end procedure;

		procedure decode_words(
			constant words : in word_arr_t;
			constant count : in integer;
			variable found_write : out boolean;
			variable found_read : out boolean;
			variable addr_out : out std_logic_vector(16 downto 0);
			variable payload_out : out std_logic_vector(31 downto 0);
			variable payload1_out : out std_logic_vector(31 downto 0);
			variable write_count_out : out std_logic_vector(1 downto 0);
			variable req_id_out : out std_logic_vector(15 downto 0);
			variable req_tag_out : out std_logic_vector(7 downto 0)
		) is
			variable hdr : std_logic_vector(31 downto 0);
			variable fmt_type : std_logic_vector(7 downto 0);
			variable addr_low : std_logic_vector(31 downto 0);
			variable payload : std_logic_vector(31 downto 0);
			variable req_hdr : std_logic_vector(31 downto 0);
			variable addr_idx : integer;
			variable payload_idx : integer;
		begin
			found_write := false;
			found_read := false;
			addr_out := (others => '0');
			payload_out := (others => '0');
			payload1_out := (others => '0');
			write_count_out := "00";
			req_id_out := (others => '0');
			req_tag_out := (others => '0');

			-- SOP and descending-lane compaction guarantee that the first protocol
			-- dword is word zero. Searching every array position creates a deep
			-- priority chain for no supported TLP case.
			if count = 0 then
				return;
			end if;
			hdr := words(0);
			fmt_type := hdr(31 downto 24);

			if (fmt_type = x"40") or (fmt_type = x"60") then
				if (hdr(9 downto 0) = "0000000001")
					or (hdr(9 downto 0) = "0000000010") then
					if fmt_type = x"40" then
						addr_idx := 2;
						payload_idx := 3;
					else
						addr_idx := 3;
						payload_idx := 4;
					end if;

					if (addr_idx < count) and (payload_idx < count)
						and ((hdr(9 downto 0) = "0000000001")
							or (payload_idx + 1 < count)) then
						addr_low := words(addr_idx);
						payload := words(payload_idx);
						addr_out := addr_low(18 downto 2);
						payload_out := byte_swap32(payload);
						if hdr(9 downto 0) = "0000000010" then
							payload1_out := byte_swap32(words(payload_idx + 1));
							write_count_out := "10";
						else
							write_count_out := "01";
						end if;
						found_write := true;
					end if;
				end if;
			elsif (fmt_type = x"00") or (fmt_type = x"20") then
				if hdr(9 downto 0) = "0000000001" then
					if fmt_type = x"00" then
						addr_idx := 2;
					else
						addr_idx := 3;
					end if;

					if (1 < count) and (addr_idx < count) then
						req_hdr := words(1);
						addr_low := words(addr_idx);
						addr_out := addr_low(18 downto 2);
						req_id_out := req_hdr(31 downto 16);
						req_tag_out := req_hdr(15 downto 8);
						found_read := true;
					end if;
				end if;
			end if;
		end procedure;

		procedure queue_cpld(
			constant req_id_in : in std_logic_vector(15 downto 0);
			constant req_tag_in : in std_logic_vector(7 downto 0);
			constant addr_in : in std_logic_vector(9 downto 0);
			constant data_in : in std_logic_vector(31 downto 0)
		) is
			variable dw0 : std_logic_vector(31 downto 0);
			variable dw1 : std_logic_vector(31 downto 0);
			variable dw2 : std_logic_vector(31 downto 0);
		begin
			dw0 := (others => '0');
			dw1 := (others => '0');
			dw2 := (others => '0');
				dw0(31 downto 24) := x"4A";
				dw0(9 downto 0) := "0000000001";
				-- Gowin reports {Bus[7:0], Device[4:0]}. The PCIe Completer ID
				-- is {Bus, Device, Function}, and this endpoint is function zero.
				dw1(31 downto 16) := tl_cfg_busdev & "000";
				dw1(11 downto 0) := std_logic_vector(to_unsigned(4, 12));
				dw2(31 downto 16) := req_id_in;
				dw2(15 downto 8) := req_tag_in;
				dw2(7) := '0';
				dw2(6 downto 0) := addr_in(4 downto 0) & "00";

				dbg_last_cpld_dw0 <= dw0;
				dbg_last_cpld_dw1 <= dw1;
				dbg_last_cpld_dw2 <= dw2;
				dbg_last_cpld_data <= data_in;

			tx_pending_data <= (others => '0');
			-- The Gowin TL bus carries the first dword in [255:224] and then
			-- descends through the vector. A four-dword CplD therefore occupies
			-- the high half of the beat and uses valid=F0, matching IPUG1020
			-- Figure 3-1. Using the low half makes the controller see the payload
			-- as the first dword and discard the malformed completion.
			tx_pending_data(255 downto 224) <= dw0;
			tx_pending_data(223 downto 192) <= dw1;
			tx_pending_data(191 downto 160) <= dw2;
			tx_pending_data(159 downto 128) <= byte_swap32(data_in);
			tx_pending_valid <= "11110000";
			tx_pending_sop <= '1';
			tx_pending_eop <= '1';
			tx_pending <= '1';
	end procedure;
	begin
		if rising_edge(tlp_clk) then
			if (pcie_perst_n = '0') or (pll_lock = '0') then
				led_reg <= "00001";
				debug_led_mode <= '0';
				call_magic <= WORK_PACKAGE_MAGIC;
				call_abi_function <= x"0000" & WORK_ABI_VERSION;
				call_id_low <= (others => '0');
				call_id_high <= (others => '0');
				call_state <= WORK_STATE_IDLE;
				call_flags <= (others => '0');
				call_input_len <= (others => '0');
				call_output_cap <= (others => '0');
				call_output_len <= (others => '0');
				call_error <= (others => '0');
				call_input_words <= (others => (others => '0'));
				call_output_words <= (others => (others => '0'));
				call_pending <= '0';
				call_active <= '0';
					call_active_function <= (others => '0');
					call_active_output_bytes <= (others => '0');
					function_required_input_bytes_q <= (others => '0');
					function_output_bytes_q <= (others => '0');
					function_valid_q <= '0';
					function_start <= '0';
					call_irq_retire <= '0';
					call_irq_bar_ack <= '0';
					call_retire_count <= (others => '0');
					call_irq_request_prev <= '0';
					call_irq_controller_ack_prev <= '0';
					call_irq_request_count <= (others => '0');
					call_irq_controller_ack_count <= (others => '0');
					stream_irq_enable <= '0';
					decode_irq_enable <= '0';
					lfm25_decode_command <= (others => '0');
					lfm25_decode_position <= (others => '0');
					lfm25_decode_session_epoch <= (others => '0');
					lfm25_decode_doorbell <= '0';
					lfm25_decode_doorbell_value <= (others => '0');
					stream_write <= '0';
					stream_write_addr_dw <= (others => '0');
					stream_write_data <= (others => '0');
					stream_start <= '0';
					stream_control <= (others => '0');
					stream_row <= (others => '0');
					stream_state <= STREAM_STATE_IDLE;
					stream_error_code <= (others => '0');
					stream_gate_q30 <= (others => '0');
					stream_up_q30 <= (others => '0');
					stream_result_q30 <= (others => '0');
					stream_completion_count <= (others => '0');
				capture_pending <= '0';
				rx_packet_active <= '0';
				rx_packet_bardec <= (others => '0');
				pkt_cnt_rev <= (others => '0');
				pkt_words_rev <= (others => (others => '0'));
				continuation_pending <= '0';
				continuation_data <= (others => '0');
				continuation_valid <= (others => '0');
				continuation_eop <= '0';
				continuation_lane <= (others => '0');
				decode_pending <= '0';
				transaction_pending <= '0';
				transaction_write <= '0';
				transaction_read <= '0';
				transaction_addr_dw <= (others => '0');
				transaction_bardec <= (others => '0');
				transaction_payload_dw <= (others => '0');
				transaction_payload_dw1 <= (others => '0');
				transaction_write_count <= (others => '0');
				transaction_req_id <= (others => '0');
				transaction_req_tag <= (others => '0');
				stream_write_pending <= '0';
				stream_write_pending_addr_dw <= (others => '0');
				stream_write_pending_data <= (others => '0');
				bar_read_select_pending <= '0';
				bar_read_data_select_pending <= '0';
				bar_read_completion_pending <= '0';
				bar_read_bank <= BAR_READ_BANK_NONE;
				bar_read_selected_bank <= BAR_READ_BANK_NONE;
				bar_read_word_index <= (others => '0');
				bar_read_addr_dw <= (others => '0');
				bar_read_req_id <= (others => '0');
				bar_read_req_tag <= (others => '0');
				bar_read_control_data_dw <= (others => '0');
				bar_read_stream_data_dw <= (others => '0');
				bar_read_debug_data_dw <= (others => '0');
				bar_read_call_header_data_dw <= (others => '0');
				bar_read_call_input_data_dw <= (others => '0');
				bar_read_call_output_data_dw <= (others => '0');
				bar_read_manifest_data_dw <= (others => '0');
				bar_read_data_dw <= (others => '0');
				rx_nonposted_busy <= '0';
					tx_pending <= '0';
					tx_pending_data <= (others => '0');
				tx_pending_valid <= (others => '0');
				tx_pending_sop <= '0';
				tx_pending_eop <= '0';
				dbg_rx_bar0_eop <= '0';
					dbg_hit_write <= '0';
					dbg_hit_read <= '0';
					dbg_magic_read <= '0';
					dbg_queue_cpld <= '0';
				dbg_tx_fire <= '0';
				dbg_cpld_blocked <= '0';

					dbg_seen_rx_bar0_eop <= '0';
					dbg_seen_hit_write <= '0';
					dbg_seen_hit_read <= '0';
					dbg_seen_magic_read <= '0';
					dbg_seen_queue_cpld <= '0';
					dbg_seen_tx_fire <= '0';
					dbg_last_addr_dw <= (others => '0');
					dbg_last_payload_dw <= (others => '0');
					dbg_last_req_id <= (others => '0');
					dbg_last_req_tag <= (others => '0');
					dbg_last_read_data <= (others => '0');
					dbg_last_rx_rev_dw0 <= (others => '0');
					dbg_last_rx_rev_dw1 <= (others => '0');
					dbg_last_rx_rev_dw2 <= (others => '0');
					dbg_last_rx_rev_dw3 <= (others => '0');
					dbg_last_cpld_dw0 <= (others => '0');
					dbg_last_cpld_dw1 <= (others => '0');
					dbg_last_cpld_dw2 <= (others => '0');
					dbg_last_cpld_data <= (others => '0');
					dbg_rx_capture_count <= (others => '0');
					dbg_write_count <= (others => '0');
					dbg_word30_write_count <= (others => '0');
					dbg_word30_last_payload <= (others => '0');
					dbg_rx_error_count <= (others => '0');
				else
					-- Slot metadata is registered before package validation. The function
					-- selector is stable for many clocks between its BAR write and the
					-- doorbell, so this removes a long selector-to-error path without
					-- changing the accepted call or adding another protocol phase.
					function_required_input_bytes_q <= function_required_input_bytes;
					function_output_bytes_q <= function_output_bytes;
					function_valid_q <= function_valid;
					function_start <= '0';
					call_irq_retire <= '0';
					call_irq_bar_ack <= '0';
					stream_write <= '0';
					stream_start <= '0';
					lfm25_decode_doorbell <= '0';
					if decode_irq_retire = '1' then
						call_retire_count <= call_retire_count + 1;
					end if;
					if (call_irq_request = '1') and (call_irq_request_prev = '0') then
						call_irq_request_count <= call_irq_request_count + 1;
					end if;
					if (call_irq_controller_ack = '1') and (call_irq_controller_ack_prev = '0') then
						call_irq_controller_ack_count <= call_irq_controller_ack_count + 1;
					end if;
					call_irq_request_prev <= call_irq_request;
					call_irq_controller_ack_prev <= call_irq_controller_ack;
					dbg_rx_bar0_eop <= '0';
					dbg_hit_write <= '0';
					dbg_hit_read <= '0';
					dbg_magic_read <= '0';
					dbg_queue_cpld <= '0';
					dbg_tx_fire <= '0';
					dbg_cpld_blocked <= '0';

					if stream_engine_done = '1' then
						stream_gate_q30 <= stream_engine_gate_q30;
						stream_up_q30 <= stream_engine_up_q30;
						stream_result_q30 <= stream_engine_result_q30;
						stream_error_code <= stream_engine_error_code;
						stream_completion_count <= stream_completion_count + 1;
						if stream_engine_error = '1' then
							stream_state <= STREAM_STATE_FAILED;
						else
							stream_state <= STREAM_STATE_COMPLETE;
						end if;
						call_irq_retire <= '1';
						call_retire_count <= call_retire_count + 1;
					end if;

					-- The doorbell launches one already-fused slot through a common
					-- start/busy/done contract. The shell waits for done; it does not fetch
					-- instructions or interpret a device-side command stream.
					if call_pending = '1' then
						call_output_len <= (others => '0');
						call_error <= (others => '0');
						call_output_words <= (others => (others => '0'));
						if (call_magic /= WORK_PACKAGE_MAGIC)
							or (call_abi_function(15 downto 0) /= WORK_ABI_VERSION) then
							call_error <= CALL_ERROR_BAD_PACKAGE;
							call_state <= WORK_STATE_FAILED;
							call_irq_retire <= '1';
							call_retire_count <= call_retire_count + 1;
					elsif function_valid_q = '0' then
							call_error <= CALL_ERROR_BAD_FUNCTION;
							call_state <= WORK_STATE_FAILED;
							call_irq_retire <= '1';
							call_retire_count <= call_retire_count + 1;
					elsif unsigned(call_input_len)
						/= resize(unsigned(function_required_input_bytes_q), 32) then
							call_error <= CALL_ERROR_BAD_LENGTH;
							call_state <= WORK_STATE_FAILED;
							call_irq_retire <= '1';
							call_retire_count <= call_retire_count + 1;
					elsif unsigned(call_output_cap)
						< resize(unsigned(function_output_bytes_q), 32) then
							call_error <= CALL_ERROR_BAD_LENGTH;
							call_state <= WORK_STATE_FAILED;
							call_irq_retire <= '1';
							call_retire_count <= call_retire_count + 1;
						elsif function_busy = '1' then
							call_error <= CALL_ERROR_BAD_PACKAGE;
							call_state <= WORK_STATE_FAILED;
							call_irq_retire <= '1';
							call_retire_count <= call_retire_count + 1;
						else
							function_start <= '1';
							call_active <= '1';
							call_active_function <= call_abi_function(31 downto 16);
						call_active_output_bytes <= function_output_bytes_q;
						end if;
						call_pending <= '0';
					elsif (call_active = '1') and (function_done = '1') then
						call_active <= '0';
						call_irq_retire <= '1';
						call_retire_count <= call_retire_count + 1;
						if function_error = '1' then
							call_output_len <= (others => '0');
							call_error <= CALL_ERROR_FUNCTION_FAILED;
							call_state <= WORK_STATE_FAILED;
						else
							for i in 0 to 23 loop
								call_output_words(i) <= function_output_data((i + 1) * 32 - 1 downto i * 32);
							end loop;
							call_output_len <= x"0000" & call_active_output_bytes;
							call_state <= WORK_STATE_COMPLETE;
							if call_active_function = x"0000" then
								led_reg <= function_next_led;
							end if;
						end if;
					end if;

					if (tx_pending = '1') and (tl_tx_wait = '0') then
						dbg_tx_fire <= '1';
						dbg_seen_tx_fire <= '1';
						tx_pending <= '0';
						tx_pending_data <= (others => '0');
					tx_pending_valid <= (others => '0');
					tx_pending_sop <= '0';
						tx_pending_eop <= '0';
						rx_nonposted_busy <= '0';
					end if;

				next_cnt_rev := to_integer(pkt_cnt_rev);
				next_words_rev := pkt_words_rev;

					-- Stage four of a BAR read: the completion fields now come only
					-- from registers.  Keep the request pending if the controller still
					-- owns the previous TX beat.
					if bar_read_completion_pending = '1' then
						if tx_pending = '0' then
							dbg_queue_cpld <= '1';
							dbg_seen_queue_cpld <= '1';
							queue_cpld(bar_read_req_id, bar_read_req_tag, bar_read_addr_dw, bar_read_data_dw);
							bar_read_completion_pending <= '0';
						else
							dbg_cpld_blocked <= '1';
						end if;
					end if;

					-- Stage three selects one of the independently registered bank
					-- results.  This is intentionally a separate boundary: otherwise
					-- synthesis can flatten all bank-local muxes back into one tree.
					if bar_read_data_select_pending = '1' then
						case bar_read_selected_bank is
						when BAR_READ_BANK_CONTROL => read_data_dw := bar_read_control_data_dw;
						when BAR_READ_BANK_STREAM => read_data_dw := bar_read_stream_data_dw;
						when BAR_READ_BANK_DEBUG => read_data_dw := bar_read_debug_data_dw;
						when BAR_READ_BANK_CALL_HEADER => read_data_dw := bar_read_call_header_data_dw;
						when BAR_READ_BANK_CALL_INPUT => read_data_dw := bar_read_call_input_data_dw;
						when BAR_READ_BANK_CALL_OUTPUT => read_data_dw := bar_read_call_output_data_dw;
						when BAR_READ_BANK_MANIFEST => read_data_dw := bar_read_manifest_data_dw;
						when others => read_data_dw := (others => '0');
						end case;
						bar_read_data_dw <= read_data_dw;
						dbg_last_read_data <= read_data_dw;
						bar_read_data_select_pending <= '0';
						bar_read_completion_pending <= '1';
					end if;

					-- Stage two selects only within one predecoded bank.  Each bank has
					-- its own destination register so the index cannot fan out through
					-- a second, full-BAR priority mux in the same clock period.
					if bar_read_select_pending = '1' then
						read_data_dw := (others => '0');
						case bar_read_bank is
						when BAR_READ_BANK_CALL_HEADER =>
							case to_integer(unsigned(bar_read_word_index)) is
							when CALL_MAGIC_WORD => read_data_dw := call_magic;
							when CALL_ABI_FUNCTION_WORD => read_data_dw := call_abi_function;
							when 2 => read_data_dw := call_id_low;
							when 3 => read_data_dw := call_id_high;
							when CALL_STATE_WORD => read_data_dw := call_state;
							when 5 => read_data_dw := call_flags;
							when CALL_INPUT_LEN_WORD => read_data_dw := call_input_len;
							when CALL_OUTPUT_CAP_WORD => read_data_dw := call_output_cap;
							when CALL_OUTPUT_LEN_WORD => read_data_dw := call_output_len;
							when CALL_ERROR_WORD => read_data_dw := call_error;
							when others => null;
							end case;
							bar_read_call_header_data_dw <= read_data_dw;
						when BAR_READ_BANK_CALL_INPUT =>
							bar_read_call_input_data_dw <= call_input_words(to_integer(unsigned(bar_read_word_index)));
						when BAR_READ_BANK_CALL_OUTPUT =>
							bar_read_call_output_data_dw <= call_output_words(to_integer(unsigned(bar_read_word_index)));
						when BAR_READ_BANK_MANIFEST =>
							read_data_dw := (others => '0');
							if to_integer(unsigned(bar_read_word_index)) < FIRMWARE_MANIFEST_WORD_COUNT then
								read_data_dw := firmware_manifest_word;
							end if;
							bar_read_manifest_data_dw <= read_data_dw;
						when BAR_READ_BANK_STREAM =>
							case to_integer(unsigned(bar_read_word_index)) is
							when 0 => read_data_dw := STREAM_CAPABILITY_MAGIC;
							when 1 => read_data_dw := stream_control;
							when 2 => read_data_dw := stream_row;
							when 4 => read_data_dw := stream_state;
							when 5 => read_data_dw := stream_gate_q30(31 downto 0);
							when 6 => read_data_dw := stream_gate_q30(63 downto 32);
							when 7 => read_data_dw := stream_up_q30(31 downto 0);
							when 8 => read_data_dw := stream_up_q30(63 downto 32);
								when 9 =>
									if (lfm25_decode_state = DECODE_STATE_COMPLETE)
										and (lfm25_decode_command(7 downto 0) = DECODE_OP_LM_HEAD_ARGMAX) then
										read_data_dw := lfm25_decode_argmax_score_q30(31 downto 0);
									else
										read_data_dw := stream_result_q30(31 downto 0);
									end if;
								when 10 =>
									if (lfm25_decode_state = DECODE_STATE_COMPLETE)
										and (lfm25_decode_command(7 downto 0) = DECODE_OP_LM_HEAD_ARGMAX) then
										read_data_dw := lfm25_decode_argmax_score_q30(63 downto 32);
									else
										read_data_dw := stream_result_q30(63 downto 32);
									end if;
								when 11 => read_data_dw := stream_error_code;
							when 12 => read_data_dw := std_logic_vector(stream_completion_count);
							when 13 => read_data_dw := stream_accepted_write_count;
							when 14 => read_data_dw := std_logic_vector(dbg_rx_capture_count);
							when 15 => read_data_dw := std_logic_vector(dbg_write_count);
								when 16 => read_data_dw := std_logic_vector(dbg_rx_error_count);
								when 17 => read_data_dw := lfm25_decode_capability_magic;
								when 18 => read_data_dw := lfm25_decode_capability_bits;
								when 19 => read_data_dw := lfm25_decode_command;
								when 20 => read_data_dw := lfm25_decode_position;
								when 21 => read_data_dw := lfm25_decode_session_epoch;
								when 22 => read_data_dw := (others => '0');
								when 23 => read_data_dw := lfm25_decode_state;
								when 24 => read_data_dw := lfm25_decode_result0;
								when 25 => read_data_dw := lfm25_decode_result1;
								when others => null;
							end case;
							bar_read_stream_data_dw <= read_data_dw;
						when BAR_READ_BANK_DEBUG =>
							case bar_read_word_index is
							when "000000" =>
								read_data_dw := make_seen_word(
									dbg_seen_rx_bar0_eop,
									dbg_seen_hit_write,
									dbg_seen_hit_read,
									dbg_seen_magic_read,
									dbg_seen_queue_cpld,
									dbg_seen_tx_fire,
									dbg_cpld_blocked,
									pcie_linkup
								);
							when "000001" => read_data_dw(9 downto 0) := dbg_last_addr_dw;
							when "000010" => read_data_dw := dbg_last_read_data;
							when "000011" =>
								read_data_dw(31 downto 16) := dbg_last_req_id;
								read_data_dw(15 downto 8) := dbg_last_req_tag;
							when "000100" => read_data_dw := dbg_last_cpld_dw0;
							when "000101" => read_data_dw := dbg_last_cpld_dw1;
							when "000110" => read_data_dw := dbg_last_cpld_dw2;
							when "000111" => read_data_dw := dbg_last_cpld_data;
							when "001000" => read_data_dw := std_logic_vector(dbg_rx_capture_count);
							when "001001" => read_data_dw := std_logic_vector(dbg_write_count);
							when "001010" => read_data_dw := std_logic_vector(dbg_word30_write_count);
							when "001011" => read_data_dw := dbg_word30_last_payload;
							when "001100" => read_data_dw := call_input_words(DEBUG_TARGET_PACKAGE_WORD - CALL_INPUT_WORD);
							when "001101" =>
								read_data_dw(7) := capture_pending;
								read_data_dw(8) := decode_pending;
								read_data_dw(9) := transaction_pending;
								read_data_dw(10) := tl_rx_backpressure;
								read_data_dw(11) := rx_nonposted_busy;
							when "001110" => read_data_dw := std_logic_vector(dbg_rx_error_count);
							when others => null;
							end case;
							bar_read_debug_data_dw <= read_data_dw;
						when BAR_READ_BANK_CONTROL =>
							case to_integer(unsigned(bar_read_word_index)) is
							when 0 => read_data_dw(4 downto 0) := led_reg;
							when 5 => read_data_dw := call_state;
							when 8 =>
								read_data_dw := PROTOCOL_MAGIC;
								dbg_magic_read <= '1';
								dbg_seen_magic_read <= '1';
							when BAR0_CALL_DOORBELL_DW =>
								read_data_dw := std_logic_vector(call_retire_count);
							when BAR0_CALL_IRQ_RETIRE_COUNT_DW =>
								read_data_dw := std_logic_vector(call_retire_count);
							when BAR0_CALL_IRQ_REQUEST_COUNT_DW =>
								read_data_dw := std_logic_vector(call_irq_request_count);
							when BAR0_CALL_IRQ_CONTROLLER_ACK_COUNT_DW =>
								read_data_dw := std_logic_vector(call_irq_controller_ack_count);
							when BAR0_CALL_IRQ_STATE_DW =>
								read_data_dw(0) := call_irq_status;
								read_data_dw(1) := call_irq_request;
								read_data_dw(2) := call_irq_controller_ack;
								read_data_dw(3) := call_flags(0);
							when others => null;
							end case;
							bar_read_control_data_dw <= read_data_dw;
						when others => null;
						end case;
						bar_read_selected_bank <= bar_read_bank;
						bar_read_select_pending <= '0';
						bar_read_data_select_pending <= '1';
					end if;

					if transaction_pending = '1' then
							hit_write := transaction_write = '1';
							hit_read := transaction_read = '1';
							addr_dw := transaction_addr_dw;
							payload_dw := transaction_payload_dw;
							req_id := transaction_req_id;
							req_tag := transaction_req_tag;
							-- BAR0 is a 1 KiB register aperture; BAR2 is a 512 KiB
							-- streaming aperture.  Decode each address relative to the
							-- BAR that the PCIe hard block reports, even if it preserves
							-- upper bus-address bits in the receive header.
							if transaction_bardec(0) = '1' then
								addr_index := to_integer(unsigned(addr_dw(7 downto 0)));
							else
								addr_index := to_integer(unsigned(addr_dw));
							end if;

							if hit_write and (transaction_bardec(2) = '1') then
								-- BAR2 is posted-write-only in this milestone. The row
								-- streamer owns address validation and ignores writes while
								-- its exact row engine is consuming the staged buffers.
								stream_write_addr_dw <= addr_dw;
								stream_write_data <= payload_dw;
								stream_write <= '1';
								if transaction_write_count = "10" then
									stream_write_pending <= '1';
									stream_write_pending_addr_dw <= std_logic_vector(unsigned(addr_dw) + 1);
									stream_write_pending_data <= transaction_payload_dw1;
								else
									rx_nonposted_busy <= '0';
								end if;
								dbg_hit_write <= '1';
								dbg_seen_hit_write <= '1';
								dbg_write_count <= dbg_write_count + 1;
							elsif hit_write and (transaction_bardec(0) = '1') then
								dbg_hit_write <= '1';
								dbg_seen_hit_write <= '1';
								dbg_write_count <= dbg_write_count + 1;
								if addr_index = BAR0_CALL_BASE_DW + DEBUG_TARGET_PACKAGE_WORD then
									dbg_word30_write_count <= dbg_word30_write_count + 1;
									dbg_word30_last_payload <= payload_dw;
								end if;
								if (addr_index >= BAR0_CALL_BASE_DW)
									and (addr_index < BAR0_CALL_BASE_DW + CALL_WORD_COUNT) then
									if (addr_index >= BAR0_CALL_BASE_DW + CALL_INPUT_WORD)
										and (addr_index < BAR0_CALL_BASE_DW + CALL_OUTPUT_WORD) then
										call_input_words(addr_index - BAR0_CALL_BASE_DW - CALL_INPUT_WORD) <= payload_dw;
									elsif addr_index < BAR0_CALL_BASE_DW + CALL_INPUT_WORD then
										case addr_index - BAR0_CALL_BASE_DW is
										when CALL_MAGIC_WORD => call_magic <= payload_dw;
										when CALL_ABI_FUNCTION_WORD => call_abi_function <= payload_dw;
										when 2 => call_id_low <= payload_dw;
										when 3 => call_id_high <= payload_dw;
										when CALL_STATE_WORD => call_state <= payload_dw;
										when 5 => call_flags <= payload_dw;
										when CALL_INPUT_LEN_WORD => call_input_len <= payload_dw;
										when CALL_OUTPUT_CAP_WORD => call_output_cap <= payload_dw;
										when CALL_OUTPUT_LEN_WORD => call_output_len <= payload_dw;
										when CALL_ERROR_WORD => call_error <= payload_dw;
										when others => null;
										end case;
									end if;
								elsif (addr_index >= BAR0_FIRMWARE_MANIFEST_BASE_DW)
									and (addr_index < BAR0_FIRMWARE_MANIFEST_BASE_DW + FIRMWARE_MANIFEST_WORD_COUNT) then
									null;
								elsif addr_index = BAR0_CALL_DOORBELL_DW then
									if (payload_dw = CALL_DOORBELL_MAGIC)
										and (call_state = WORK_STATE_HOST_READY)
										and (call_pending = '0')
										and (call_active = '0')
										and (function_busy = '0') then
										call_state <= WORK_STATE_FPGA_BUSY;
										call_pending <= '1';
									else
										call_output_len <= (others => '0');
										call_error <= CALL_ERROR_BAD_PACKAGE;
										call_state <= WORK_STATE_FAILED;
										call_irq_retire <= '1';
										call_retire_count <= call_retire_count + 1;
									end if;
							elsif addr_index = BAR0_CALL_IRQ_ACK_DW then
								if payload_dw(0) = '1' then
									call_irq_bar_ack <= '1';
										if (stream_state = STREAM_STATE_COMPLETE)
											or (stream_state = STREAM_STATE_FAILED) then
										stream_irq_enable <= '0';
									end if;
									if (lfm25_decode_state = DECODE_STATE_COMPLETE)
										or (lfm25_decode_state = DECODE_STATE_FAILED) then
										decode_irq_enable <= '0';
									end if;
								end if;
								elsif addr_index = BAR0_STREAM_CONTROL_DW then
									stream_control <= payload_dw;
								elsif addr_index = BAR0_STREAM_ROW_DW then
									stream_row <= payload_dw;
							elsif addr_index = BAR0_STREAM_DOORBELL_DW then
									stream_irq_enable <= stream_control(8);
									if (payload_dw = STREAM_DOORBELL_MAGIC)
										and (stream_state /= STREAM_STATE_BUSY)
										and (stream_engine_busy = '0')
										and ((stream_control(1 downto 0) = "01")
											or (stream_control(1 downto 0) = "10")) then
										stream_state <= STREAM_STATE_BUSY;
										stream_error_code <= (others => '0');
										stream_start <= '1';
									else
										stream_state <= STREAM_STATE_FAILED;
										stream_error_code <= STREAM_ERROR_BAD_DOORBELL;
										stream_completion_count <= stream_completion_count + 1;
										call_irq_retire <= '1';
									call_retire_count <= call_retire_count + 1;
								end if;
							elsif addr_index = BAR0_DECODE_COMMAND_DW then
								lfm25_decode_command <= payload_dw;
							elsif addr_index = BAR0_DECODE_POSITION_DW then
								lfm25_decode_position <= payload_dw;
							elsif addr_index = BAR0_DECODE_SESSION_EPOCH_DW then
								lfm25_decode_session_epoch <= payload_dw;
							elsif addr_index = BAR0_DECODE_DOORBELL_DW then
								-- TGD1 owns interrupt admission for exactly this doorbell;
								-- it never depends on the legacy row stream's control bit.
								decode_irq_enable <= '1';
								lfm25_decode_doorbell_value <= payload_dw;
								lfm25_decode_doorbell <= '1';
							else
									case addr_dw(9 downto 0) is
									when BAR0_LED_DW =>
										if payload_dw = LED_DEBUG_ON then
											debug_led_mode <= '1';
										elsif payload_dw = LED_DEBUG_OFF then
											debug_led_mode <= '0';
											led_reg <= "00001";
										elsif debug_led_mode = '0' then
											val8 := payload_byte(payload_dw);
											led_reg <= val8(4 downto 0);
										end if;
									when BAR0_RESET_DW =>
										call_magic <= WORK_PACKAGE_MAGIC;
										call_abi_function <= x"0000" & WORK_ABI_VERSION;
										call_id_low <= (others => '0');
										call_id_high <= (others => '0');
										call_state <= WORK_STATE_IDLE;
										call_flags <= (others => '0');
										call_input_len <= (others => '0');
										call_output_cap <= (others => '0');
										call_output_len <= (others => '0');
										call_error <= (others => '0');
									call_input_words <= (others => (others => '0'));
									call_output_words <= (others => (others => '0'));
									call_pending <= '0';
									call_active <= '0';
									call_active_function <= (others => '0');
									call_active_output_bytes <= (others => '0');
									call_irq_bar_ack <= '1';
									when others =>
										null;
									end case;
								end if;
								rx_nonposted_busy <= '0';
							elsif hit_read and (transaction_bardec(0) = '1') then
								dbg_hit_read <= '1';
								dbg_seen_hit_read <= '1';
								bar_read_addr_dw <= addr_dw(9 downto 0);
								bar_read_req_id <= req_id;
								bar_read_req_tag <= req_tag;
								if (addr_index >= BAR0_CALL_BASE_DW)
									and (addr_index < BAR0_CALL_BASE_DW + CALL_INPUT_WORD) then
									bar_read_bank <= BAR_READ_BANK_CALL_HEADER;
									bar_read_word_index <= std_logic_vector(to_unsigned(addr_index - BAR0_CALL_BASE_DW, 6));
								elsif (addr_index >= BAR0_CALL_BASE_DW + CALL_INPUT_WORD)
									and (addr_index < BAR0_CALL_BASE_DW + CALL_OUTPUT_WORD) then
									bar_read_bank <= BAR_READ_BANK_CALL_INPUT;
									bar_read_word_index <= std_logic_vector(to_unsigned(addr_index - BAR0_CALL_BASE_DW - CALL_INPUT_WORD, 6));
								elsif (addr_index >= BAR0_CALL_BASE_DW + CALL_OUTPUT_WORD)
									and (addr_index < BAR0_CALL_BASE_DW + CALL_WORD_COUNT) then
									bar_read_bank <= BAR_READ_BANK_CALL_OUTPUT;
									bar_read_word_index <= std_logic_vector(to_unsigned(addr_index - BAR0_CALL_BASE_DW - CALL_OUTPUT_WORD, 6));
								elsif (addr_index >= BAR0_FIRMWARE_MANIFEST_BASE_DW)
									and (addr_index < BAR0_FEED_CAPABILITY_BASE_DW + FEED_CAPABILITY_WORD_COUNT) then
									bar_read_bank <= BAR_READ_BANK_MANIFEST;
									bar_read_word_index <= std_logic_vector(to_unsigned(addr_index - BAR0_FIRMWARE_MANIFEST_BASE_DW, 6));
							elsif (addr_index >= BAR0_STREAM_CAPABILITY_DW)
								and (addr_index <= BAR0_DECODE_RESULT1_DW) then
									bar_read_bank <= BAR_READ_BANK_STREAM;
									bar_read_word_index <= std_logic_vector(to_unsigned(addr_index - BAR0_STREAM_CAPABILITY_DW, 6));
								elsif (addr_index >= 16) and (addr_index <= 30) then
									bar_read_bank <= BAR_READ_BANK_DEBUG;
									bar_read_word_index <= std_logic_vector(to_unsigned(addr_index - 16, 6));
								elsif addr_index < 64 then
									bar_read_bank <= BAR_READ_BANK_CONTROL;
									bar_read_word_index <= addr_dw(5 downto 0);
								else
									bar_read_bank <= BAR_READ_BANK_NONE;
									bar_read_word_index <= (others => '0');
								end if;
								bar_read_select_pending <= '1';
							else
								rx_nonposted_busy <= '0';
							end if;

							transaction_pending <= '0';
							transaction_write <= '0';
							transaction_read <= '0';
							transaction_bardec <= (others => '0');
							transaction_write_count <= (others => '0');
					elsif decode_pending = '1' then
							-- Gowin presents the first protocol dword in the highest valid
							-- lane (IPUG1020 Figure 3-1), so the descending-lane snapshot is
							-- the only protocol order. Do not heuristically decode the
							-- ascending view: a payload such as 0x20DF9801 can itself look
							-- like a valid one-dword Memory Read header and shadow the real
							-- write TLP.
							decode_words(pkt_words_rev, to_integer(pkt_cnt_rev), hit_write, hit_read,
								addr_dw, payload_dw, payload_dw1, write_count, req_id, req_tag);
							dbg_last_addr_dw <= addr_dw(9 downto 0);
							dbg_last_payload_dw <= payload_dw;
							dbg_last_req_id <= req_id;
							dbg_last_req_tag <= req_tag;
							if hit_write then
								transaction_write <= '1';
							else
								transaction_write <= '0';
							end if;
							if hit_read then
								transaction_read <= '1';
							else
								transaction_read <= '0';
							end if;
							if hit_write or hit_read then
								transaction_pending <= '1';
							else
								transaction_pending <= '0';
								rx_nonposted_busy <= '0';
							end if;
							transaction_addr_dw <= addr_dw;
							transaction_bardec <= rx_packet_bardec;
							transaction_payload_dw <= payload_dw;
							transaction_payload_dw1 <= payload_dw1;
							transaction_write_count <= write_count;
							transaction_req_id <= req_id;
							transaction_req_tag <= req_tag;
							next_cnt_rev := 0;
							clear_words(next_words_rev);
							decode_pending <= '0';
					elsif stream_write_pending = '1' then
						-- A two-dword posted write is retired into the existing
						-- single-dword row-memory port over two adjacent clocks.
						stream_write_addr_dw <= stream_write_pending_addr_dw;
						stream_write_data <= stream_write_pending_data;
						stream_write <= '1';
						stream_write_pending <= '0';
						rx_nonposted_busy <= '0';
					elsif continuation_pending = '1' then
						-- Continuation beats are deliberately serialized one protocol
						-- dword per clock. Multi-beat TLPs are rare, and this keeps the
						-- accumulated-count barrel mux off the 100 MHz receive path.
						case to_integer(continuation_lane) is
						when 0 =>
							payload_dw := continuation_data(31 downto 0);
							hit_write := continuation_valid(0) = '1';
						when 1 =>
							payload_dw := continuation_data(63 downto 32);
							hit_write := continuation_valid(1) = '1';
						when 2 =>
							payload_dw := continuation_data(95 downto 64);
							hit_write := continuation_valid(2) = '1';
						when 3 =>
							payload_dw := continuation_data(127 downto 96);
							hit_write := continuation_valid(3) = '1';
						when 4 =>
							payload_dw := continuation_data(159 downto 128);
							hit_write := continuation_valid(4) = '1';
						when 5 =>
							payload_dw := continuation_data(191 downto 160);
							hit_write := continuation_valid(5) = '1';
						when 6 =>
							payload_dw := continuation_data(223 downto 192);
							hit_write := continuation_valid(6) = '1';
						when others =>
							payload_dw := continuation_data(255 downto 224);
							hit_write := continuation_valid(7) = '1';
						end case;

						if hit_write and (next_cnt_rev < PKT_MAX_WORDS) then
							case next_cnt_rev is
							when 0 => next_words_rev(0) := payload_dw;
							when 1 => next_words_rev(1) := payload_dw;
							when 2 => next_words_rev(2) := payload_dw;
							when 3 => next_words_rev(3) := payload_dw;
							when 4 => next_words_rev(4) := payload_dw;
							when 5 => next_words_rev(5) := payload_dw;
							when 6 => next_words_rev(6) := payload_dw;
							when others => next_words_rev(7) := payload_dw;
							end case;
							next_cnt_rev := next_cnt_rev + 1;
						end if;

						if continuation_lane = 0 then
							continuation_pending <= '0';
							if continuation_eop = '1' then
								dbg_last_rx_rev_dw0 <= next_words_rev(0);
								dbg_last_rx_rev_dw1 <= next_words_rev(1);
								dbg_last_rx_rev_dw2 <= next_words_rev(2);
								dbg_last_rx_rev_dw3 <= next_words_rev(3);
								rx_packet_active <= '0';
								decode_pending <= '1';
							else
								rx_packet_active <= '1';
								rx_nonposted_busy <= '0';
							end if;
						else
							continuation_lane <= continuation_lane - 1;
						end if;
					elsif capture_pending = '1' then
						-- A SOP beat takes the original zero-based compactor path. A
						-- continuation is copied into a registered lane scanner so packet
						-- accumulation never becomes a single-cycle barrel shift.
						dw(0) := rx_snapshot_data(31 downto 0);
						dw(1) := rx_snapshot_data(63 downto 32);
						dw(2) := rx_snapshot_data(95 downto 64);
						dw(3) := rx_snapshot_data(127 downto 96);
						dw(4) := rx_snapshot_data(159 downto 128);
						dw(5) := rx_snapshot_data(191 downto 160);
						dw(6) := rx_snapshot_data(223 downto 192);
						dw(7) := rx_snapshot_data(255 downto 224);
						capture_pending <= '0';
						if rx_snapshot_sop = '1' then
							next_cnt_rev := 0;
							clear_words(next_words_rev);
							for i in 7 downto 0 loop
								if rx_snapshot_valid(i) = '1' then
									next_words_rev(next_cnt_rev) := dw(i);
									next_cnt_rev := next_cnt_rev + 1;
								end if;
							end loop;
							rx_packet_bardec <= rx_snapshot_bardec;
							dbg_rx_capture_count <= dbg_rx_capture_count + 1;
							if rx_snapshot_eop = '1' then
								dbg_last_rx_rev_dw0 <= next_words_rev(0);
								dbg_last_rx_rev_dw1 <= next_words_rev(1);
								dbg_last_rx_rev_dw2 <= next_words_rev(2);
								dbg_last_rx_rev_dw3 <= next_words_rev(3);
								rx_packet_active <= '0';
								decode_pending <= '1';
							else
								rx_packet_active <= '1';
								rx_nonposted_busy <= '0';
							end if;
						else
							continuation_data <= rx_snapshot_data;
							continuation_valid <= rx_snapshot_valid;
							continuation_eop <= rx_snapshot_eop;
							continuation_lane <= to_unsigned(7, continuation_lane'length);
							continuation_pending <= '1';
						end if;
						if rx_snapshot_eop = '1' then
							dbg_rx_bar0_eop <= '1';
							dbg_seen_rx_bar0_eop <= '1';
						end if;
						if rx_snapshot_err /= x"00" then
							dbg_rx_error_count <= dbg_rx_error_count + 1;
						end if;
					elsif ((tl_rx_sop = '1') or (rx_packet_active = '1'))
						and (tl_rx_valid /= x"00")
						and (pcie_linkup = '1')
						and ((rx_packet_active = '1')
							or (tl_rx_bardec(0) = '1') or (tl_rx_bardec(2) = '1'))
						-- Reaching this final branch already proves that capture,
						-- decode, transaction, and staged BAR2 work are idle.  Test
						-- only the independently-held read/receive busy bit here;
						-- feeding the combined RX-wait signal back into this clock
						-- enable creates an avoidable decode-to-debug-counter path.
						and (rx_nonposted_busy = '0') then
						capture_pending <= '1';
						rx_nonposted_busy <= '1';
					end if;

				pkt_cnt_rev <= to_unsigned(next_cnt_rev, pkt_cnt_rev'length);
				pkt_words_rev <= next_words_rev;
				if debug_led_mode = '1' then
					led_reg(0) <= dbg_seen_rx_bar0_eop;
					led_reg(1) <= dbg_seen_hit_write;
					led_reg(2) <= dbg_seen_hit_read;
					led_reg(3) <= dbg_seen_magic_read;
					led_reg(4) <= dbg_seen_queue_cpld;
				end if;
			end if;
		end if;
	end process;

	-- Board LEDs are active-low.
	usr_led0 <= not led_reg(0);
	usr_led1 <= not led_reg(1);
	usr_led2 <= not led_reg(2);
	usr_led3 <= not led_reg(3);
	usr_led4 <= not led_reg(4);

end architecture;
