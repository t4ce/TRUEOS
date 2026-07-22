// Standalone fixed resident-vector execution join for the LFM2.5 decode path.
//
// A handle is internal circuit metadata, not a host address:
//   [36:5] session epoch, [4] type (0=Q30, 1=Q8_0), [3:0] slot.
// Four typed operations exist. TokenEmbedding installs a new nonzero session
// epoch while accepting 32 streamed Q8_0 blocks. RMSNorm reads a resident Q30
// vector and accepts 1024 raw F32/BF16 weight words. ResidualAdd reads two
// resident Q30 vectors. The internal Import operation transactionally joins an
// externally computed ordered Q30[1024] vector back into a resident Q30 slot.
// No parser, DMA, TLB, or runtime machinery is present here.
module truega_lfm25_resident_vector_engine #(
    parameter integer Q30_SLOTS = 4,
    parameter integer Q8_SLOTS = 4
) (
    input  wire                 clk,
    input  wire                 reset_n,
    input  wire                 abort_i,

    input  wire                 command_valid_i,
    output wire                 command_ready_o,
    input  wire [1:0]           command_operation_i,
    input  wire [36:0]          command_source0_handle_i,
    input  wire [36:0]          command_source1_handle_i,
    input  wire [36:0]          command_destination_handle_i,

    input  wire                 embedding_block_valid_i,
    output wire                 embedding_block_ready_o,
    input  wire [4:0]           embedding_block_index_i,
    input  wire [271:0]         embedding_q8_block_i,

    input  wire                 weight_valid_i,
    output wire                 weight_ready_o,
    input  wire [9:0]           weight_index_i,
    input  wire                 weight_format_bf16_i,
    input  wire [31:0]          weight_bits_i,

    input  wire                 import_valid_i,
    output wire                 import_ready_o,
    input  wire [9:0]           import_index_i,
    input  wire signed [63:0]   import_q30_i,

    output wire                 result_valid_o,
    input  wire                 result_ready_i,
    output reg                  result_error_o,
    output reg  [36:0]          result_handle_o,

    input  wire                 inspect_valid_i,
    output wire                 inspect_ready_o,
    input  wire [36:0]          inspect_handle_i,
    input  wire [9:0]           inspect_index_i,
    output wire                 inspect_rsp_valid_o,
    input  wire                 inspect_rsp_ready_i,
    output reg                  inspect_rsp_error_o,
    output reg  [271:0]         inspect_rsp_data_o,

    output wire [31:0]          session_epoch_o,
    output wire                 busy_o
);
    localparam [1:0] OP_TOKEN_EMBEDDING = 2'd0;
    localparam [1:0] OP_RMSNORM         = 2'd1;
    localparam [1:0] OP_RESIDUAL_ADD    = 2'd2;
    // Internal fixed-circuit join only; not a public decode opcode.
    localparam [1:0] OP_IMPORT_Q30      = 2'd3;
    localparam       HANDLE_Q30         = 1'b0;
    localparam       HANDLE_Q8          = 1'b1;

    localparam [4:0] ST_IDLE                = 5'd0;
    localparam [4:0] ST_SESSION_BEGIN       = 5'd1;
    localparam [4:0] ST_SESSION_WAIT        = 5'd2;
    localparam [4:0] ST_EMBED_BLOCK         = 5'd3;
    localparam [4:0] ST_EMBED_START         = 5'd4;
    localparam [4:0] ST_EMBED_RUN           = 5'd5;
    localparam [4:0] ST_RMS_START           = 5'd6;
    localparam [4:0] ST_RMS_WEIGHT          = 5'd7;
    localparam [4:0] ST_RMS_READ_WAIT       = 5'd8;
    localparam [4:0] ST_RMS_FEED            = 5'd9;
    localparam [4:0] ST_RMS_OUTPUT          = 5'd10;
    localparam [4:0] ST_RMS_DONE            = 5'd11;
    localparam [4:0] ST_RES_START           = 5'd12;
    localparam [4:0] ST_RES_READ_A          = 5'd13;
    localparam [4:0] ST_RES_WAIT_A          = 5'd14;
    localparam [4:0] ST_RES_READ_B          = 5'd15;
    localparam [4:0] ST_RES_WAIT_B          = 5'd16;
    localparam [4:0] ST_RES_FEED            = 5'd17;
    localparam [4:0] ST_RES_OUTPUT          = 5'd18;
    localparam [4:0] ST_RES_DONE            = 5'd19;
    localparam [4:0] ST_INSPECT_WAIT        = 5'd20;
    localparam [4:0] ST_INSPECT_RESULT      = 5'd21;
    localparam [4:0] ST_RESULT              = 5'd22;
    localparam [4:0] ST_EMBED_TXN_BEGIN     = 5'd23;
    localparam [4:0] ST_EMBED_COMMIT        = 5'd24;
    localparam [4:0] ST_RMS_TXN_BEGIN       = 5'd25;
    localparam [4:0] ST_RMS_COMMIT          = 5'd26;
    localparam [4:0] ST_RES_TXN_BEGIN       = 5'd27;
    localparam [4:0] ST_RES_COMMIT          = 5'd28;
    localparam [4:0] ST_IMPORT_TXN_BEGIN    = 5'd29;
    localparam [4:0] ST_IMPORT_STREAM       = 5'd30;
    localparam [4:0] ST_IMPORT_COMMIT       = 5'd31;

    reg [4:0] state;
    reg [36:0] source0_handle;
    reg [36:0] source1_handle;
    reg [36:0] destination_handle;
    reg [4:0] block_index;
    reg [9:0] element_index;
    reg [271:0] embedding_block;
    reg weight_format;
    reg [31:0] weight_bits;
    reg worker_reset;

    wire worker_reset_n = reset_n && !worker_reset;
    wire command_accept = command_valid_i && command_ready_o;
    wire embedding_accept = embedding_block_valid_i
        && embedding_block_ready_o;
    wire weight_accept = weight_valid_i && weight_ready_o;
    wire import_accept = import_valid_i && import_ready_o;
    wire result_accept = result_valid_o && result_ready_i;
    wire inspect_accept = inspect_valid_i && inspect_ready_o;
    wire inspect_rsp_accept = inspect_rsp_valid_o && inspect_rsp_ready_i;

    wire [31:0] command_source0_epoch = command_source0_handle_i[36:5];
    wire command_source0_type = command_source0_handle_i[4];
    wire [3:0] command_source0_slot = command_source0_handle_i[3:0];
    wire [31:0] command_source1_epoch = command_source1_handle_i[36:5];
    wire command_source1_type = command_source1_handle_i[4];
    wire [3:0] command_source1_slot = command_source1_handle_i[3:0];
    wire [31:0] command_destination_epoch =
        command_destination_handle_i[36:5];
    wire command_destination_type = command_destination_handle_i[4];
    wire [3:0] command_destination_slot =
        command_destination_handle_i[3:0];

    wire [31:0] source0_epoch = source0_handle[36:5];
    wire [3:0] source0_slot = source0_handle[3:0];
    wire [31:0] source1_epoch = source1_handle[36:5];
    wire [3:0] source1_slot = source1_handle[3:0];
    wire [31:0] destination_epoch = destination_handle[36:5];
    wire [3:0] destination_slot = destination_handle[3:0];

    wire embedding_command_valid =
        command_source0_epoch != 32'd0
        && command_source0_epoch == command_destination_epoch
        && command_source0_type == HANDLE_Q8
        && command_destination_type == HANDLE_Q30
        && command_source0_slot < Q8_SLOTS
        && command_destination_slot < Q30_SLOTS;
    wire rms_command_valid =
        command_source0_epoch != 32'd0
        && command_source0_epoch == session_epoch_o
        && command_destination_epoch == session_epoch_o
        && command_source0_type == HANDLE_Q30
        && command_destination_type == HANDLE_Q8
        && command_source0_slot < Q30_SLOTS
        && command_destination_slot < Q8_SLOTS;
    wire residual_command_valid =
        command_source0_epoch != 32'd0
        && command_source0_epoch == session_epoch_o
        && command_source1_epoch == session_epoch_o
        && command_destination_epoch == session_epoch_o
        && command_source0_type == HANDLE_Q30
        && command_source1_type == HANDLE_Q30
        && command_destination_type == HANDLE_Q30
        && command_source0_slot < Q30_SLOTS
        && command_source1_slot < Q30_SLOTS
        && command_destination_slot < Q30_SLOTS
        && command_destination_slot != command_source0_slot
        && command_destination_slot != command_source1_slot;
    wire import_command_valid =
        command_source0_handle_i == 37'd0
        && command_source1_handle_i == 37'd0
        && command_destination_epoch != 32'd0
        && command_destination_epoch == session_epoch_o
        && command_destination_type == HANDLE_Q30
        && command_destination_slot < Q30_SLOTS;

    assign command_ready_o = state == ST_IDLE && !inspect_valid_i;
    assign embedding_block_ready_o = state == ST_EMBED_BLOCK;
    assign weight_ready_o = state == ST_RMS_WEIGHT;
    assign import_ready_o = state == ST_IMPORT_STREAM;
    assign result_valid_o = state == ST_RESULT;
    assign inspect_ready_o = state == ST_IDLE && !command_valid_i;
    assign inspect_rsp_valid_o = state == ST_INSPECT_RESULT;
    assign busy_o = state != ST_IDLE
        && state != ST_RESULT && state != ST_INSPECT_RESULT;

    // Resident tensor store.
    wire store_session_begin = state == ST_SESSION_BEGIN;
    wire store_session_done;
    wire store_session_error;
    wire store_q30_write_ready;
    wire store_q30_read_rsp;
    wire signed [63:0] store_q30_read_data;
    wire store_q30_read_error;
    wire store_q8_write_ready;
    wire store_q8_read_rsp;
    wire [271:0] store_q8_read_data;
    wire store_q8_read_error;
    wire store_q30_transaction_begin_ready;
    wire store_q30_transaction_commit_ready;
    wire store_q8_transaction_begin_ready;
    wire store_q8_transaction_commit_ready;
    wire store_q30_transaction_begin = state == ST_EMBED_TXN_BEGIN
        || state == ST_RES_TXN_BEGIN || state == ST_IMPORT_TXN_BEGIN;
    wire store_q30_transaction_commit = !abort_i
        && (state == ST_EMBED_COMMIT || state == ST_RES_COMMIT
            || state == ST_IMPORT_COMMIT);
    wire store_q8_transaction_begin = state == ST_RMS_TXN_BEGIN;
    wire store_q8_transaction_commit = !abort_i && state == ST_RMS_COMMIT;

    wire inspect_type = inspect_handle_i[4];
    wire [31:0] inspect_epoch = inspect_handle_i[36:5];
    wire [3:0] inspect_slot = inspect_handle_i[3:0];
    wire inspect_q30_shape_ok = inspect_type == HANDLE_Q30
        && inspect_slot < Q30_SLOTS;
    wire inspect_q8_shape_ok = inspect_type == HANDLE_Q8
        && inspect_slot < Q8_SLOTS && inspect_index_i[9:5] == 5'd0;
    wire inspect_shape_ok = inspect_q30_shape_ok || inspect_q8_shape_ok;

    wire rms_weight_index_ok = weight_index_i == element_index;
    wire import_index_ok = import_index_i == element_index;
    wire store_q30_read_valid =
        ((state == ST_RMS_WEIGHT) && weight_accept && rms_weight_index_ok)
        || state == ST_RES_READ_A
        || state == ST_RES_READ_B
        || (inspect_accept && inspect_q30_shape_ok);
    wire [3:0] store_q30_read_slot = state == ST_RES_READ_A
        ? source0_slot
        : state == ST_RES_READ_B ? source1_slot
        : state == ST_RMS_WEIGHT ? source0_slot : inspect_slot;
    wire [9:0] store_q30_read_index =
        inspect_accept ? inspect_index_i : element_index;
    wire [31:0] store_q30_read_epoch = inspect_accept
        ? inspect_epoch
        : state == ST_RES_READ_B ? source1_epoch : source0_epoch;
    wire store_q8_read_valid = inspect_accept && inspect_q8_shape_ok;

    // Embedding dequantizer.
    wire dequant_output_valid;
    wire [4:0] dequant_output_index;
    wire dequant_output_last;
    wire signed [63:0] dequant_output_q30;
    wire dequant_busy;
    wire dequant_done;
    wire dequant_error;
    wire [5:0] dequant_samples;
    wire dequant_output_ready = state == ST_EMBED_RUN
        && store_q30_write_ready;

    truega_q8_0_dequant_block_slot dequantize_embedding (
        .clk(clk), .reset_n(worker_reset_n),
        .start_i(state == ST_EMBED_START), .q8_block_i(embedding_block),
        .output_valid_o(dequant_output_valid),
        .output_ready_i(dequant_output_ready),
        .output_index_o(dequant_output_index),
        .output_last_o(dequant_output_last),
        .output_q30_o(dequant_output_q30),
        .busy_o(dequant_busy), .done_o(dequant_done),
        .error_o(dequant_error), .samples_retired_o(dequant_samples)
    );

    // Fixed RMSNorm vector slot.
    reg rms_input_valid;
    reg signed [63:0] rms_input_q30;
    wire rms_input_ready;
    wire rms_output_valid;
    wire [4:0] rms_output_block;
    wire rms_output_last;
    wire [271:0] rms_output_q8;
    wire rms_busy;
    wire rms_done;
    wire rms_error;
    wire [10:0] rms_inputs;
    wire [5:0] rms_blocks;
    wire signed [63:0] rms_mean_square;
    wire signed [63:0] rms_inv;
    wire rms_input_accept = rms_input_valid && rms_input_ready;
    wire rms_output_ready = state == ST_RMS_OUTPUT
        && store_q8_write_ready;
    wire rms_output_accept = rms_output_valid && rms_output_ready;

    truega_lfm25_rmsnorm_vector_slot rmsnorm (
        .clk(clk), .reset_n(worker_reset_n),
        .start_i(state == ST_RMS_START),
        .input_valid_i(rms_input_valid), .input_ready_o(rms_input_ready),
        .x_q30_i(rms_input_q30),
        .weight_format_bf16_i(weight_format),
        .weight_bits_i(weight_bits),
        .output_valid_o(rms_output_valid),
        .output_ready_i(rms_output_ready),
        .output_block_index_o(rms_output_block),
        .output_last_o(rms_output_last), .output_q8_block_o(rms_output_q8),
        .busy_o(rms_busy), .done_o(rms_done), .error_o(rms_error),
        .inputs_accepted_o(rms_inputs), .blocks_retired_o(rms_blocks),
        .mean_square_q30_o(rms_mean_square), .inv_rms_q30_o(rms_inv)
    );

    // Fixed residual vector slot.
    reg signed [63:0] residual_a;
    reg signed [63:0] residual_b;
    reg residual_input_valid;
    wire residual_input_ready;
    wire residual_output_valid;
    wire [9:0] residual_output_index;
    wire signed [63:0] residual_output_q30;
    wire residual_busy;
    wire residual_done;
    wire residual_error;
    wire [10:0] residual_elements;
    wire residual_input_accept = residual_input_valid
        && residual_input_ready;
    wire residual_output_ready = state == ST_RES_OUTPUT
        && store_q30_write_ready;
    wire residual_output_accept = residual_output_valid
        && residual_output_ready;

    truega_lfm25_residual_vector_slot residual_add (
        .clk(clk), .reset_n(worker_reset_n),
        .start_i(state == ST_RES_START),
        .input_valid_i(residual_input_valid),
        .input_ready_o(residual_input_ready),
        .residual_q30_i(residual_a), .branch_q30_i(residual_b),
        .output_valid_o(residual_output_valid),
        .output_ready_i(residual_output_ready),
        .output_index_o(residual_output_index),
        .output_q30_o(residual_output_q30),
        .busy_o(residual_busy), .done_o(residual_done),
        .error_o(residual_error), .elements_retired_o(residual_elements)
    );

    wire store_q30_write_valid =
        (state == ST_EMBED_RUN && dequant_output_valid)
        || (state == ST_RES_OUTPUT && residual_output_valid)
        || (state == ST_IMPORT_STREAM && import_valid_i
            && import_index_ok);
    wire [3:0] store_q30_write_slot = destination_slot;
    wire [9:0] store_q30_write_index = state == ST_EMBED_RUN
        ? {block_index, dequant_output_index}
        : state == ST_RES_OUTPUT ? residual_output_index : import_index_i;
    wire signed [63:0] store_q30_write_data = state == ST_EMBED_RUN
        ? dequant_output_q30
        : state == ST_RES_OUTPUT ? residual_output_q30 : import_q30_i;
    wire store_q8_write_valid = state == ST_RMS_OUTPUT && rms_output_valid;

    truega_lfm25_resident_tensor_store #(
        .Q30_SLOTS(Q30_SLOTS), .Q8_SLOTS(Q8_SLOTS)
    ) store (
        .clk(clk), .reset_n(reset_n),
        .session_begin_i(store_session_begin),
        .session_begin_epoch_i(destination_epoch),
        .session_epoch_o(session_epoch_o),
        .session_begin_done_o(store_session_done),
        .session_begin_error_o(store_session_error),
        .q30_transaction_begin_i(store_q30_transaction_begin),
        .q30_transaction_commit_i(store_q30_transaction_commit),
        .q30_transaction_slot_i(destination_slot),
        .q30_transaction_epoch_i(destination_epoch),
        .q30_transaction_begin_ready_o(
            store_q30_transaction_begin_ready),
        .q30_transaction_commit_ready_o(
            store_q30_transaction_commit_ready),
        .q8_transaction_begin_i(store_q8_transaction_begin),
        .q8_transaction_commit_i(store_q8_transaction_commit),
        .q8_transaction_slot_i(destination_slot),
        .q8_transaction_epoch_i(destination_epoch),
        .q8_transaction_begin_ready_o(store_q8_transaction_begin_ready),
        .q8_transaction_commit_ready_o(store_q8_transaction_commit_ready),
        .q30_write_valid_i(store_q30_write_valid),
        .q30_write_slot_i(store_q30_write_slot),
        .q30_write_index_i(store_q30_write_index),
        .q30_write_data_i(store_q30_write_data),
        .q30_write_epoch_i(destination_epoch),
        .q30_write_ready_o(store_q30_write_ready),
        .q30_read_valid_i(store_q30_read_valid),
        .q30_read_slot_i(store_q30_read_slot),
        .q30_read_index_i(store_q30_read_index),
        .q30_read_epoch_i(store_q30_read_epoch),
        .q30_read_rsp_valid_o(store_q30_read_rsp),
        .q30_read_data_o(store_q30_read_data),
        .q30_read_error_o(store_q30_read_error),
        .q8_write_valid_i(store_q8_write_valid),
        .q8_write_slot_i(destination_slot),
        .q8_write_block_i(rms_output_block),
        .q8_write_data_i(rms_output_q8),
        .q8_write_epoch_i(destination_epoch),
        .q8_write_ready_o(store_q8_write_ready),
        .q8_read_valid_i(store_q8_read_valid),
        .q8_read_slot_i(inspect_slot),
        .q8_read_block_i(inspect_index_i[4:0]),
        .q8_read_epoch_i(inspect_epoch),
        .q8_read_rsp_valid_o(store_q8_read_rsp),
        .q8_read_data_o(store_q8_read_data),
        .q8_read_error_o(store_q8_read_error)
    );

    always @(posedge clk) begin
        if (!reset_n) begin
            state <= ST_IDLE;
            source0_handle <= 37'd0;
            source1_handle <= 37'd0;
            destination_handle <= 37'd0;
            block_index <= 5'd0;
            element_index <= 10'd0;
            embedding_block <= 272'd0;
            weight_format <= 1'b0;
            weight_bits <= 32'd0;
            worker_reset <= 1'b0;
            rms_input_valid <= 1'b0;
            rms_input_q30 <= 64'sd0;
            residual_a <= 64'sd0;
            residual_b <= 64'sd0;
            residual_input_valid <= 1'b0;
            result_error_o <= 1'b0;
            result_handle_o <= 37'd0;
            inspect_rsp_error_o <= 1'b0;
            inspect_rsp_data_o <= 272'd0;
        end else begin
            worker_reset <= 1'b0;
            if (abort_i && busy_o) begin
                // Destination validity was cleared by transaction begin.
                // Reset only worker state; payload RAM remains untouched and
                // the uncommitted slot stays unreadable.
                worker_reset <= 1'b1;
                rms_input_valid <= 1'b0;
                residual_input_valid <= 1'b0;
                result_error_o <= 1'b1;
                result_handle_o <= 37'd0;
                state <= ST_RESULT;
            end else case (state)
                ST_IDLE: begin
                    rms_input_valid <= 1'b0;
                    residual_input_valid <= 1'b0;
                    if (command_accept) begin
                        source0_handle <= command_source0_handle_i;
                        source1_handle <= command_source1_handle_i;
                        destination_handle <= command_destination_handle_i;
                        result_error_o <= 1'b0;
                        // A destination handle is not published until the
                        // full-vector transaction has committed.
                        result_handle_o <= 37'd0;
                        block_index <= 5'd0;
                        element_index <= 10'd0;
                        if (command_operation_i == OP_TOKEN_EMBEDDING
                                && embedding_command_valid) begin
                            state <= ST_SESSION_BEGIN;
                        end else if (command_operation_i == OP_RMSNORM
                                && rms_command_valid) begin
                            state <= ST_RMS_TXN_BEGIN;
                        end else if (command_operation_i == OP_RESIDUAL_ADD
                                && residual_command_valid) begin
                            state <= ST_RES_TXN_BEGIN;
                        end else if (command_operation_i == OP_IMPORT_Q30
                                && import_command_valid) begin
                            state <= ST_IMPORT_TXN_BEGIN;
                        end else begin
                            result_error_o <= 1'b1;
                            result_handle_o <= 37'd0;
                            state <= ST_RESULT;
                        end
                    end else if (inspect_accept) begin
                        inspect_rsp_error_o <= 1'b0;
                        inspect_rsp_data_o <= 272'd0;
                        if (!inspect_shape_ok) begin
                            inspect_rsp_error_o <= 1'b1;
                            state <= ST_INSPECT_RESULT;
                        end else begin
                            state <= ST_INSPECT_WAIT;
                        end
                    end
                end

                ST_SESSION_BEGIN: begin
                    state <= ST_SESSION_WAIT;
                end

                ST_SESSION_WAIT: begin
                    if (store_session_done) begin
                        if (store_session_error) begin
                            result_error_o <= 1'b1;
                            result_handle_o <= 37'd0;
                            state <= ST_RESULT;
                        end else begin
                            block_index <= 5'd0;
                            state <= ST_EMBED_TXN_BEGIN;
                        end
                    end
                end

                ST_EMBED_TXN_BEGIN: begin
                    if (store_q30_transaction_begin_ready)
                        state <= ST_EMBED_BLOCK;
                end

                ST_EMBED_BLOCK: begin
                    if (embedding_accept) begin
                        if (embedding_block_index_i != block_index) begin
                            result_error_o <= 1'b1;
                            result_handle_o <= 37'd0;
                            state <= ST_RESULT;
                        end else begin
                            embedding_block <= embedding_q8_block_i;
                            state <= ST_EMBED_START;
                        end
                    end
                end

                ST_EMBED_START: begin
                    state <= ST_EMBED_RUN;
                end

                ST_EMBED_RUN: begin
                    if (dequant_done) begin
                        if (dequant_error || dequant_samples != 6'd32) begin
                            worker_reset <= 1'b1;
                            result_error_o <= 1'b1;
                            result_handle_o <= 37'd0;
                            state <= ST_RESULT;
                        end else if (block_index == 5'd31) begin
                            state <= ST_EMBED_COMMIT;
                        end else begin
                            block_index <= block_index + 5'd1;
                            state <= ST_EMBED_BLOCK;
                        end
                    end
                end

                ST_EMBED_COMMIT: begin
                    if (store_q30_transaction_commit_ready) begin
                        result_handle_o <= destination_handle;
                        state <= ST_RESULT;
                    end
                end

                ST_RMS_TXN_BEGIN: begin
                    if (store_q8_transaction_begin_ready)
                        state <= ST_RMS_START;
                end

                ST_RMS_START: begin
                    state <= ST_RMS_WEIGHT;
                end

                ST_RMS_WEIGHT: begin
                    if (weight_accept) begin
                        if (!rms_weight_index_ok) begin
                            worker_reset <= 1'b1;
                            result_error_o <= 1'b1;
                            result_handle_o <= 37'd0;
                            state <= ST_RESULT;
                        end else begin
                            weight_format <= weight_format_bf16_i;
                            weight_bits <= weight_bits_i;
                            state <= ST_RMS_READ_WAIT;
                        end
                    end
                end

                ST_RMS_READ_WAIT: begin
                    if (store_q30_read_error) begin
                        worker_reset <= 1'b1;
                        result_error_o <= 1'b1;
                        result_handle_o <= 37'd0;
                        state <= ST_RESULT;
                    end else if (store_q30_read_rsp) begin
                        rms_input_q30 <= store_q30_read_data;
                        rms_input_valid <= 1'b1;
                        state <= ST_RMS_FEED;
                    end
                end

                ST_RMS_FEED: begin
                    if (rms_input_accept) begin
                        rms_input_valid <= 1'b0;
                        if (element_index == 10'd1023) begin
                            state <= ST_RMS_OUTPUT;
                        end else begin
                            element_index <= element_index + 10'd1;
                            state <= ST_RMS_WEIGHT;
                        end
                    end
                end

                ST_RMS_OUTPUT: begin
                    if (rms_done && rms_error) begin
                        worker_reset <= 1'b1;
                        result_error_o <= 1'b1;
                        result_handle_o <= 37'd0;
                        state <= ST_RESULT;
                    end else if (rms_output_accept && rms_output_last) begin
                        state <= ST_RMS_DONE;
                    end
                end

                ST_RMS_DONE: begin
                    if (rms_done) begin
                        if (rms_error || rms_inputs != 11'd1024
                                || rms_blocks != 6'd32) begin
                            result_error_o <= 1'b1;
                            result_handle_o <= 37'd0;
                            state <= ST_RESULT;
                        end else begin
                            state <= ST_RMS_COMMIT;
                        end
                    end
                end

                ST_RMS_COMMIT: begin
                    if (store_q8_transaction_commit_ready) begin
                        result_handle_o <= destination_handle;
                        state <= ST_RESULT;
                    end
                end

                ST_RES_TXN_BEGIN: begin
                    if (store_q30_transaction_begin_ready)
                        state <= ST_RES_START;
                end

                ST_RES_START: begin
                    state <= ST_RES_READ_A;
                end

                ST_RES_READ_A: begin
                    state <= ST_RES_WAIT_A;
                end

                ST_RES_WAIT_A: begin
                    if (store_q30_read_error) begin
                        worker_reset <= 1'b1;
                        result_error_o <= 1'b1;
                        result_handle_o <= 37'd0;
                        state <= ST_RESULT;
                    end else if (store_q30_read_rsp) begin
                        residual_a <= store_q30_read_data;
                        state <= ST_RES_READ_B;
                    end
                end

                ST_RES_READ_B: begin
                    state <= ST_RES_WAIT_B;
                end

                ST_RES_WAIT_B: begin
                    if (store_q30_read_error) begin
                        worker_reset <= 1'b1;
                        result_error_o <= 1'b1;
                        result_handle_o <= 37'd0;
                        state <= ST_RESULT;
                    end else if (store_q30_read_rsp) begin
                        residual_b <= store_q30_read_data;
                        residual_input_valid <= 1'b1;
                        state <= ST_RES_FEED;
                    end
                end

                ST_RES_FEED: begin
                    if (residual_input_accept) begin
                        residual_input_valid <= 1'b0;
                        state <= ST_RES_OUTPUT;
                    end
                end

                ST_RES_OUTPUT: begin
                    if (residual_done && residual_error) begin
                        worker_reset <= 1'b1;
                        result_error_o <= 1'b1;
                        result_handle_o <= 37'd0;
                        state <= ST_RESULT;
                    end else if (residual_output_accept) begin
                        if (residual_output_index == 10'd1023) begin
                            state <= ST_RES_DONE;
                        end else begin
                            element_index <= element_index + 10'd1;
                            state <= ST_RES_READ_A;
                        end
                    end
                end

                ST_RES_DONE: begin
                    if (residual_done) begin
                        if (residual_error || residual_elements != 11'd1024) begin
                            result_error_o <= 1'b1;
                            result_handle_o <= 37'd0;
                            state <= ST_RESULT;
                        end else begin
                            state <= ST_RES_COMMIT;
                        end
                    end
                end

                ST_RES_COMMIT: begin
                    if (store_q30_transaction_commit_ready) begin
                        result_handle_o <= destination_handle;
                        state <= ST_RESULT;
                    end
                end

                ST_IMPORT_TXN_BEGIN: begin
                    // Begin invalidates any older value in the destination.
                    if (store_q30_transaction_begin_ready)
                        state <= ST_IMPORT_STREAM;
                end

                ST_IMPORT_STREAM: begin
                    if (import_accept) begin
                        if (!import_index_ok || !store_q30_write_ready) begin
                            result_error_o <= 1'b1;
                            result_handle_o <= 37'd0;
                            state <= ST_RESULT;
                        end else if (element_index == 10'd1023) begin
                            state <= ST_IMPORT_COMMIT;
                        end else begin
                            element_index <= element_index + 10'd1;
                        end
                    end
                end

                ST_IMPORT_COMMIT: begin
                    if (store_q30_transaction_commit_ready) begin
                        result_handle_o <= destination_handle;
                        state <= ST_RESULT;
                    end
                end

                ST_INSPECT_WAIT: begin
                    if (store_q30_read_error || store_q8_read_error) begin
                        inspect_rsp_error_o <= 1'b1;
                        inspect_rsp_data_o <= 272'd0;
                        state <= ST_INSPECT_RESULT;
                    end else if (store_q30_read_rsp) begin
                        inspect_rsp_data_o <= {{208{store_q30_read_data[63]}},
                                               store_q30_read_data};
                        state <= ST_INSPECT_RESULT;
                    end else if (store_q8_read_rsp) begin
                        inspect_rsp_data_o <= store_q8_read_data;
                        state <= ST_INSPECT_RESULT;
                    end
                end

                ST_INSPECT_RESULT: begin
                    if (inspect_rsp_accept)
                        state <= ST_IDLE;
                end

                ST_RESULT: begin
                    if (result_accept)
                        state <= ST_IDLE;
                end

                default: begin
                    worker_reset <= 1'b1;
                    result_error_o <= 1'b1;
                    result_handle_o <= 37'd0;
                    state <= ST_RESULT;
                end
            endcase
        end
    end

    wire unused_dequant_busy = dequant_busy ^ dequant_output_last;
    wire unused_rms = rms_busy ^ rms_mean_square[0] ^ rms_inv[0];
    wire unused_residual_busy = residual_busy;
endmodule
