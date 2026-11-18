// Fill the camera frame slot with the same pattern as usb_rgba8_source.
// Eight adjacent pixels have identical colors when x is a multiple of eight.
module rgba8_ddr_test_pattern (
    input wire clk, reset,
    output wire valid,
    input wire ready,
    output reg [1:0] kind,
    output wire [255:0] data
);
    localparam BEGIN_FRAME = 0, FRAME_DATA = 1, COMMIT_FRAME = 2, DONE = 3;
    reg [10:0] x, y;
    wire [7:0] blue = (x[6] ^ y[6]) ? 8'd224 : 8'd32;
    assign data = {8{8'hff, blue, y[10:3], x[10:3]}};
    assign valid = !reset && kind != DONE;
    always @(posedge clk) begin
        if (reset) begin
            kind <= BEGIN_FRAME;
            x <= 0;
            y <= 0;
        end else if (valid && ready) begin
            case (kind)
                BEGIN_FRAME: kind <= FRAME_DATA;
                FRAME_DATA: if (x == 1912) begin
                    x <= 0;
                    if (y == 1079)
                        kind <= COMMIT_FRAME;
                    else
                        y <= y + 1'b1;
                end else begin
                    x <= x + 4'd8;
                end
                COMMIT_FRAME: kind <= DONE;
                default: ;
            endcase
        end
    end
endmodule
