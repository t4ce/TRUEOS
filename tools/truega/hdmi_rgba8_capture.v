// Capture one complete active raster from the HDMI receiver pixel clock.
// The downstream frame store must discard all writes if frame_dropped pulses.
module hdmi_rgba8_capture #(
    parameter WIDTH = 1920,
    parameter HEIGHT = 1080
) (
    input wire pixel_clk, reset,
    input wire arm,
    input wire vsync, de,
    input wire [7:0] red, green, blue,
    input wire pixel_ready,
    output wire pixel_valid,
    output wire [31:0] rgba,
    output reg frame_complete, frame_dropped
);
    reg armed, capturing, invalid;
    reg vsync_previous, de_previous;
    reg [$clog2(WIDTH + 1)-1:0] column;
    reg [$clog2(HEIGHT + 1)-1:0] row;
    wire frame_edge = vsync && !vsync_previous;
    assign pixel_valid = capturing && !invalid && de && !vsync &&
                         column < WIDTH && row < HEIGHT;
    // Little-endian readout has R, G, B, A in ascending byte addresses.
    assign rgba = {8'hff, blue, green, red};

    always @(posedge pixel_clk) begin
        if (reset) begin
            armed <= 0;
            capturing <= 0;
            invalid <= 0;
            vsync_previous <= 0;
            de_previous <= 0;
            frame_complete <= 0;
            frame_dropped <= 0;
            column <= 0;
            row <= 0;
        end else begin
            vsync_previous <= vsync;
            de_previous <= de;
            frame_complete <= 0;
            frame_dropped <= 0;
            if (arm)
                armed <= 1;

            if (frame_edge) begin
                if (capturing) begin
                    frame_complete <= !invalid && row == HEIGHT && column == 0;
                    frame_dropped <= invalid || row != HEIGHT || column != 0;
                end
                capturing <= armed || arm;
                armed <= 0;
                invalid <= 0;
                column <= 0;
                row <= 0;
            end else if (capturing) begin
                if (de) begin
                    if (!pixel_ready || vsync || column == WIDTH || row == HEIGHT)
                        invalid <= 1;
                    else
                        column <= column + 1'b1;
                end else if (de_previous) begin
                    if (column != WIDTH || row == HEIGHT)
                        invalid <= 1;
                    else
                        row <= row + 1'b1;
                    column <= 0;
                end
            end
        end
    end
endmodule
