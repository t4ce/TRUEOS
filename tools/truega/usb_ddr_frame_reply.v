// One indexed USB reply from the frozen DDR frame. A packet carries fifteen
// 32-byte memory words and a CRC32 over the 480 pixel bytes.
module usb_ddr_frame_reply (
    input wire clk, reset, frozen,
    input wire request,
    input wire [14:0] packet,
    output wire read_valid,
    input wire read_ready,
    output wire [18:0] read_index,
    input wire reply_valid,
    output wire reply_ready,
    input wire [18:0] reply_index,
    input wire [255:0] reply_data,
    output wire valid,
    output reg [7:0] data
);
    localparam IDLE = 0, HEADER = 1, READ = 2, WAIT_DATA = 3,
               PIXELS = 4, CRC = 5, PADDING = 6;
    reg [2:0] state;
    reg [14:0] selected;
    reg [8:0] position;
    reg [18:0] burst_index;
    reg [3:0] burst_count;
    reg [255:0] bytes_left;
    reg [31:0] crc;
    reg available;

    function [31:0] crc_next(input [31:0] previous, input [7:0] octet);
        reg [31:0] value;
        integer i;
        begin
            value = previous ^ {24'b0, octet};
            for (i = 0; i < 8; i = i + 1)
                value = (value >> 1) ^ (value[0] ? 32'hedb88320 : 32'b0);
            crc_next = value;
        end
    endfunction

    assign read_valid = !reset && state == READ;
    assign read_index = burst_index;
    assign reply_ready = state == IDLE || state == WAIT_DATA || reset;
    assign valid = !reset && (state == HEADER || state == PIXELS ||
                              state == CRC || state == PADDING);
    always @* begin
        data = 0;
        case (state)
            HEADER: case (position)
                0: data = 8'hfe;
                1: data = 8'h07;
                2: data = selected[7:0];
                3: data = {1'b0, selected[14:8]};
                4: data = {7'b0, available};
                default: ;
            endcase
            PIXELS: data = bytes_left[7:0];
            CRC: data = ~crc[7:0];
            default: ;
        endcase
    end

    always @(posedge clk) begin
        if (reset) begin
            state <= IDLE;
        end else begin
            if (valid)
                position <= position + 1'b1;
            case (state)
                IDLE: if (request) begin
                    selected <= packet;
                    available <= frozen && packet < 17280;
                    position <= 0;
                    burst_count <= 0;
                    burst_index <= ({4'b0, packet} << 4) - {4'b0, packet};
                    crc <= 32'hffffffff;
                    state <= HEADER;
                end
                HEADER: if (position == 4)
                    state <= available ? READ : PADDING;
                READ: if (read_ready)
                    state <= WAIT_DATA;
                WAIT_DATA: if (reply_valid && reply_index == burst_index) begin
                    bytes_left <= reply_data;
                    state <= PIXELS;
                end
                PIXELS: begin
                    bytes_left <= bytes_left >> 8;
                    crc <= crc_next(crc, data);
                    if (position[4:0] == 5'd4) begin
                        if (burst_count == 14)
                            state <= CRC;
                        else begin
                            burst_count <= burst_count + 1'b1;
                            burst_index <= burst_index + 1'b1;
                            state <= READ;
                        end
                    end
                end
                CRC: begin
                    crc <= crc >> 8;
                    if (position == 488)
                        state <= PADDING;
                end
                PADDING: if (position == 511) state <= IDLE;
                default: state <= IDLE;
            endcase
        end
    end
endmodule
