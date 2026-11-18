// One indexed 512-byte USB reply per 512-byte OUT request. The request
// carries little-endian x/y followed by padding. The reply echoes x/y and
// returns 127 RGBA pixels, wrapping at the frame edge.
module usb_rgba8_source #(
    parameter CAMERA_READOUT = 0
) (
    input wire clk, reset,
    input wire request_valid,
    input wire [7:0] request_data,
    input wire [23:0] camera_status,
    input wire [14:0] frame_sequence,
    input wire [7:0] capture_trace,
    output wire camera_request,
    output wire hpd_request,
    output wire release_request,
    output wire [14:0] camera_packet,
    output wire valid,
    output reg [7:0] data
);
    reg [8:0] request_byte, reply_byte;
    reg [10:0] x, y;
    reg [10:0] start_x, start_y;
    reg [6:0] camera_packet_high;
    reg sending;
    reg [23:0] sampled_status;
    reg [14:0] sampled_sequence;
    reg [7:0] sampled_trace;
    assign valid = sending;
    assign camera_request = CAMERA_READOUT && !reset && request_valid &&
                            request_byte == 511 && start_x == 2046;
    assign hpd_request = !reset && request_valid && request_byte == 511 &&
                         start_x == 2045 && start_y == 0;
    assign release_request = CAMERA_READOUT && !reset && request_valid &&
                             request_byte == 511 && start_x == 2044;
    assign camera_packet = {camera_packet_high, start_y[7:0]};

    always @* begin
        case (reply_byte)
            0: data = start_x[7:0];
            1: data = {5'b0, start_x[10:8]};
            2: data = start_y[7:0];
            3: data = start_x == 2044 ? {1'b0, camera_packet_high} : {5'b0, start_y[10:8]};
            default: if (start_x == 2047 || start_x == 2044 || (start_x == 2045 && start_y == 0))
                case (reply_byte)
                    4: data = sampled_status[7:0];
                    5: data = sampled_status[15:8];
                    6: data = sampled_status[23:16];
                    7: data = sampled_sequence[7:0];
                    8: data = {1'b0, sampled_sequence[14:8]};
                    9: data = sampled_trace;
                    default: data = 0;
                endcase
            else case (reply_byte[1:0])
                0: data = x[10:3];
                1: data = y[10:3];
                2: data = (x[6] ^ y[6]) ? 8'd224 : 8'd32;
                3: data = 8'hff;
            endcase
        endcase
    end

    always @(posedge clk) begin
        if (reset) begin
            request_byte <= 0;
            reply_byte <= 0;
            sending <= 0;
        end else begin
            if (request_valid) begin
                request_byte <= request_byte + 1'b1;
                case (request_byte)
                    0: start_x[7:0] <= request_data;
                    1: start_x[10:8] <= request_data[2:0];
                    2: start_y[7:0] <= request_data;
                    3: begin
                        start_y[10:8] <= request_data[2:0];
                        camera_packet_high <= request_data[6:0];
                    end
                    511: begin
                        x <= start_x;
                        y <= start_y;
                        reply_byte <= 0;
                        sending <= !camera_request;
                        sampled_status <= camera_status;
                        sampled_sequence <= frame_sequence;
                        sampled_trace <= capture_trace;
                    end
                    default: ;
                endcase
            end
            if (sending) begin
                reply_byte <= reply_byte + 1'b1;
                if (reply_byte == 511)
                    sending <= 0;
                if (reply_byte > 3 && reply_byte[1:0] == 3) begin
                    if (x == 1919) begin
                        x <= 0;
                        y <= y == 1079 ? 11'd0 : y + 1'b1;
                    end else begin
                        x <= x + 1'b1;
                    end
                end
            end
        end
    end
endmodule
