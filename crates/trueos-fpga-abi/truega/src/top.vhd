library IEEE;
use IEEE.std_logic_1164.all;
use IEEE.numeric_std.all;

entity top is
	port (
		clk          : in  std_logic;
		pcie_perst_n : in  std_logic;

		-- PCIe PHY pins (x1)
		pcie_refclk_p : in  std_logic;
		pcie_refclk_n : in  std_logic;
		pcie_rxp0     : in  std_logic;
		pcie_rxn0     : in  std_logic;
		pcie_txp0     : out std_logic;
		pcie_txn0     : out std_logic;

		usr_led0 : out std_logic;
		usr_led1 : out std_logic;
		usr_led2 : out std_logic;
		usr_led3 : out std_logic;
		usr_led4 : out std_logic
	);
end entity;

architecture rtl of top is
	component SerDes_Top is
		port (
			PCIE_Controller_Top_pcie_tl_rx_sop_o        : out std_logic;
			PCIE_Controller_Top_pcie_tl_rx_eop_o        : out std_logic;
			PCIE_Controller_Top_pcie_tl_rx_data_o       : out std_logic_vector(255 downto 0);
			PCIE_Controller_Top_pcie_tl_rx_valid_o      : out std_logic_vector(7 downto 0);
			PCIE_Controller_Top_pcie_tl_rx_bardec_o     : out std_logic_vector(5 downto 0);
			PCIE_Controller_Top_pcie_tl_rx_err_o        : out std_logic_vector(7 downto 0);
			PCIE_Controller_Top_pcie_tl_tx_wait_o       : out std_logic;
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

			pcie_refclk_p_i : in  std_logic;
			pcie_refclk_n_i : in  std_logic;
			pcie_rxp0_i     : in  std_logic;
			pcie_rxn0_i     : in  std_logic;
			pcie_txp0_o     : out std_logic;
			pcie_txn0_o     : out std_logic;

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
			PCIE_Controller_Top_pcie_tl_drp_addr_i   : in  std_logic_vector(23 downto 0);
			PCIE_Controller_Top_pcie_tl_drp_wrdata_i : in  std_logic_vector(31 downto 0);
			PCIE_Controller_Top_pcie_tl_drp_strb_i   : in  std_logic_vector(7 downto 0);
			PCIE_Controller_Top_pcie_tl_drp_wr_i     : in  std_logic;
			PCIE_Controller_Top_pcie_tl_drp_rd_i     : in  std_logic
		);
	end component;

	type word_arr_t is array (0 to 15) of std_logic_vector(31 downto 0);
	type call_word_arr_t is array (0 to 63) of std_logic_vector(31 downto 0);
	subtype byte_t is std_logic_vector(7 downto 0);
	constant PKT_MAX_WORDS : integer := 16;
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
	constant BAR0_CALL_BASE_DW : integer := 16#100# / 4;
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
	constant LED_DEBUG_ON : std_logic_vector(31 downto 0) := x"D06D0001";
	constant LED_DEBUG_OFF : std_logic_vector(31 downto 0) := x"D06D0000";
	constant TX_CPLD_REVERSE_DWORDS : boolean := false;

	signal pcie_linkup : std_logic;

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

	signal led_reg : std_logic_vector(4 downto 0) := (others => '0');
	signal debug_led_mode : std_logic := '0';
	signal call_words : call_word_arr_t := (others => (others => '0'));
	signal call_pending : std_logic := '0';
	signal call_retire_count : unsigned(31 downto 0) := (others => '0');
	signal tx_pending : std_logic := '0';
	signal tx_pending_data : std_logic_vector(255 downto 0) := (others => '0');
	signal tx_pending_valid : std_logic_vector(7 downto 0) := (others => '0');
	signal tx_pending_sop : std_logic := '0';
	signal tx_pending_eop : std_logic := '0';

	signal pkt_active    : std_logic := '0';
	signal pkt_bar0      : std_logic := '0';
	signal pkt_cnt_fwd   : unsigned(4 downto 0) := (others => '0');
	signal pkt_cnt_rev   : unsigned(4 downto 0) := (others => '0');
	signal pkt_words_fwd : word_arr_t := (others => (others => '0'));
	signal pkt_words_rev : word_arr_t := (others => (others => '0'));

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
	u_serdes: SerDes_Top
		port map(
			PCIE_Controller_Top_pcie_tl_rx_sop_o        => tl_rx_sop,
			PCIE_Controller_Top_pcie_tl_rx_eop_o        => tl_rx_eop,
			PCIE_Controller_Top_pcie_tl_rx_data_o       => tl_rx_data,
			PCIE_Controller_Top_pcie_tl_rx_valid_o      => tl_rx_valid,
			PCIE_Controller_Top_pcie_tl_rx_bardec_o     => tl_rx_bardec,
			PCIE_Controller_Top_pcie_tl_rx_err_o        => tl_rx_err,
			PCIE_Controller_Top_pcie_tl_tx_wait_o       => tl_tx_wait,
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

			pcie_refclk_p_i => pcie_refclk_p,
			pcie_refclk_n_i => pcie_refclk_n,
			pcie_rxp0_i     => pcie_rxp0,
			pcie_rxn0_i     => pcie_rxn0,
			pcie_txp0_o     => pcie_txp0,
			pcie_txn0_o     => pcie_txn0,

			debug_refclk_det_o => open,
			debug_rx_lock_o    => open,

			PCIE_Controller_Top_pcie_rstn_i          => pcie_perst_n,
			PCIE_Controller_Top_pcie_tl_clk_i        => clk,
			PCIE_Controller_Top_pcie_tl_rx_wait_i    => '0',
			PCIE_Controller_Top_pcie_tl_rx_masknp_i  => '0',
			PCIE_Controller_Top_pcie_tl_tx_sop_i     => tl_tx_sop,
			PCIE_Controller_Top_pcie_tl_tx_eop_i     => tl_tx_eop,
			PCIE_Controller_Top_pcie_tl_tx_data_i    => tl_tx_data,
			PCIE_Controller_Top_pcie_tl_tx_valid_i   => tl_tx_valid,
			PCIE_Controller_Top_pcie_tl_drp_addr_i   => (others => '0'),
			PCIE_Controller_Top_pcie_tl_drp_wrdata_i => (others => '0'),
			PCIE_Controller_Top_pcie_tl_drp_strb_i   => (others => '0'),
			PCIE_Controller_Top_pcie_tl_drp_wr_i     => '0',
			PCIE_Controller_Top_pcie_tl_drp_rd_i     => '0'
		);

	process(clk)
		variable dw : word_arr_t;
		variable next_words_fwd : word_arr_t;
		variable next_words_rev : word_arr_t;
		variable next_tx_data : std_logic_vector(255 downto 0);
		variable next_tx_valid : std_logic_vector(7 downto 0);
		variable next_tx_sop : std_logic;
		variable next_tx_eop : std_logic;
		variable next_active : std_logic;
		variable next_bar0 : std_logic;
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

			for h in 0 to PKT_MAX_WORDS - 1 loop
				exit when h >= count;
				hdr := words(h);
				fmt_type := hdr(31 downto 24);

				if (fmt_type = x"40") or (fmt_type = x"60") then
					if hdr(9 downto 0) = "0000000001" then
						if fmt_type = x"40" then
							addr_idx := h + 2;
							payload_idx := h + 3;
						else
							addr_idx := h + 3;
							payload_idx := h + 4;
						end if;

						if (addr_idx < 0) or (payload_idx < 0) then
							next;
						end if;
						if (addr_idx >= count) or (payload_idx >= count) then
							next;
						end if;
						if (addr_idx >= PKT_MAX_WORDS) or (payload_idx >= PKT_MAX_WORDS) then
							next;
						end if;

						addr_low := words(addr_idx);
						payload := words(payload_idx);
						addr_out := addr_low(11 downto 2);
						payload_out := payload;
						found_write := true;
						return;
					end if;
				elsif (fmt_type = x"00") or (fmt_type = x"20") then
					if hdr(9 downto 0) = "0000000001" then
						if fmt_type = x"00" then
							addr_idx := h + 2;
						else
							addr_idx := h + 3;
						end if;

						if (h + 1 >= count) or (addr_idx >= count) then
							next;
						end if;
						if (h + 1 >= PKT_MAX_WORDS) or (addr_idx >= PKT_MAX_WORDS) then
							next;
						end if;

						req_hdr := words(h + 1);
						addr_low := words(addr_idx);
						addr_out := addr_low(11 downto 2);
						req_id_out := req_hdr(31 downto 16);
						req_tag_out := req_hdr(15 downto 8);
						found_read := true;
						return;
					end if;
				end if;
			end loop;
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
				dw1(31 downto 16) := "000" & tl_cfg_busdev;
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
			if TX_CPLD_REVERSE_DWORDS then
				tx_pending_data(31 downto 0) <= data_in;
				tx_pending_data(63 downto 32) <= dw2;
				tx_pending_data(95 downto 64) <= dw1;
				tx_pending_data(127 downto 96) <= dw0;
			else
				tx_pending_data(31 downto 0) <= dw0;
				tx_pending_data(63 downto 32) <= dw1;
				tx_pending_data(95 downto 64) <= dw2;
				tx_pending_data(127 downto 96) <= data_in;
			end if;
			tx_pending_valid <= "00001111";
			tx_pending_sop <= '1';
			tx_pending_eop <= '1';
			tx_pending <= '1';
		end procedure;
	begin
		if rising_edge(clk) then
			next_tx_data := (others => '0');
			next_tx_valid := (others => '0');
			next_tx_sop := '0';
			next_tx_eop := '0';

			if pcie_perst_n = '0' then
				led_reg <= (others => '0');
				debug_led_mode <= '0';
				call_words <= (others => (others => '0'));
				call_words(CALL_MAGIC_WORD) <= WORK_PACKAGE_MAGIC;
				call_words(CALL_ABI_FUNCTION_WORD)(15 downto 0) <= WORK_ABI_VERSION;
				call_words(CALL_STATE_WORD) <= WORK_STATE_IDLE;
				call_pending <= '0';
				call_retire_count <= (others => '0');
				pkt_active <= '0';
				pkt_bar0 <= '0';
				pkt_cnt_fwd <= (others => '0');
				pkt_cnt_rev <= (others => '0');
				pkt_words_fwd <= (others => (others => '0'));
				pkt_words_rev <= (others => (others => '0'));
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
					dbg_rx_bar0_eop <= '0';
					dbg_hit_write <= '0';
					dbg_hit_read <= '0';
					dbg_magic_read <= '0';
					dbg_queue_cpld <= '0';
					dbg_tx_fire <= '0';
					dbg_cpld_blocked <= '0';

					-- A doorbell only selects one of three circuits already present in this
					-- bitstream. There is no instruction fetch or command interpreter.
					if call_pending = '1' then
						call_words(CALL_OUTPUT_LEN_WORD) <= (others => '0');
						call_words(CALL_ERROR_WORD) <= (others => '0');
						if (call_words(CALL_MAGIC_WORD) /= WORK_PACKAGE_MAGIC)
							or (call_words(CALL_ABI_FUNCTION_WORD)(15 downto 0) /= WORK_ABI_VERSION) then
							call_words(CALL_ERROR_WORD) <= CALL_ERROR_BAD_PACKAGE;
							call_words(CALL_STATE_WORD) <= WORK_STATE_FAILED;
						elsif unsigned(call_words(CALL_OUTPUT_CAP_WORD)) < to_unsigned(4, 32) then
							call_words(CALL_ERROR_WORD) <= CALL_ERROR_BAD_LENGTH;
							call_words(CALL_STATE_WORD) <= WORK_STATE_FAILED;
						else
							case call_words(CALL_ABI_FUNCTION_WORD)(31 downto 16) is
							when x"0000" =>
								-- slot 0: heartbeat() -> "TGAT"
								call_words(CALL_OUTPUT_WORD) <= PROTOCOL_MAGIC;
								call_words(CALL_OUTPUT_LEN_WORD) <= x"00000004";
								call_words(CALL_STATE_WORD) <= WORK_STATE_COMPLETE;
							when x"0001" =>
								-- slot 1: add_u32(a, b) -> a + b
								if unsigned(call_words(CALL_INPUT_LEN_WORD)) < to_unsigned(8, 32) then
									call_words(CALL_ERROR_WORD) <= CALL_ERROR_BAD_LENGTH;
									call_words(CALL_STATE_WORD) <= WORK_STATE_FAILED;
								else
									call_words(CALL_OUTPUT_WORD) <= std_logic_vector(
										unsigned(call_words(CALL_INPUT_WORD))
										+ unsigned(call_words(CALL_INPUT_WORD + 1))
									);
									call_words(CALL_OUTPUT_LEN_WORD) <= x"00000004";
									call_words(CALL_STATE_WORD) <= WORK_STATE_COMPLETE;
								end if;
							when x"0002" =>
								-- slot 2: xor_u32(a, b) -> a xor b
								if unsigned(call_words(CALL_INPUT_LEN_WORD)) < to_unsigned(8, 32) then
									call_words(CALL_ERROR_WORD) <= CALL_ERROR_BAD_LENGTH;
									call_words(CALL_STATE_WORD) <= WORK_STATE_FAILED;
								else
									call_words(CALL_OUTPUT_WORD) <= call_words(CALL_INPUT_WORD)
										xor call_words(CALL_INPUT_WORD + 1);
									call_words(CALL_OUTPUT_LEN_WORD) <= x"00000004";
									call_words(CALL_STATE_WORD) <= WORK_STATE_COMPLETE;
								end if;
							when others =>
								call_words(CALL_ERROR_WORD) <= CALL_ERROR_BAD_FUNCTION;
								call_words(CALL_STATE_WORD) <= WORK_STATE_FAILED;
							end case;
						end if;
						call_pending <= '0';
						call_retire_count <= call_retire_count + 1;
					end if;

					if (tx_pending = '1') and (tl_tx_wait = '0') then
						next_tx_data := tx_pending_data;
						next_tx_valid := tx_pending_valid;
						next_tx_sop := tx_pending_sop;
						next_tx_eop := tx_pending_eop;
						dbg_tx_fire <= '1';
						dbg_seen_tx_fire <= '1';
						tx_pending <= '0';
						tx_pending_data <= (others => '0');
					tx_pending_valid <= (others => '0');
					tx_pending_sop <= '0';
					tx_pending_eop <= '0';
				end if;

				next_active := pkt_active;
				next_bar0 := pkt_bar0;
				next_cnt_fwd := to_integer(pkt_cnt_fwd);
				next_cnt_rev := to_integer(pkt_cnt_rev);
				next_words_fwd := pkt_words_fwd;
				next_words_rev := pkt_words_rev;

				dw(0) := tl_rx_data(31 downto 0);
				dw(1) := tl_rx_data(63 downto 32);
				dw(2) := tl_rx_data(95 downto 64);
				dw(3) := tl_rx_data(127 downto 96);
				dw(4) := tl_rx_data(159 downto 128);
				dw(5) := tl_rx_data(191 downto 160);
				dw(6) := tl_rx_data(223 downto 192);
				dw(7) := tl_rx_data(255 downto 224);

				if tl_rx_sop = '1' then
					next_active := '1';
					next_bar0 := tl_rx_bardec(0);
					next_cnt_fwd := 0;
					next_cnt_rev := 0;
					clear_words(next_words_fwd);
					clear_words(next_words_rev);
				elsif next_active = '1' then
					if tl_rx_bardec(0) = '1' then
						next_bar0 := '1';
					end if;
				end if;

				if next_active = '1' then
					for i in 0 to 7 loop
						if tl_rx_valid(i) = '1' then
							if next_cnt_fwd < PKT_MAX_WORDS then
								next_words_fwd(next_cnt_fwd) := dw(i);
								next_cnt_fwd := next_cnt_fwd + 1;
							end if;
						end if;
					end loop;

					for i in 7 downto 0 loop
						if tl_rx_valid(i) = '1' then
							if next_cnt_rev < PKT_MAX_WORDS then
								next_words_rev(next_cnt_rev) := dw(i);
								next_cnt_rev := next_cnt_rev + 1;
							end if;
						end if;
					end loop;
				end if;

					if (next_active = '1') and (tl_rx_eop = '1') then
						if (pcie_linkup = '1') and (next_bar0 = '1') then
							dbg_rx_bar0_eop <= '1';
							dbg_seen_rx_bar0_eop <= '1';
							dbg_last_rx_fwd_dw0 <= next_words_fwd(0);
							dbg_last_rx_fwd_dw1 <= next_words_fwd(1);
							dbg_last_rx_fwd_dw2 <= next_words_fwd(2);
							dbg_last_rx_fwd_dw3 <= next_words_fwd(3);
							dbg_last_rx_rev_dw0 <= next_words_rev(0);
							dbg_last_rx_rev_dw1 <= next_words_rev(1);
							dbg_last_rx_rev_dw2 <= next_words_rev(2);
							dbg_last_rx_rev_dw3 <= next_words_rev(3);
							decode_words(next_words_fwd, next_cnt_fwd, hit_write, hit_read, addr_dw, payload_dw, req_id, req_tag);
							if not hit_write and not hit_read then
								decode_words(next_words_rev, next_cnt_rev, hit_write, hit_read, addr_dw, payload_dw, req_id, req_tag);
							end if;
							dbg_last_addr_dw <= addr_dw;
							dbg_last_payload_dw <= payload_dw;
							dbg_last_req_id <= req_id;
							dbg_last_req_tag <= req_tag;
							addr_index := to_integer(unsigned(addr_dw));

							if hit_write then
								dbg_hit_write <= '1';
								dbg_seen_hit_write <= '1';
								if (addr_index >= BAR0_CALL_BASE_DW)
									and (addr_index < BAR0_CALL_BASE_DW + CALL_WORD_COUNT) then
									call_words(addr_index - BAR0_CALL_BASE_DW) <= payload_dw;
								elsif addr_index = BAR0_CALL_DOORBELL_DW then
									if (payload_dw = CALL_DOORBELL_MAGIC)
										and (call_words(CALL_STATE_WORD) = WORK_STATE_HOST_READY)
										and (call_pending = '0') then
										call_words(CALL_STATE_WORD) <= WORK_STATE_FPGA_BUSY;
										call_pending <= '1';
									else
										call_words(CALL_OUTPUT_LEN_WORD) <= (others => '0');
										call_words(CALL_ERROR_WORD) <= CALL_ERROR_BAD_PACKAGE;
										call_words(CALL_STATE_WORD) <= WORK_STATE_FAILED;
									end if;
								elsif addr_index = BAR0_CALL_IRQ_ACK_DW then
									null;
								else
									case addr_dw is
									when BAR0_LED_DW =>
										if payload_dw = LED_DEBUG_ON then
											debug_led_mode <= '1';
										elsif payload_dw = LED_DEBUG_OFF then
											debug_led_mode <= '0';
											led_reg <= (others => '0');
										elsif debug_led_mode = '0' then
											val8 := payload_byte(payload_dw);
											led_reg <= val8(4 downto 0);
										end if;
									when BAR0_RESET_DW =>
										call_words <= (others => (others => '0'));
										call_words(CALL_MAGIC_WORD) <= WORK_PACKAGE_MAGIC;
										call_words(CALL_ABI_FUNCTION_WORD)(15 downto 0) <= WORK_ABI_VERSION;
										call_words(CALL_STATE_WORD) <= WORK_STATE_IDLE;
										call_pending <= '0';
									when others =>
										null;
									end case;
								end if;
							elsif hit_read then
								dbg_hit_read <= '1';
								dbg_seen_hit_read <= '1';
								read_data_dw := (others => '0');
								if (addr_index >= BAR0_CALL_BASE_DW)
									and (addr_index < BAR0_CALL_BASE_DW + CALL_WORD_COUNT) then
									read_data_dw := call_words(addr_index - BAR0_CALL_BASE_DW);
								elsif addr_index = BAR0_CALL_DOORBELL_DW then
									read_data_dw := std_logic_vector(call_retire_count);
								else
									case addr_dw is
									when BAR0_LED_DW =>
										read_data_dw(4 downto 0) := led_reg;
									when BAR0_STATUS_DW =>
										read_data_dw := call_words(CALL_STATE_WORD);
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
								end if;
							end if;
						end if;

					next_active := '0';
					next_bar0 := '0';
					next_cnt_fwd := 0;
					next_cnt_rev := 0;
					clear_words(next_words_fwd);
					clear_words(next_words_rev);
				end if;

				pkt_active <= next_active;
				pkt_bar0 <= next_bar0;
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
				tl_tx_data <= next_tx_data;
				tl_tx_valid <= next_tx_valid;
				tl_tx_sop <= next_tx_sop;
				tl_tx_eop <= next_tx_eop;
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
