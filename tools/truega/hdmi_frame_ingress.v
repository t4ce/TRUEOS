// Run on the receiver's pixel clock. Send the ordered token stream through
// one async FIFO into rgba8_frame_store. Arm only when DDR has a free slot.
module hdmi_frame_ingress #(
    parameter WIDTH = 1920,
    parameter HEIGHT = 1080
) (
    input wire pixel_clk, reset,
    input wire shot_valid,
    output wire shot_ready,
    input wire vsync, de,
    input wire [7:0] red, green, blue,
    output wire token_valid,
    input wire token_ready,
    output wire [1:0] token_kind,
    output wire [255:0] token_data
);
    localparam IDLE = 0, BEGIN_FRAME = 1, CAPTURE = 2, FINISH = 3;
    reg [1:0] state;
    reg abort_frame;
    wire pixel_valid, pixel_ready, frame_complete, frame_dropped;
    wire [31:0] rgba;
    wire burst_valid, burst_ready;
    wire draining = (state == CAPTURE || state == FINISH) && burst_valid;
    assign shot_ready = !reset && state == IDLE;
    assign token_valid = !reset && (state == BEGIN_FRAME || draining || state == FINISH);
    assign token_kind = state == BEGIN_FRAME ? 2'd0 :
                        draining ? 2'd1 : abort_frame ? 2'd3 : 2'd2;
    assign burst_ready = draining && token_ready;

    hdmi_rgba8_capture #(.WIDTH(WIDTH), .HEIGHT(HEIGHT)) raster (
        .pixel_clk(pixel_clk), .reset(reset),
        .arm(state == BEGIN_FRAME && token_ready),
        .vsync(vsync), .de(de), .red(red), .green(green), .blue(blue),
        .pixel_ready(pixel_ready), .pixel_valid(pixel_valid), .rgba(rgba),
        .frame_complete(frame_complete), .frame_dropped(frame_dropped)
    );

    rgba8_ddr_burst packer (
        .clk(pixel_clk), .reset(reset || state == IDLE || state == BEGIN_FRAME),
        .pixel_valid(pixel_valid && state == CAPTURE), .pixel_ready(pixel_ready),
        .pixel_data(rgba), .burst_valid(burst_valid), .burst_ready(burst_ready),
        .burst_data(token_data)
    );

    always @(posedge pixel_clk) begin
        if (reset) begin
            state <= IDLE;
            abort_frame <= 0;
        end else case (state)
            IDLE: if (shot_valid) state <= BEGIN_FRAME;
            BEGIN_FRAME: if (token_ready) state <= CAPTURE;
            CAPTURE: if (frame_complete || frame_dropped) begin
                abort_frame <= frame_dropped;
                state <= FINISH;
            end
            FINISH: if (!burst_valid && token_ready) state <= IDLE;
            default: state <= IDLE;
        endcase
    end
endmodule
