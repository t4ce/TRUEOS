// Full HD active raster with blanking. Blue identifies the source frame.
module hdmi_test_raster (
    input wire clk, reset,
    output wire vsync, de,
    output wire [7:0] red, green, blue
);
    reg [11:0] column;
    reg [10:0] row;
    reg [7:0] source_frame;
    assign vsync = row >= 1084 && row < 1089;
    assign de = column < 1920 && row < 1080;
    assign red = column[10:3];
    assign green = row[10:3];
    assign blue = source_frame;
    always @(posedge clk) begin
        if (reset) begin
            column <= 0;
            row <= 0;
            source_frame <= 0;
        end else if (column == 2199) begin
            column <= 0;
            if (row == 1124) begin
                row <= 0;
                source_frame <= source_frame + 1'b1;
            end else
                row <= row + 1'b1;
        end else
            column <= column + 1'b1;
    end
endmodule
