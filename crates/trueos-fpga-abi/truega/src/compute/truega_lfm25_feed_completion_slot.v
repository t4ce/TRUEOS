// Async completion/status slot for the fixed TGF2 model-feed frontend.
//
// The BAR0 offsets and values duplicated below come from
// crates/trueos-fpga-abi/src/lfm25_decode_feed.rs.  This block exposes the
// register values for a future top-level BAR mux; it does not add an interrupt
// fabric. irq_retire_o feeds the existing shared completion bridge and
// irq_ack_i is the existing BAR0 0x084 acknowledgement.
module truega_lfm25_feed_completion_slot (
    input  wire                 clk,
    input  wire                 reset_n,

    input  wire                 item_valid_i,
    input  wire                 item_ready_i,
    input  wire [7:0]           item_mode_i,
    input  wire [7:0]           item_layer_i,
    input  wire [31:0]          item_session_epoch_i,
    input  wire [31:0]          item_sequence_i,
    input  wire [31:0]          item_index_i,
    input  wire                 item_error_i,
    input  wire [31:0]          item_error_code_i,
    input  wire                 frontend_poisoned_i,

    input  wire                 irq_ack_i,
    input  wire                 control_write_i,
    input  wire [31:0]          control_value_i,
    output reg                  frontend_state_reset_o,

    output reg  [31:0]          state_o,
    output reg  [31:0]          retired_mode_layer_o,
    output reg  [31:0]          retired_session_epoch_o,
    output reg  [31:0]          retired_sequence_o,
    output reg  [31:0]          retired_item_o,
    output reg  [31:0]          error_code_o,
    output reg  [31:0]          completion_count_o,
    output reg                  irq_retire_o
);
    // BAR0 status offsets: state 0x294, tag 0x298, epoch 0x29c,
    // sequence 0x2a0, item 0x2a4, error 0x2a8, count 0x2ac,
    // and reset control 0x2b0.  The top-level owns address decoding.
    localparam [31:0] STATE_IDLE = 32'd0;
    localparam [31:0] STATE_BUSY = 32'd1;
    localparam [31:0] STATE_COMPLETE = 32'd2;
    localparam [31:0] STATE_FAILED = 32'd3;
    localparam [31:0] STATE_POISONED = 32'd4;
    localparam [31:0] RESET_MAGIC = 32'h3254_5352; // "RST2"

    // FeedState::Poisoned is authoritative.  The Rust ABI intentionally leaves
    // error values engine-defined; this fixed diagnostic distinguishes a
    // frontend protocol poison from a downstream compute failure.
    localparam [31:0] ERROR_FRONTEND_POISON = 32'hbad4_0001;

    reg poison_seen;
    wire state_terminal = state_o == STATE_COMPLETE
        || state_o == STATE_FAILED || state_o == STATE_POISONED;
    wire item_retires = item_valid_i && item_ready_i
        && !state_terminal;
    wire poison_rises_without_item = frontend_poisoned_i
        && !poison_seen && !item_valid_i && !state_terminal;
    wire valid_reset = control_write_i && control_value_i == RESET_MAGIC;

    always @(posedge clk) begin
        if (!reset_n) begin
            state_o <= STATE_IDLE;
            retired_mode_layer_o <= 32'h0000_ffff;
            retired_session_epoch_o <= 32'd0;
            retired_sequence_o <= 32'd0;
            retired_item_o <= 32'd0;
            error_code_o <= 32'd0;
            completion_count_o <= 32'd0;
            frontend_state_reset_o <= 1'b0;
            irq_retire_o <= 1'b0;
            poison_seen <= 1'b0;
        end else begin
            frontend_state_reset_o <= 1'b0;
            irq_retire_o <= 1'b0;

            // RST2 is an explicit abort/recovery command.  It wins over every
            // simultaneous event, clears the current status envelope, and
            // pulses the frontend reset.  The monotonic diagnostic count is
            // deliberately preserved. Unknown control values have no effect.
            if (valid_reset) begin
                state_o <= STATE_IDLE;
                retired_mode_layer_o <= 32'h0000_ffff;
                retired_session_epoch_o <= 32'd0;
                retired_sequence_o <= 32'd0;
                retired_item_o <= 32'd0;
                error_code_o <= 32'd0;
                frontend_state_reset_o <= 1'b1;
                poison_seen <= 1'b0;
            end else begin
                if (!frontend_poisoned_i)
                    poison_seen <= 1'b0;

                // A new retirement wins over a coincident stale IRQ ACK.
                if (item_retires) begin
                    state_o <= item_error_i ? STATE_FAILED : STATE_COMPLETE;
                    retired_mode_layer_o <= {16'd0, item_layer_i, item_mode_i};
                    retired_session_epoch_o <= item_session_epoch_i;
                    retired_sequence_o <= item_sequence_i;
                    retired_item_o <= item_index_i;
                    error_code_o <= item_error_i ? item_error_code_i : 32'd0;
                    completion_count_o <= completion_count_o + 1'b1;
                    irq_retire_o <= 1'b1;
                    if (frontend_poisoned_i)
                        poison_seen <= 1'b1;
                end else if (poison_rises_without_item) begin
                    state_o <= STATE_POISONED;
                    retired_mode_layer_o <= 32'h0000_ffff;
                    retired_session_epoch_o <= 32'd0;
                    retired_sequence_o <= 32'd0;
                    retired_item_o <= 32'd0;
                    error_code_o <= ERROR_FRONTEND_POISON;
                    completion_count_o <= completion_count_o + 1'b1;
                    irq_retire_o <= 1'b1;
                    poison_seen <= 1'b1;
                end else if (irq_ack_i && state_terminal) begin
                    // Retired tags and the diagnostic error remain readable as
                    // the last-completion envelope; only ownership returns idle.
                    state_o <= STATE_IDLE;
                end else if (state_o == STATE_IDLE && item_valid_i) begin
                    state_o <= STATE_BUSY;
                end
            end
        end
    end
endmodule
