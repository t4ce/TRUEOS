// Write and read all 8,294,400 bytes through the physical DDR controller.
module rgba8_ddr_selftest (
    input wire clk, reset, calibrated,
    input wire cmd_ready, wr_data_rdy,
    output wire cmd_en,
    output wire [2:0] cmd,
    output wire [28:0] addr,
    output wire wr_data_en, wr_data_end,
    output wire [255:0] wr_data,
    input wire rd_data_valid,
    input wire [255:0] rd_data,
    output wire done,
    output reg failed
);
    localparam FRAME_BURSTS = 259200;
    localparam HEADER = 0, WRITE = 1, COMMIT = 2,
               READ = 3, WAIT_REPLY = 4, DONE = 5;
    reg [2:0] state;
    reg [18:0] index;
    wire capture_valid = state == HEADER || state == WRITE || state == COMMIT;
    wire [1:0] capture_kind = state == HEADER ? 2'd0 :
                              state == WRITE ? 2'd1 : 2'd2;
    wire [31:0] word = {13'd0, index} ^ 32'h9abcf135;
    wire [255:0] expected = {word ^ 32'h1423657a, word ^ 32'h9bc27df1,
                             word ^ 32'h46ebf109, word ^ 32'hdeadbeef,
                             ~word, word ^ 32'h1f568234, word ^ 32'hacabce54, word};
    wire capture_ready, frame_ready, read_ready, reply_valid;
    wire [255:0] reply_data;
    assign done = state == DONE;

    rgba8_frame_store store (
        .clk(clk), .reset(reset), .calibrated(calibrated),
        .capture_valid(capture_valid), .capture_ready(capture_ready),
        .capture_kind(capture_kind), .capture_data(expected),
        .frame_ready(frame_ready), .release_frame(1'b0),
        .read_valid(state == READ), .read_ready(read_ready), .read_index(index),
        .reply_valid(reply_valid), .reply_ready(state == WAIT_REPLY),
        .reply_data(reply_data), .cmd_ready(cmd_ready), .wr_data_rdy(wr_data_rdy),
        .cmd_en(cmd_en), .cmd(cmd), .addr(addr), .wr_data_en(wr_data_en),
        .wr_data_end(wr_data_end), .wr_data(wr_data),
        .rd_data_valid(rd_data_valid), .rd_data(rd_data)
    );

    always @(posedge clk) begin
        if (reset || !calibrated) begin
            state <= HEADER;
            index <= 0;
            failed <= 0;
        end else case (state)
            HEADER: if (capture_ready) state <= WRITE;
            WRITE: if (capture_ready) begin
                index <= index + 1'b1;
                if (index == FRAME_BURSTS - 1)
                    state <= COMMIT;
            end
            COMMIT: if (capture_ready) begin
                index <= 0;
                state <= READ;
            end
            READ: if (frame_ready && read_ready) state <= WAIT_REPLY;
            WAIT_REPLY: if (reply_valid) begin
                if (reply_data != expected) begin
                    failed <= 1;
                    state <= DONE;
                end else if (index == FRAME_BURSTS - 1) begin
                    state <= DONE;
                end else begin
                    index <= index + 1'b1;
                    state <= READ;
                end
            end
            default: ;
        endcase
    end
endmodule
