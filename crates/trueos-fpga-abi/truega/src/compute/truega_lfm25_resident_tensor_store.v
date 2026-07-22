// Fixed FPGA-resident tensor storage for the LFM2.5 AOT decode path.
//
// The storage slots are selected by fixed operation wiring.  They are not host
// addresses and this block does not parse commands. A changing host-minted
// nonzero session epoch makes every value from an earlier logical
// decode session unusable without clearing the payload RAMs. PCIe connection
// generation is checked by the kernel worker; the epoch is checked again here
// at the circuit boundary. A slot becomes readable only after an explicit
// begin, every sequential payload index, and a full-vector commit.
//
// Payload RAMs are intentionally unreset and have synchronous reads.  Only the
// small validity/epoch metadata is reset.
module truega_lfm25_resident_tensor_store #(
    parameter integer Q30_SLOTS = 4,
    parameter integer Q8_SLOTS = 4
) (
    input  wire                 clk,
    input  wire                 reset_n,

    input  wire                 session_begin_i,
    input  wire [31:0]          session_begin_epoch_i,
    output reg  [31:0]          session_epoch_o,
    output reg                  session_begin_done_o,
    output reg                  session_begin_error_o,

    input  wire                 q30_transaction_begin_i,
    input  wire                 q30_transaction_commit_i,
    input  wire [3:0]           q30_transaction_slot_i,
    input  wire [31:0]          q30_transaction_epoch_i,
    output wire                 q30_transaction_begin_ready_o,
    output wire                 q30_transaction_commit_ready_o,

    input  wire                 q8_transaction_begin_i,
    input  wire                 q8_transaction_commit_i,
    input  wire [3:0]           q8_transaction_slot_i,
    input  wire [31:0]          q8_transaction_epoch_i,
    output wire                 q8_transaction_begin_ready_o,
    output wire                 q8_transaction_commit_ready_o,

    input  wire                 q30_write_valid_i,
    input  wire [3:0]           q30_write_slot_i,
    input  wire [9:0]           q30_write_index_i,
    input  wire signed [63:0]   q30_write_data_i,
    input  wire [31:0]          q30_write_epoch_i,
    output wire                 q30_write_ready_o,

    input  wire                 q30_read_valid_i,
    input  wire [3:0]           q30_read_slot_i,
    input  wire [9:0]           q30_read_index_i,
    input  wire [31:0]          q30_read_epoch_i,
    output reg                  q30_read_rsp_valid_o,
    output reg  signed [63:0]   q30_read_data_o,
    output reg                  q30_read_error_o,

    input  wire                 q8_write_valid_i,
    input  wire [3:0]           q8_write_slot_i,
    input  wire [4:0]           q8_write_block_i,
    input  wire [271:0]         q8_write_data_i,
    input  wire [31:0]          q8_write_epoch_i,
    output wire                 q8_write_ready_o,

    input  wire                 q8_read_valid_i,
    input  wire [3:0]           q8_read_slot_i,
    input  wire [4:0]           q8_read_block_i,
    input  wire [31:0]          q8_read_epoch_i,
    output reg                  q8_read_rsp_valid_o,
    output reg  [271:0]         q8_read_data_o,
    output reg                  q8_read_error_o
);
    localparam integer Q30_WORDS = Q30_SLOTS * 1024;
    localparam integer Q8_BLOCKS = Q8_SLOTS * 32;

    reg signed [63:0] q30_memory [0:Q30_WORDS-1];
    reg [271:0] q8_memory [0:Q8_BLOCKS-1];
    reg [Q30_SLOTS-1:0] q30_slot_valid;
    reg [Q8_SLOTS-1:0] q8_slot_valid;
    reg [Q30_SLOTS-1:0] q30_transaction_active;
    reg [Q8_SLOTS-1:0] q8_transaction_active;
    reg [31:0] q30_slot_epoch [0:Q30_SLOTS-1];
    reg [31:0] q8_slot_epoch [0:Q8_SLOTS-1];
    reg [10:0] q30_transaction_count [0:Q30_SLOTS-1];
    reg [5:0] q8_transaction_count [0:Q8_SLOTS-1];

    wire q30_write_slot_ok = q30_write_slot_i < Q30_SLOTS;
    wire q30_read_slot_ok = q30_read_slot_i < Q30_SLOTS;
    wire q8_write_slot_ok = q8_write_slot_i < Q8_SLOTS;
    wire q8_read_slot_ok = q8_read_slot_i < Q8_SLOTS;
    wire q30_transaction_slot_ok = q30_transaction_slot_i < Q30_SLOTS;
    wire q8_transaction_slot_ok = q8_transaction_slot_i < Q8_SLOTS;
    wire q30_write_epoch_ok = q30_write_epoch_i == session_epoch_o;
    wire q8_write_epoch_ok = q8_write_epoch_i == session_epoch_o;
    wire q30_transaction_epoch_ok =
        q30_transaction_epoch_i == session_epoch_o;
    wire q8_transaction_epoch_ok =
        q8_transaction_epoch_i == session_epoch_o;

    assign q30_write_ready_o = !session_begin_i
        && session_epoch_o != 32'd0
        && q30_write_slot_ok && q30_write_epoch_ok
        && q30_transaction_active[q30_write_slot_i]
        && q30_write_index_i
            == q30_transaction_count[q30_write_slot_i][9:0]
        && q30_transaction_count[q30_write_slot_i] < 11'd1024;
    assign q8_write_ready_o = !session_begin_i
        && session_epoch_o != 32'd0
        && q8_write_slot_ok && q8_write_epoch_ok
        && q8_transaction_active[q8_write_slot_i]
        && q8_write_block_i == q8_transaction_count[q8_write_slot_i][4:0]
        && q8_transaction_count[q8_write_slot_i] < 6'd32;
    assign q30_transaction_begin_ready_o = !session_begin_i
        && session_epoch_o != 32'd0 && q30_transaction_slot_ok
        && q30_transaction_epoch_ok;
    assign q8_transaction_begin_ready_o = !session_begin_i
        && session_epoch_o != 32'd0 && q8_transaction_slot_ok
        && q8_transaction_epoch_ok;
    assign q30_transaction_commit_ready_o =
        q30_transaction_begin_ready_o
        && q30_transaction_active[q30_transaction_slot_i]
        && q30_transaction_count[q30_transaction_slot_i] == 11'd1024;
    assign q8_transaction_commit_ready_o =
        q8_transaction_begin_ready_o
        && q8_transaction_active[q8_transaction_slot_i]
        && q8_transaction_count[q8_transaction_slot_i] == 6'd32;

    integer metadata_index;
    integer q30_write_address;
    integer q30_read_address;
    integer q8_write_address;
    integer q8_read_address;

    always @* begin
        q30_write_address = q30_write_slot_i * 1024 + q30_write_index_i;
        q30_read_address = q30_read_slot_i * 1024 + q30_read_index_i;
        q8_write_address = q8_write_slot_i * 32 + q8_write_block_i;
        q8_read_address = q8_read_slot_i * 32 + q8_read_block_i;
    end

    always @(posedge clk) begin
        if (!reset_n) begin
            session_epoch_o <= 32'd0;
            session_begin_done_o <= 1'b0;
            session_begin_error_o <= 1'b0;
            q30_slot_valid <= {Q30_SLOTS{1'b0}};
            q8_slot_valid <= {Q8_SLOTS{1'b0}};
            q30_transaction_active <= {Q30_SLOTS{1'b0}};
            q8_transaction_active <= {Q8_SLOTS{1'b0}};
            q30_read_rsp_valid_o <= 1'b0;
            q30_read_data_o <= 64'sd0;
            q30_read_error_o <= 1'b0;
            q8_read_rsp_valid_o <= 1'b0;
            q8_read_data_o <= 272'd0;
            q8_read_error_o <= 1'b0;
            for (metadata_index = 0; metadata_index < Q30_SLOTS;
                 metadata_index = metadata_index + 1)
            begin
                q30_slot_epoch[metadata_index] <= 32'd0;
                q30_transaction_count[metadata_index] <= 11'd0;
            end
            for (metadata_index = 0; metadata_index < Q8_SLOTS;
                 metadata_index = metadata_index + 1)
            begin
                q8_slot_epoch[metadata_index] <= 32'd0;
                q8_transaction_count[metadata_index] <= 6'd0;
            end
        end else begin
            session_begin_done_o <= 1'b0;
            session_begin_error_o <= 1'b0;
            q30_read_rsp_valid_o <= 1'b0;
            q30_read_error_o <= 1'b0;
            q8_read_rsp_valid_o <= 1'b0;
            q8_read_error_o <= 1'b0;

            if (session_begin_i) begin
                session_begin_done_o <= 1'b1;
                // Epoch zero is permanently invalid. Reinstalling the current
                // epoch would make stale handles indistinguishable from fresh
                // ones, so the host-minted epoch must also change.
                if (session_begin_epoch_i == 32'd0
                        || session_begin_epoch_i == session_epoch_o) begin
                    session_begin_error_o <= 1'b1;
                end else begin
                    session_epoch_o <= session_begin_epoch_i;
                    q30_slot_valid <= {Q30_SLOTS{1'b0}};
                    q8_slot_valid <= {Q8_SLOTS{1'b0}};
                    q30_transaction_active <= {Q30_SLOTS{1'b0}};
                    q8_transaction_active <= {Q8_SLOTS{1'b0}};
                end
            end else begin
                if (q30_transaction_begin_i
                        && q30_transaction_begin_ready_o) begin
                    q30_slot_valid[q30_transaction_slot_i] <= 1'b0;
                    q30_transaction_active[q30_transaction_slot_i] <= 1'b1;
                    q30_transaction_count[q30_transaction_slot_i] <= 11'd0;
                end
                if (q8_transaction_begin_i
                        && q8_transaction_begin_ready_o) begin
                    q8_slot_valid[q8_transaction_slot_i] <= 1'b0;
                    q8_transaction_active[q8_transaction_slot_i] <= 1'b1;
                    q8_transaction_count[q8_transaction_slot_i] <= 6'd0;
                end
                if (q30_write_valid_i && q30_write_ready_o) begin
                    q30_memory[q30_write_address] <= q30_write_data_i;
                    q30_transaction_count[q30_write_slot_i]
                        <= q30_transaction_count[q30_write_slot_i] + 11'd1;
                end
                if (q8_write_valid_i && q8_write_ready_o) begin
                    q8_memory[q8_write_address] <= q8_write_data_i;
                    q8_transaction_count[q8_write_slot_i]
                        <= q8_transaction_count[q8_write_slot_i] + 6'd1;
                end
                if (q30_transaction_commit_i
                        && q30_transaction_commit_ready_o) begin
                    q30_slot_valid[q30_transaction_slot_i] <= 1'b1;
                    q30_slot_epoch[q30_transaction_slot_i] <= session_epoch_o;
                    q30_transaction_active[q30_transaction_slot_i] <= 1'b0;
                end
                if (q8_transaction_commit_i
                        && q8_transaction_commit_ready_o) begin
                    q8_slot_valid[q8_transaction_slot_i] <= 1'b1;
                    q8_slot_epoch[q8_transaction_slot_i] <= session_epoch_o;
                    q8_transaction_active[q8_transaction_slot_i] <= 1'b0;
                end

                if (q30_read_valid_i) begin
                    if (q30_read_slot_ok
                        && q30_read_epoch_i == session_epoch_o
                        && q30_slot_valid[q30_read_slot_i]
                        && q30_slot_epoch[q30_read_slot_i] == session_epoch_o) begin
                        q30_read_data_o <= q30_memory[q30_read_address];
                        q30_read_rsp_valid_o <= 1'b1;
                    end else begin
                        q30_read_error_o <= 1'b1;
                    end
                end
                if (q8_read_valid_i) begin
                    if (q8_read_slot_ok
                        && q8_read_epoch_i == session_epoch_o
                        && q8_slot_valid[q8_read_slot_i]
                        && q8_slot_epoch[q8_read_slot_i] == session_epoch_o) begin
                        q8_read_data_o <= q8_memory[q8_read_address];
                        q8_read_rsp_valid_o <= 1'b1;
                    end else begin
                        q8_read_error_o <= 1'b1;
                    end
                end
            end
        end
    end
endmodule
