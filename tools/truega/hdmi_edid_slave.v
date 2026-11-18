// DDC EDID reader at I2C address 0x50. The board supplies pull-ups.
// Clock this block from the board clock, independent of HDMI link lock.
module hdmi_edid_slave #(
    parameter EDID_HEX = "z7-edid.hex"
) (
    input wire clk,
    input wire reset,
    input wire scl,
    inout wire sda,
    output reg address_seen,
    output reg scl_activity, sda_activity,
    output wire scl_level, sda_level
);
    reg [7:0] rom [0:255];
    initial $readmemh(EDID_HEX, rom);

    reg [2:0] scl_sync, sda_sync;
    wire scl_rise = scl_sync[2:1] == 2'b01;
    wire scl_fall = scl_sync[2:1] == 2'b10;
    wire start = scl_sync[2:1] == 2'b11 && sda_sync[2:1] == 2'b10;
    wire stop = scl_sync[2:1] == 2'b11 && sda_sync[2:1] == 2'b01;
    assign scl_level = scl_sync[1];
    assign sda_level = sda_sync[1];

    localparam IDLE = 0, ADDRESS = 1, OFFSET = 2, ACK = 3,
               SEND = 4, MASTER_ACK = 5;
    reg [2:0] state, after_ack, bit_index;
    reg [7:0] pointer, received;
    reg [7:0] outgoing;
    reg ack_asserted, master_acknowledged, sda_low;
    wire [7:0] input_byte = {received[6:0], sda_sync[1]};
    assign sda = sda_low ? 1'b0 : 1'bz;

    always @(posedge clk) begin
        if (reset) begin
            scl_sync <= 3'b111;
            sda_sync <= 3'b111;
            state <= IDLE;
            pointer <= 0;
            sda_low <= 0;
            address_seen <= 0;
            scl_activity <= 0;
            sda_activity <= 0;
        end else begin
            scl_sync <= {scl_sync[1:0], scl};
            sda_sync <= {sda_sync[1:0], sda};
            scl_activity <= scl_activity || scl_rise || scl_fall;
            sda_activity <= sda_activity || (sda_sync[2] != sda_sync[1]);

            if (start || stop) begin
                state <= start ? ADDRESS : IDLE;
                bit_index <= 7;
                sda_low <= 0;
            end else case (state)
                ADDRESS, OFFSET: if (scl_rise) begin
                    received <= input_byte;
                    bit_index <= bit_index - 1'b1;
                    if (bit_index == 0) begin
                        ack_asserted <= 0;
                        if (state == OFFSET) begin
                            pointer <= input_byte;
                            after_ack <= OFFSET;
                            state <= ACK;
                        end else if (input_byte[7:1] == 7'h50) begin
                            address_seen <= 1;
                            after_ack <= input_byte[0] ? SEND : OFFSET;
                            state <= ACK;
                        end else begin
                            state <= IDLE;
                        end
                    end
                end
                ACK: if (scl_fall) begin
                    if (ack_asserted) begin
                        state <= after_ack;
                        bit_index <= 7;
                        outgoing <= rom[pointer];
                        sda_low <= after_ack == SEND && !rom[pointer][7];
                    end else begin
                        sda_low <= 1;
                        ack_asserted <= 1;
                    end
                end
                SEND: if (scl_fall) begin
                    bit_index <= bit_index - 1'b1;
                    if (bit_index == 0) begin
                        state <= MASTER_ACK;
                        sda_low <= 0;
                        pointer <= pointer + 1'b1;
                    end else begin
                        sda_low <= !outgoing[bit_index - 1'b1];
                    end
                end
                MASTER_ACK: begin
                    if (scl_rise)
                        master_acknowledged <= !sda_sync[1];
                    if (scl_fall) begin
                        state <= master_acknowledged ? SEND : IDLE;
                        bit_index <= 7;
                        outgoing <= rom[pointer];
                        sda_low <= master_acknowledged && !rom[pointer][7];
                    end
                end
                default: state <= IDLE;
            endcase
        end
    end
endmodule
