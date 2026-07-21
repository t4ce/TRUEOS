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

	type word_arr_t is array (0 to 7) of std_logic_vector(31 downto 0);
	type call_data_arr_t is array (0 to 23) of std_logic_vector(31 downto 0);
	constant RX_FIFO_DEPTH : integer := 4;
	type rx_data_fifo_t is array (0 to RX_FIFO_DEPTH - 1) of std_logic_vector(255 downto 0);
	type rx_valid_fifo_t is array (0 to RX_FIFO_DEPTH - 1) of std_logic_vector(7 downto 0);
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
	constant BAR0_CALL_DOORBELL_DW : integer := 16#080# / 4;
	constant BAR0_CALL_IRQ_ACK_DW : integer := 16#084# / 4;
	constant BAR0_CALL_IRQ_RETIRE_COUNT_DW : integer := 16#088# / 4;
	constant BAR0_CALL_IRQ_REQUEST_COUNT_DW : integer := 16#08C# / 4;
	constant BAR0_CALL_IRQ_CONTROLLER_ACK_COUNT_DW : integer := 16#090# / 4;
	constant BAR0_CALL_IRQ_STATE_DW : integer := 16#094# / 4;
	constant BAR0_CALL_BASE_DW : integer := 16#100# / 4;
	constant BAR0_FIRMWARE_MANIFEST_BASE_DW : integer := 16#200# / 4;
	constant FIRMWARE_MANIFEST_WORD_COUNT : integer := 32;
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
	constant LED_DEBUG_ON : std_logic_vector(31 downto 0) := x"D06D0001";
	constant LED_DEBUG_OFF : std_logic_vector(31 downto 0) := x"D06D0000";

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
	signal function_busy : std_logic;
	signal function_done : std_logic;
	signal function_error : std_logic;
	signal firmware_manifest_word : std_logic_vector(31 downto 0);
	signal tx_pending : std_logic := '0';
	signal tx_pending_data : std_logic_vector(255 downto 0) := (others => '0');
	signal tx_pending_valid : std_logic_vector(7 downto 0) := (others => '0');
	signal tx_pending_sop : std_logic := '0';
	signal tx_pending_eop : std_logic := '0';

	-- The hard IP can present posted writes back-to-back. Keep that receive
	-- boundary independent of the multi-cycle decode/execute path so asserting
	-- RX_WAIT never creates a one-beat acceptance hole.
	signal rx_fifo_data : rx_data_fifo_t := (others => (others => '0'));
	signal rx_fifo_valid : rx_valid_fifo_t := (others => (others => '0'));
	signal rx_fifo_write_ptr : unsigned(1 downto 0) := (others => '0');
	signal rx_fifo_read_ptr : unsigned(1 downto 0) := (others => '0');
	signal rx_fifo_count : unsigned(2 downto 0) := (others => '0');
	signal capture_pending : std_logic := '0';
	signal rx_snapshot_data : std_logic_vector(255 downto 0) := (others => '0');
	signal rx_snapshot_valid : std_logic_vector(7 downto 0) := (others => '0');
	signal pkt_cnt_fwd   : unsigned(4 downto 0) := (others => '0');
	signal pkt_cnt_rev   : unsigned(4 downto 0) := (others => '0');
	signal pkt_words_fwd : word_arr_t := (others => (others => '0'));
	signal pkt_words_rev : word_arr_t := (others => (others => '0'));
	signal decode_pending : std_logic := '0';
	signal transaction_pending : std_logic := '0';
	signal transaction_write : std_logic := '0';
	signal transaction_read : std_logic := '0';
	signal transaction_addr_dw : std_logic_vector(9 downto 0) := (others => '0');
	signal transaction_payload_dw : std_logic_vector(31 downto 0) := (others => '0');
	signal transaction_req_id : std_logic_vector(15 downto 0) := (others => '0');
	signal transaction_req_tag : std_logic_vector(7 downto 0) := (others => '0');
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
	signal dbg_last_rx_fwd_dw0  : std_logic_vector(31 downto 0) := (others => '0');
	signal dbg_last_rx_fwd_dw1  : std_logic_vector(31 downto 0) := (others => '0');
	signal dbg_last_rx_fwd_dw2  : std_logic_vector(31 downto 0) := (others => '0');
	signal dbg_last_rx_fwd_dw3  : std_logic_vector(31 downto 0) := (others => '0');
	signal dbg_last_rx_rev_dw0  : std_logic_vector(31 downto 0) := (others => '0');
	signal dbg_last_rx_rev_dw1  : std_logic_vector(31 downto 0) := (others => '0');
	signal dbg_last_rx_rev_dw2  : std_logic_vector(31 downto 0) := (others => '0');
	signal dbg_last_rx_rev_dw3  : std_logic_vector(31 downto 0) := (others => '0');
	signal dbg_last_cpld_dw0    : std_logic_vector(31 downto 0) := (others => '0');
	signal dbg_last_cpld_dw1    : std_logic_vector(31 downto 0) := (others => '0');
	signal dbg_last_cpld_dw2    : std_logic_vector(31 downto 0) := (others => '0');
	signal dbg_last_cpld_data   : std_logic_vector(31 downto 0) := (others => '0');

	attribute syn_keep : boolean;
	-- Keep the explicit FIFO-read snapshot as a physical timing boundary.  If
	-- these registers are folded into the lane compactor, a BSRAM read and the
	-- variable valid-lane packing land on the same 100 MHz path.
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
	attribute syn_keep of dbg_last_rx_fwd_dw0  : signal is true;
	attribute syn_keep of dbg_last_rx_fwd_dw1  : signal is true;
	attribute syn_keep of dbg_last_rx_fwd_dw2  : signal is true;
	attribute syn_keep of dbg_last_rx_fwd_dw3  : signal is true;
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
	-- Present a queued completion continuously. The controller samples the beat
	-- on a rising TLP clock edge where VALID is asserted and WAIT is deasserted;
	-- therefore VALID/data must not be pulsed based on the previous WAIT value.
	tl_tx_data <= tx_pending_data when tx_pending = '1' else (others => '0');
	tl_tx_valid <= tx_pending_valid when tx_pending = '1' else (others => '0');
	tl_tx_sop <= tx_pending_sop when tx_pending = '1' else '0';
	tl_tx_eop <= tx_pending_eop when tx_pending = '1' else '0';
	-- Backpressure describes the ingress queue itself, not downstream decode
	-- latency. This permits a burst to continue until every accepted beat has a
	-- physical slot and then holds the controller while the queue drains.
	tl_rx_backpressure <= '1'
		when rx_fifo_count = to_unsigned(RX_FIFO_DEPTH, rx_fifo_count'length)
		else '0';
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
			word_index => transaction_addr_dw(4 downto 0),
			data       => firmware_manifest_word
			);

	u_completion_irq: truega_completion_irq
		port map(
			clk                => tlp_clk,
			reset_n            => pcie_core_reset_n,
			retire_i           => call_irq_retire,
			interrupt_enable_i => call_flags(0),
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
		variable next_words_fwd : word_arr_t;
		variable next_words_rev : word_arr_t;
		variable next_cnt_fwd : integer range 0 to PKT_MAX_WORDS;
		variable next_cnt_rev : integer range 0 to PKT_MAX_WORDS;
		variable hit_write : boolean;
		variable hit_read : boolean;
		variable val8 : byte_t;
		variable addr_dw : std_logic_vector(9 downto 0);
		variable payload_dw : std_logic_vector(31 downto 0);
		variable req_id : std_logic_vector(15 downto 0);
		variable req_tag : std_logic_vector(7 downto 0);
		variable read_data_dw : std_logic_vector(31 downto 0);
		variable addr_index : integer range 0 to 1023;
		variable rx_fifo_push : boolean;
		variable rx_fifo_pop : boolean;

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
			variable addr_out : out std_logic_vector(9 downto 0);
			variable payload_out : out std_logic_vector(31 downto 0);
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
			req_id_out := (others => '0');
			req_tag_out := (others => '0');

			-- SOP and lane-valid compaction guarantee that the first protocol dword is
			-- word zero in one of the two lane interpretations. Searching every array
			-- position creates a deep priority chain for no supported TLP case.
			if count = 0 then
				return;
			end if;
			hdr := words(0);
			fmt_type := hdr(31 downto 24);

			if (fmt_type = x"40") or (fmt_type = x"60") then
				if hdr(9 downto 0) = "0000000001" then
					if fmt_type = x"40" then
						addr_idx := 2;
						payload_idx := 3;
					else
						addr_idx := 3;
						payload_idx := 4;
					end if;

					if (addr_idx < count) and (payload_idx < count) then
						addr_low := words(addr_idx);
						payload := words(payload_idx);
						addr_out := addr_low(11 downto 2);
						payload_out := byte_swap32(payload);
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
						addr_out := addr_low(11 downto 2);
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
			rx_fifo_push := false;
			rx_fifo_pop := false;
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
					function_start <= '0';
					call_irq_retire <= '0';
					call_irq_bar_ack <= '0';
					call_retire_count <= (others => '0');
					call_irq_request_prev <= '0';
					call_irq_controller_ack_prev <= '0';
					call_irq_request_count <= (others => '0');
					call_irq_controller_ack_count <= (others => '0');
				capture_pending <= '0';
				rx_snapshot_data <= (others => '0');
				rx_snapshot_valid <= (others => '0');
				pkt_cnt_fwd <= (others => '0');
				pkt_cnt_rev <= (others => '0');
				pkt_words_fwd <= (others => (others => '0'));
				pkt_words_rev <= (others => (others => '0'));
				decode_pending <= '0';
				transaction_pending <= '0';
				transaction_write <= '0';
				transaction_read <= '0';
				transaction_addr_dw <= (others => '0');
				transaction_payload_dw <= (others => '0');
				transaction_req_id <= (others => '0');
				transaction_req_tag <= (others => '0');
				rx_nonposted_busy <= '0';
					tx_pending <= '0';
					tx_pending_data <= (others => '0');
				tx_pending_valid <= (others => '0');
				tx_pending_sop <= '0';
				tx_pending_eop <= '0';
				rx_fifo_data <= (others => (others => '0'));
				rx_fifo_valid <= (others => (others => '0'));
				rx_fifo_write_ptr <= (others => '0');
				rx_fifo_read_ptr <= (others => '0');
				rx_fifo_count <= (others => '0');
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
					dbg_last_rx_fwd_dw0 <= (others => '0');
					dbg_last_rx_fwd_dw1 <= (others => '0');
					dbg_last_rx_fwd_dw2 <= (others => '0');
					dbg_last_rx_fwd_dw3 <= (others => '0');
					dbg_last_rx_rev_dw0 <= (others => '0');
					dbg_last_rx_rev_dw1 <= (others => '0');
					dbg_last_rx_rev_dw2 <= (others => '0');
					dbg_last_rx_rev_dw3 <= (others => '0');
					dbg_last_cpld_dw0 <= (others => '0');
					dbg_last_cpld_dw1 <= (others => '0');
					dbg_last_cpld_dw2 <= (others => '0');
					dbg_last_cpld_data <= (others => '0');
				else
					function_start <= '0';
					call_irq_retire <= '0';
					call_irq_bar_ack <= '0';
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

					-- Capture is deliberately independent of the decoder state. The
					-- controller is allowed to advance only while RX_WAIT is low, and
					-- every such single-beat BAR0 TLP is committed to this FIFO here.
					if (tl_rx_sop = '1') and (tl_rx_eop = '1')
						and (pcie_linkup = '1') and (tl_rx_bardec(0) = '1')
						and (tl_rx_backpressure = '0') then
						rx_fifo_data(to_integer(rx_fifo_write_ptr)) <= tl_rx_data;
						rx_fifo_valid(to_integer(rx_fifo_write_ptr)) <= tl_rx_valid;
						rx_fifo_write_ptr <= rx_fifo_write_ptr + 1;
						rx_fifo_push := true;
						rx_nonposted_busy <= '1';
						dbg_rx_bar0_eop <= '1';
						dbg_seen_rx_bar0_eop <= '1';
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
						elsif function_valid = '0' then
							call_error <= CALL_ERROR_BAD_FUNCTION;
							call_state <= WORK_STATE_FAILED;
							call_irq_retire <= '1';
							call_retire_count <= call_retire_count + 1;
						elsif unsigned(call_input_len)
							/= resize(unsigned(function_required_input_bytes), 32) then
							call_error <= CALL_ERROR_BAD_LENGTH;
							call_state <= WORK_STATE_FAILED;
							call_irq_retire <= '1';
							call_retire_count <= call_retire_count + 1;
						elsif unsigned(call_output_cap)
							< resize(unsigned(function_output_bytes), 32) then
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
							call_active_output_bytes <= function_output_bytes;
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

				next_cnt_fwd := to_integer(pkt_cnt_fwd);
				next_cnt_rev := to_integer(pkt_cnt_rev);
				next_words_fwd := pkt_words_fwd;
				next_words_rev := pkt_words_rev;

					if transaction_pending = '1' then
							hit_write := transaction_write = '1';
							hit_read := transaction_read = '1';
							addr_dw := transaction_addr_dw;
							payload_dw := transaction_payload_dw;
							req_id := transaction_req_id;
							req_tag := transaction_req_tag;
							addr_index := to_integer(unsigned(addr_dw));

							if hit_write then
								dbg_hit_write <= '1';
								dbg_seen_hit_write <= '1';
								if (addr_index >= BAR0_CALL_BASE_DW)
									and (addr_index < BAR0_CALL_BASE_DW + CALL_WORD_COUNT) then
									if (addr_index >= BAR0_CALL_BASE_DW + CALL_INPUT_WORD)
										and (addr_index < BAR0_CALL_BASE_DW + CALL_OUTPUT_WORD) then
										call_input_words(addr_index - BAR0_CALL_BASE_DW - CALL_INPUT_WORD) <= payload_dw;
									elsif addr_index >= BAR0_CALL_BASE_DW + CALL_OUTPUT_WORD then
										call_output_words(addr_index - BAR0_CALL_BASE_DW - CALL_OUTPUT_WORD) <= payload_dw;
									else
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
									end if;
								else
									case addr_dw is
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
							elsif hit_read then
								dbg_hit_read <= '1';
								dbg_seen_hit_read <= '1';
								read_data_dw := (others => '0');
								if (addr_index >= BAR0_CALL_BASE_DW)
									and (addr_index < BAR0_CALL_BASE_DW + CALL_WORD_COUNT) then
									if (addr_index >= BAR0_CALL_BASE_DW + CALL_INPUT_WORD)
										and (addr_index < BAR0_CALL_BASE_DW + CALL_OUTPUT_WORD) then
										read_data_dw := call_input_words(addr_index - BAR0_CALL_BASE_DW - CALL_INPUT_WORD);
									elsif addr_index >= BAR0_CALL_BASE_DW + CALL_OUTPUT_WORD then
										read_data_dw := call_output_words(addr_index - BAR0_CALL_BASE_DW - CALL_OUTPUT_WORD);
									else
										case addr_index - BAR0_CALL_BASE_DW is
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
									end if;
								elsif (addr_index >= BAR0_FIRMWARE_MANIFEST_BASE_DW)
									and (addr_index < BAR0_FIRMWARE_MANIFEST_BASE_DW + FIRMWARE_MANIFEST_WORD_COUNT) then
									read_data_dw := firmware_manifest_word;
								elsif addr_index = BAR0_CALL_DOORBELL_DW then
									read_data_dw := std_logic_vector(call_retire_count);
								elsif addr_index = BAR0_CALL_IRQ_RETIRE_COUNT_DW then
									read_data_dw := std_logic_vector(call_retire_count);
								elsif addr_index = BAR0_CALL_IRQ_REQUEST_COUNT_DW then
									read_data_dw := std_logic_vector(call_irq_request_count);
								elsif addr_index = BAR0_CALL_IRQ_CONTROLLER_ACK_COUNT_DW then
									read_data_dw := std_logic_vector(call_irq_controller_ack_count);
								elsif addr_index = BAR0_CALL_IRQ_STATE_DW then
									read_data_dw(0) := call_irq_status;
									read_data_dw(1) := call_irq_request;
									read_data_dw(2) := call_irq_controller_ack;
									read_data_dw(3) := call_flags(0);
								else
									case addr_dw is
									when BAR0_LED_DW =>
										read_data_dw(4 downto 0) := led_reg;
									when BAR0_STATUS_DW =>
										read_data_dw := call_state;
									when BAR0_MAGIC_DW =>
										read_data_dw := PROTOCOL_MAGIC;
										dbg_magic_read <= '1';
										dbg_seen_magic_read <= '1';
									when BAR0_DBG_SEEN_DW =>
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
									when BAR0_DBG_LAST_ADDR_DW =>
									read_data_dw(9 downto 0) := dbg_last_addr_dw;
									when BAR0_DBG_LAST_READ_DATA_DW =>
									read_data_dw := dbg_last_read_data;
									when BAR0_DBG_LAST_REQ_DW =>
									read_data_dw(31 downto 16) := dbg_last_req_id;
									read_data_dw(15 downto 8) := dbg_last_req_tag;
									when BAR0_DBG_LAST_CPLD0_DW =>
									read_data_dw := dbg_last_cpld_dw0;
									when BAR0_DBG_LAST_CPLD1_DW =>
									read_data_dw := dbg_last_cpld_dw1;
									when BAR0_DBG_LAST_CPLD2_DW =>
									read_data_dw := dbg_last_cpld_dw2;
									when BAR0_DBG_LAST_CPLD_DATA_DW =>
									read_data_dw := dbg_last_cpld_data;
									when others =>
									read_data_dw := (others => '0');
									end case;
								end if;
								dbg_last_read_data <= read_data_dw;

								if tx_pending = '0' then
									dbg_queue_cpld <= '1';
									dbg_seen_queue_cpld <= '1';
									queue_cpld(req_id, req_tag, addr_dw, read_data_dw);
								else
									dbg_cpld_blocked <= '1';
									rx_nonposted_busy <= '0';
								end if;
							end if;

							transaction_pending <= '0';
							transaction_write <= '0';
							transaction_read <= '0';
					elsif decode_pending = '1' then
							-- Decode only the preceding cycle's packet snapshot. Using the
							-- capture variables here creates a false live RX-to-transaction
							-- combinational path through their input muxes.
							decode_words(pkt_words_fwd, to_integer(pkt_cnt_fwd), hit_write, hit_read, addr_dw, payload_dw, req_id, req_tag);
							if not hit_write and not hit_read then
								decode_words(pkt_words_rev, to_integer(pkt_cnt_rev), hit_write, hit_read, addr_dw, payload_dw, req_id, req_tag);
							end if;
							dbg_last_addr_dw <= addr_dw;
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
							transaction_payload_dw <= payload_dw;
							transaction_req_id <= req_id;
							transaction_req_tag <= req_tag;
							next_cnt_fwd := 0;
							next_cnt_rev := 0;
							clear_words(next_words_fwd);
							clear_words(next_words_rev);
							decode_pending <= '0';
					elsif capture_pending = '1' then
						-- All supported memory TLPs fit in one 256-bit beat. First
						-- snapshot the hard-IP outputs, then compact valid lanes here
						-- from registers so the SerDes pins see no priority chain.
						dw(0) := rx_snapshot_data(31 downto 0);
						dw(1) := rx_snapshot_data(63 downto 32);
						dw(2) := rx_snapshot_data(95 downto 64);
						dw(3) := rx_snapshot_data(127 downto 96);
						dw(4) := rx_snapshot_data(159 downto 128);
						dw(5) := rx_snapshot_data(191 downto 160);
						dw(6) := rx_snapshot_data(223 downto 192);
						dw(7) := rx_snapshot_data(255 downto 224);
						next_cnt_fwd := 0;
						next_cnt_rev := 0;
						clear_words(next_words_fwd);
						clear_words(next_words_rev);
						for i in 0 to 7 loop
							if rx_snapshot_valid(i) = '1' then
								next_words_fwd(next_cnt_fwd) := dw(i);
								next_cnt_fwd := next_cnt_fwd + 1;
							end if;
						end loop;
						for i in 7 downto 0 loop
							if rx_snapshot_valid(i) = '1' then
								next_words_rev(next_cnt_rev) := dw(i);
								next_cnt_rev := next_cnt_rev + 1;
							end if;
						end loop;
						dbg_last_rx_fwd_dw0 <= next_words_fwd(0);
						dbg_last_rx_fwd_dw1 <= next_words_fwd(1);
						dbg_last_rx_fwd_dw2 <= next_words_fwd(2);
						dbg_last_rx_fwd_dw3 <= next_words_fwd(3);
						dbg_last_rx_rev_dw0 <= next_words_rev(0);
						dbg_last_rx_rev_dw1 <= next_words_rev(1);
						dbg_last_rx_rev_dw2 <= next_words_rev(2);
						dbg_last_rx_rev_dw3 <= next_words_rev(3);
						capture_pending <= '0';
						decode_pending <= '1';
					elsif rx_fifo_count /= to_unsigned(0, rx_fifo_count'length) then
						rx_snapshot_data <= rx_fifo_data(to_integer(rx_fifo_read_ptr));
						rx_snapshot_valid <= rx_fifo_valid(to_integer(rx_fifo_read_ptr));
						rx_fifo_read_ptr <= rx_fifo_read_ptr + 1;
						rx_fifo_pop := true;
						capture_pending <= '1';
						rx_nonposted_busy <= '1';
					end if;

					if rx_fifo_push and not rx_fifo_pop then
						rx_fifo_count <= rx_fifo_count + 1;
					elsif rx_fifo_pop and not rx_fifo_push then
						rx_fifo_count <= rx_fifo_count - 1;
					end if;

				pkt_cnt_fwd <= to_unsigned(next_cnt_fwd, pkt_cnt_fwd'length);
				pkt_cnt_rev <= to_unsigned(next_cnt_rev, pkt_cnt_rev'length);
				pkt_words_fwd <= next_words_fwd;
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
