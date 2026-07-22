// Board-facing fixed LFM2.5 decode endpoint.
//
// This wrapper is deliberately only structural glue.  TGF2 BAR2 publication is
// consumed by the fixed 99-operation controller; matching TGD1 doorbells select
// its already-fused operations.  Both retirement sources share one interrupt
// pulse/ACK domain.  There is no instruction store, DMA engine, soft processor,
// TLB, or runtime graph machinery here.
module truega_lfm25_decode_endpoint #(
    parameter integer FAST_SCHEDULE_SIM = 0
) (
    input  wire                 clk,
    input  wire                 reset_n,

    input  wire                 bar2_write_valid_i,
    input  wire [18:0]          bar2_write_address_i,
    input  wire [31:0]          bar2_write_data_i,
    input  wire [3:0]           bar2_write_strobe_i,
    output wire                 bar2_write_ready_o,

    // The existing BAR0 0x084 shared completion acknowledgement and TGF2
    // 0x2b0 control write.  RST2 resets the complete decode session.
    input  wire                 irq_ack_i,
    input  wire                 feed_control_write_i,
    input  wire [31:0]          feed_control_value_i,

    // TGD1 BAR0 request envelope.
    input  wire [31:0]          decode_command_i,
    input  wire [31:0]          decode_position_i,
    input  wire [31:0]          decode_session_epoch_i,
    input  wire                 decode_doorbell_i,
    input  wire [31:0]          decode_doorbell_value_i,

    // One pulse into the existing shared completion bridge.  The split pulses
    // are exported only as board-level diagnostics.
    output wire                 irq_retire_o,
    output wire                 feed_irq_retire_o,
    output wire                 decode_irq_retire_o,
    output wire                 decode_irq_owned_o,

    // Exact TGF2 capability and terminal envelope.
    output wire [31:0]          feed_capability_magic_o,
    output wire [31:0]          feed_capability_version_record_bytes_o,
    output wire [31:0]          feed_capability_bits_o,
    output wire [31:0]          feed_capability_model_generation_o,
    output wire [31:0]          feed_capability_shape_set_tag_o,
    output wire [31:0]          feed_state_o,
    output wire [31:0]          feed_retired_mode_layer_o,
    output wire [31:0]          feed_retired_session_epoch_o,
    output wire [31:0]          feed_retired_sequence_o,
    output wire [31:0]          feed_retired_item_o,
    output wire [31:0]          feed_error_code_o,
    output wire [31:0]          feed_completion_count_o,

    // Exact TGD1 capability, state, and result envelope.
    output wire [31:0]          decode_capability_magic_o,
    output wire [31:0]          decode_capability_bits_o,
    output wire [31:0]          decode_state_o,
    output wire [31:0]          decode_result0_o,
    output wire [31:0]          decode_result1_o,
    output wire signed [63:0]   decode_argmax_score_q30_o,

    // Fixed-controller diagnostics; these are not protocol inputs.
    output wire [6:0]           operation_ordinal_o,
    output wire [31:0]          feed_items_retired_o,
    output wire                 controller_poisoned_o,
    output wire                 controller_busy_o,
    output wire                 frontend_poisoned_o
);
    localparam [31:0] FEED_STATE_IDLE = 32'd0;

    wire frontend_reset;
    wire frontend_bar_ready;
    wire frontend_item_valid;
    wire controller_feed_item_ready;
    wire [7:0] item_mode;
    wire [7:0] item_layer;
    wire [7:0] item_lane_mask;
    wire [7:0] item_payload_format;
    wire [31:0] item_session_epoch;
    wire [31:0] item_sequence;
    wire [31:0] item_position;
    wire [31:0] item_token;
    wire [31:0] item_index;
    wire [15:0] item_stages_per_lane;
    wire [15:0] item_last_stage_slot;
    wire [15:0] item_payload_bytes_per_stage;
    wire [31:0] item_stage_generation;
    wire [31:0] item_shape_tag;

    wire payload_read_valid;
    wire [1:0] payload_read_bank;
    wire [7:0] payload_read_slot;
    wire [3:0] payload_read_word;
    wire payload_read_ready;
    wire payload_read_rsp_valid;
    wire [31:0] payload_read_data;
    wire payload_read_error;

    wire controller_feed_item_error;
    wire [31:0] controller_feed_item_error_code;
    wire controller_engine_done;
    wire controller_engine_error;
    wire [31:0] controller_engine_error_code;
    wire [7:0] controller_engine_result_slot;
    wire [31:0] controller_engine_result_position;
    wire [31:0] controller_engine_argmax_token;
    wire [31:0] controller_engine_argmax_rows;
    wire signed [63:0] controller_engine_argmax_score;

    wire dispatch_execute_start;
    wire [3:0] dispatch_execute_operation;
    wire [7:0] dispatch_execute_layer;
    wire [31:0] dispatch_execute_position;
    wire [7:0] dispatch_execute_input_slot;
    wire [7:0] dispatch_execute_residual_slot;
    wire [31:0] dispatch_execute_session_epoch;
    wire dispatch_execute_session_begin;
    wire dispatch_retire;

    // A feed window is open only while the controller's exact schedule expects
    // another TGF2 item.  In particular, it closes after the last feed item of
    // an operation and reopens only after the matching TGD1 command retires (or
    // remains closed across the two no-feed residual operations).  This keeps a
    // premature next feed from filling the frontend while the controller waits
    // for its mandatory TGD1 handoff.
    function automatic feed_item_finishes_operation;
        input [7:0] mode;
        input [31:0] item;
        begin
            case (mode)
                8'd0, 8'd1, 8'd2, 8'd3:
                    feed_item_finishes_operation = item == 0;
                8'd6, 8'd12, 8'd14:
                    feed_item_finishes_operation = item == 1023;
                8'd15:
                    feed_item_finishes_operation = item == 65535;
                default:
                    feed_item_finishes_operation = 1'b0;
            endcase
        end
    endfunction

    function automatic ordinal_expects_feed;
        input [6:0] ordinal;
        reg [6:0] phase;
        begin
            ordinal_expects_feed = 1'b1;
            if (ordinal >= 1 && ordinal <= 96) begin
                phase = (ordinal - 1'b1) % 6;
                // OperatorResidual and FfnResidual are the two TGD1-only
                // operations in every layer's fixed six-operation schedule.
                if (phase == 2 || phase == 5)
                    ordinal_expects_feed = 1'b0;
            end
        end
    endfunction

    reg controller_feed_window;
    always @(posedge clk) begin
        if (!reset_n || frontend_reset)
            controller_feed_window <= 1'b1;
        else if (controller_feed_item_ready && frontend_item_valid
                && feed_item_finishes_operation(item_mode, item_index))
            controller_feed_window <= 1'b0;
        else if (dispatch_retire && decode_state_o == 32'd2)
            controller_feed_window <= ordinal_expects_feed(
                operation_ordinal_o);
    end

    // Feed status owns BAR2 until software has consumed and acknowledged its
    // terminal envelope.  A pending TGD1 completion owns the same physical IRQ
    // lane.  The frontend itself supplies the one-entry publication buffer; a
    // poisoned controller is never allowed to accumulate another BAR package.
    wire feed_admission = feed_state_o == FEED_STATE_IDLE
        && !decode_irq_owned_o && decode_state_o != 32'd1
        && controller_feed_window && !controller_poisoned_o;
    wire frontend_bar_valid = bar2_write_valid_i && feed_admission;
    assign bar2_write_ready_o = frontend_bar_ready && feed_admission;

    truega_lfm25_feed_frontend frontend (
        .clk(clk), .reset_n(reset_n), .state_reset_i(frontend_reset),
        .bar2_write_valid_i(frontend_bar_valid),
        .bar2_write_address_i(bar2_write_address_i),
        .bar2_write_data_i(bar2_write_data_i),
        .bar2_write_strobe_i(bar2_write_strobe_i),
        .bar2_write_ready_o(frontend_bar_ready),
        .capability_magic_o(feed_capability_magic_o),
        .capability_version_record_bytes_o(
            feed_capability_version_record_bytes_o),
        .capability_bits_o(feed_capability_bits_o),
        .capability_model_generation_o(feed_capability_model_generation_o),
        .capability_shape_set_tag_o(feed_capability_shape_set_tag_o),
        .item_valid_o(frontend_item_valid),
        .item_ready_i(controller_feed_item_ready),
        .item_mode_o(item_mode), .item_layer_o(item_layer),
        .item_lane_mask_o(item_lane_mask),
        .item_payload_format_o(item_payload_format),
        .item_session_epoch_o(item_session_epoch),
        .item_sequence_o(item_sequence), .item_position_o(item_position),
        .item_token_o(item_token), .item_index_o(item_index),
        .item_stages_per_lane_o(item_stages_per_lane),
        .item_last_stage_slot_o(item_last_stage_slot),
        .item_payload_bytes_per_stage_o(item_payload_bytes_per_stage),
        .item_stage_generation_o(item_stage_generation),
        .item_shape_tag_o(item_shape_tag),
        .payload_read_valid_i(payload_read_valid),
        .payload_read_bank_i(payload_read_bank),
        .payload_read_slot_i(payload_read_slot),
        .payload_read_word_i(payload_read_word),
        .payload_read_ready_o(payload_read_ready),
        .payload_read_rsp_valid_o(payload_read_rsp_valid),
        .payload_read_data_o(payload_read_data),
        .payload_read_error_o(payload_read_error),
        .poisoned_o(frontend_poisoned_o)
    );

    truega_lfm25_feed_completion_slot feed_completion (
        .clk(clk), .reset_n(reset_n),
        .item_valid_i(frontend_item_valid),
        .item_ready_i(controller_feed_item_ready),
        .item_mode_i(item_mode), .item_layer_i(item_layer),
        .item_session_epoch_i(item_session_epoch),
        .item_sequence_i(item_sequence), .item_index_i(item_index),
        .item_error_i(controller_feed_item_error),
        .item_error_code_i(controller_feed_item_error_code),
        .frontend_poisoned_i(frontend_poisoned_o),
        .irq_ack_i(irq_ack_i),
        .control_write_i(feed_control_write_i),
        .control_value_i(feed_control_value_i),
        .frontend_state_reset_o(frontend_reset),
        .state_o(feed_state_o),
        .retired_mode_layer_o(feed_retired_mode_layer_o),
        .retired_session_epoch_o(feed_retired_session_epoch_o),
        .retired_sequence_o(feed_retired_sequence_o),
        .retired_item_o(feed_retired_item_o),
        .error_code_o(feed_error_code_o),
        .completion_count_o(feed_completion_count_o),
        .irq_retire_o(feed_irq_retire_o)
    );

    truega_lfm25_fixed_decode_controller #(
        .FAST_SCHEDULE_SIM(FAST_SCHEDULE_SIM)
    ) controller (
        .clk(clk), .reset_n(reset_n), .clear_i(frontend_reset),
        .abort_i(1'b0),
        .feed_item_valid_i(frontend_item_valid),
        .feed_item_ready_o(controller_feed_item_ready),
        .feed_item_mode_i(item_mode), .feed_item_layer_i(item_layer),
        .feed_item_lane_mask_i(item_lane_mask),
        .feed_item_payload_format_i(item_payload_format),
        .feed_item_session_epoch_i(item_session_epoch),
        .feed_item_sequence_i(item_sequence),
        .feed_item_position_i(item_position),
        .feed_item_token_i(item_token), .feed_item_index_i(item_index),
        .feed_item_stages_per_lane_i(item_stages_per_lane),
        .feed_item_payload_bytes_per_stage_i(item_payload_bytes_per_stage),
        .feed_frontend_poisoned_i(frontend_poisoned_o),
        .feed_item_error_o(controller_feed_item_error),
        .feed_item_error_code_o(controller_feed_item_error_code),
        .payload_read_valid_o(payload_read_valid),
        .payload_read_bank_o(payload_read_bank),
        .payload_read_slot_o(payload_read_slot),
        .payload_read_word_o(payload_read_word),
        .payload_read_ready_i(payload_read_ready),
        .payload_read_rsp_valid_i(payload_read_rsp_valid),
        .payload_read_data_i(payload_read_data),
        .payload_read_error_i(payload_read_error),
        .execute_start_i(dispatch_execute_start),
        .execute_operation_i(dispatch_execute_operation),
        .execute_layer_i(dispatch_execute_layer),
        .execute_position_i(dispatch_execute_position),
        .execute_input_slot_i(dispatch_execute_input_slot),
        .execute_residual_slot_i(dispatch_execute_residual_slot),
        .execute_session_epoch_i(dispatch_execute_session_epoch),
        .execute_session_begin_i(dispatch_execute_session_begin),
        .engine_done_o(controller_engine_done),
        .engine_error_o(controller_engine_error),
        .engine_error_code_o(controller_engine_error_code),
        .engine_result_slot_o(controller_engine_result_slot),
        .engine_result_position_o(controller_engine_result_position),
        .engine_argmax_token_o(controller_engine_argmax_token),
        .engine_argmax_rows_o(controller_engine_argmax_rows),
        .engine_argmax_score_q30_o(controller_engine_argmax_score),
        .operation_ordinal_o(operation_ordinal_o),
        .feed_items_retired_o(feed_items_retired_o),
        .poisoned_o(controller_poisoned_o), .busy_o(controller_busy_o)
    );

    // RST2 is the only session reset.  Besides clearing frontend/controller
    // state, it resets the dispatcher's resident-session admission latch.  The
    // pulse is synchronous and does not suppress stable capability constants.
    wire dispatch_reset_n = reset_n && !frontend_reset;
    wire admitted_decode_doorbell = decode_doorbell_i
        && feed_state_o == FEED_STATE_IDLE && !decode_irq_owned_o
        && !controller_feed_window && !controller_poisoned_o;
    truega_lfm25_decode_dispatch #(.ENABLE(1)) dispatch (
        .clk(clk), .reset_n(dispatch_reset_n),
        .command_i(decode_command_i), .position_i(decode_position_i),
        .session_epoch_i(decode_session_epoch_i),
        .doorbell_i(admitted_decode_doorbell),
        .doorbell_value_i(decode_doorbell_value_i),
        .capability_magic_o(decode_capability_magic_o),
        .capability_bits_o(decode_capability_bits_o),
        .state_o(decode_state_o), .result0_o(decode_result0_o),
        .result1_o(decode_result1_o),
        .argmax_score_q30_o(decode_argmax_score_q30_o),
        .execute_start_o(dispatch_execute_start),
        .execute_operation_o(dispatch_execute_operation),
        .execute_layer_o(dispatch_execute_layer),
        .execute_position_o(dispatch_execute_position),
        .execute_input_slot_o(dispatch_execute_input_slot),
        .execute_residual_slot_o(dispatch_execute_residual_slot),
        .execute_session_epoch_o(dispatch_execute_session_epoch),
        .execute_session_begin_o(dispatch_execute_session_begin),
        .engine_done_i(controller_engine_done),
        .engine_error_i(controller_engine_error),
        .engine_error_code_i(controller_engine_error_code),
        .engine_result_slot_i(controller_engine_result_slot),
        .engine_result_position_i(controller_engine_result_position),
        .engine_argmax_token_i(controller_engine_argmax_token),
        .engine_argmax_rows_i(controller_engine_argmax_rows),
        .engine_argmax_score_q30_i(controller_engine_argmax_score),
        .retire_o(dispatch_retire)
    );

    reg decode_irq_owned;
    always @(posedge clk) begin
        if (!reset_n || frontend_reset)
            decode_irq_owned <= 1'b0;
        else if (dispatch_retire)
            // A new completion wins over a coincident stale shared ACK.
            decode_irq_owned <= 1'b1;
        else if (irq_ack_i)
            decode_irq_owned <= 1'b0;
    end

    assign decode_irq_retire_o = dispatch_retire;
    // Include the retirement cycle itself so BAR2/doorbell admission closes in
    // the same cycle that the shared bridge observes the new owner.
    assign decode_irq_owned_o = decode_irq_owned || dispatch_retire;
    assign irq_retire_o = feed_irq_retire_o || decode_irq_retire_o;
endmodule
