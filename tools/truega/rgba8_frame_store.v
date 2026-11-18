// Owns a single frozen RGBA8 frame in a fixed DDR region. Ingress tokens
// must arrive in order from the pixel clock FIFO: begin, data, commit/abort.
// A committed frame stays readable until the host releases it.
module rgba8_frame_store #(
    parameter FRAME_BURSTS = 259200,
    parameter [28:0] BASE_ADDRESS = 0
) (
    input wire clk, reset, calibrated,
    input wire capture_valid,
    output wire capture_ready,
    input wire [1:0] capture_kind,
    input wire [255:0] capture_data,
    output reg frame_ready,
    input wire release_frame,
    input wire read_valid,
    output wire read_ready,
    input wire [18:0] read_index,
    output reg reply_valid,
    input wire reply_ready,
    output reg [255:0] reply_data,
    input wire cmd_ready, wr_data_rdy,
    output wire cmd_en,
    output wire [2:0] cmd,
    output wire [28:0] addr,
    output wire wr_data_en,
    output wire wr_data_end,
    output wire [255:0] wr_data,
    input wire rd_data_valid,
    input wire [255:0] rd_data
);
    localparam BEGIN_FRAME = 0, FRAME_DATA = 1, COMMIT_FRAME = 2, ABORT_FRAME = 3;
    reg collecting, overflowed, read_pending, release_pending;
    reg [18:0] stored;
    wire available = calibrated && !reset;
    wire write_word = capture_kind == FRAME_DATA && collecting &&
                      !overflowed && stored < FRAME_BURSTS;
    wire taking_capture = capture_valid && capture_ready;
    wire issuing_write = taking_capture && write_word;
    wire issuing_read = read_valid && read_ready;

    assign capture_ready = available && !frame_ready &&
                           (!write_word || (cmd_ready && wr_data_rdy));
    assign read_ready = available && frame_ready && !release_frame && !release_pending &&
                        !read_pending && !reply_valid && cmd_ready &&
                        read_index < FRAME_BURSTS;
    assign cmd_en = issuing_write || issuing_read;
    assign cmd = issuing_read ? 3'd1 : 3'd0;
    // BL8 advances by eight DDR words, or one 32-byte application beat.
    assign addr = BASE_ADDRESS + {7'b0, (issuing_read ? read_index : stored), 3'b0};
    assign wr_data_en = issuing_write;
    assign wr_data_end = issuing_write;
    assign wr_data = capture_data;

    always @(posedge clk) begin
        if (!available) begin
            collecting <= 0;
            overflowed <= 0;
            stored <= 0;
            frame_ready <= 0;
            read_pending <= 0;
            release_pending <= 0;
            reply_valid <= 0;
        end else begin
            if (taking_capture) begin
                case (capture_kind)
                    BEGIN_FRAME: begin
                        collecting <= 1;
                        overflowed <= 0;
                        stored <= 0;
                    end
                    FRAME_DATA: if (collecting) begin
                        if (issuing_write)
                            stored <= stored + 1'b1;
                        else
                            overflowed <= 1;
                    end
                    COMMIT_FRAME: begin
                        frame_ready <= collecting && !overflowed && stored == FRAME_BURSTS;
                        collecting <= 0;
                    end
                    ABORT_FRAME: collecting <= 0;
                endcase
            end
            if (issuing_read)
                read_pending <= 1;
            if (reply_ready)
                reply_valid <= 0;
            if (rd_data_valid && read_pending) begin
                read_pending <= 0;
                reply_data <= rd_data;
                reply_valid <= 1;
            end
            if (release_frame && frame_ready)
                release_pending <= 1;
            if ((release_pending || release_frame) && frame_ready &&
                !read_pending && (!reply_valid || reply_ready)) begin
                frame_ready <= 0;
                release_pending <= 0;
            end
        end
    end
endmodule
