// Fixed LFM2.5 KV-cache append/read address contract.
//
// Only layers 2,5,8,10,12,14 are attention layers in the sealed 16-layer
// model.  Each has 8 KV heads of 64 Q30 elements.  K and V are adjacent
// 64-bit words (kind 0/1) at the innermost dimension.  The resulting byte
// address is suitable for an FPGA-local SRAM/DDR controller; this slot is not
// DMA, a TLB, or a programmable command interpreter.
//
// GQA is fixed at 16 query heads / 8 KV heads, hence kv_head = q_head >> 1.
// An append position becomes readable when V[head=7][element=63] commits.
module truega_lfm25_kv_address_slot #(
    parameter integer INITIAL_CONTEXT = 16384
) (
    input  wire          clk,
    input  wire          reset_n,
    input  wire          start_i,
    input  wire          append_i,
    input  wire          reset_layer_i,
    input  wire [3:0]    layer_i,
    input  wire [16:0]   position_i,
    input  wire [3:0]    query_head_i,
    input  wire [2:0]    kv_head_i,
    input  wire [5:0]    element_i,
    input  wire          value_i,
    output reg           busy_o,
    output reg           done_o,
    output reg           error_o,
    output reg [2:0]     mapped_kv_head_o,
    output reg [16:0]    valid_positions_o,
    output reg [29:0]    word_address_o,
    output reg [32:0]    byte_address_o
);
    reg append;
    reg reset_layer;
    reg [3:0] layer;
    reg [16:0] position;
    reg [3:0] query_head;
    reg [2:0] kv_head;
    reg [5:0] element;
    reg value_kind;
    reg [16:0] valid_positions [0:5];
    integer index;

    reg layer_valid;
    reg [2:0] attention_ordinal;
    reg [63:0] word_address_wide;
    always @* begin
        layer_valid = 1'b1;
        attention_ordinal = 3'd0;
        case (layer)
            4'd2:  attention_ordinal = 3'd0;
            4'd5:  attention_ordinal = 3'd1;
            4'd8:  attention_ordinal = 3'd2;
            4'd10: attention_ordinal = 3'd3;
            4'd12: attention_ordinal = 3'd4;
            4'd14: attention_ordinal = 3'd5;
            default: layer_valid = 1'b0;
        endcase
        word_address_wide = (((((attention_ordinal * INITIAL_CONTEXT)
            + position) * 8) + kv_head) * 64 + element) * 2 + value_kind;
    end

    always @(posedge clk) begin
        if (!reset_n) begin
            append <= 1'b0;
            reset_layer <= 1'b0;
            layer <= 4'd0;
            position <= 17'd0;
            query_head <= 4'd0;
            kv_head <= 3'd0;
            element <= 6'd0;
            value_kind <= 1'b0;
            busy_o <= 1'b0;
            done_o <= 1'b0;
            error_o <= 1'b0;
            mapped_kv_head_o <= 3'd0;
            valid_positions_o <= 17'd0;
            word_address_o <= 30'd0;
            byte_address_o <= 33'd0;
            for (index = 0; index < 6; index = index + 1)
                valid_positions[index] <= 17'd0;
        end else begin
            done_o <= 1'b0;
            if (start_i && !busy_o) begin
                append <= append_i;
                reset_layer <= reset_layer_i;
                layer <= layer_i;
                position <= position_i;
                query_head <= query_head_i;
                kv_head <= kv_head_i;
                element <= element_i;
                value_kind <= value_i;
                busy_o <= 1'b1;
                error_o <= 1'b0;
            end else if (busy_o) begin
                busy_o <= 1'b0;
                done_o <= 1'b1;
                mapped_kv_head_o <= query_head[3:1];
                if (!layer_valid || query_head > 4'd15
                    || position >= INITIAL_CONTEXT) begin
                    error_o <= 1'b1;
                    valid_positions_o <= 17'd0;
                    word_address_o <= 30'd0;
                    byte_address_o <= 33'd0;
                end else if (reset_layer) begin
                    valid_positions[attention_ordinal] <= 17'd0;
                    valid_positions_o <= 17'd0;
                    word_address_o <= 30'd0;
                    byte_address_o <= 33'd0;
                end else if (append
                    && position != valid_positions[attention_ordinal]) begin
                    error_o <= 1'b1;
                    valid_positions_o <= valid_positions[attention_ordinal];
                    word_address_o <= 30'd0;
                    byte_address_o <= 33'd0;
                end else if (!append
                    && position >= valid_positions[attention_ordinal]) begin
                    error_o <= 1'b1;
                    valid_positions_o <= valid_positions[attention_ordinal];
                    word_address_o <= 30'd0;
                    byte_address_o <= 33'd0;
                end else begin
                    error_o <= 1'b0;
                    word_address_o <= word_address_wide[29:0];
                    byte_address_o <= {word_address_wide[29:0], 3'b000};
                    if (append && value_kind && kv_head == 3'd7
                        && element == 6'd63) begin
                        valid_positions[attention_ordinal]
                            <= valid_positions[attention_ordinal] + 17'd1;
                        valid_positions_o
                            <= valid_positions[attention_ordinal] + 17'd1;
                    end else begin
                        valid_positions_o <= valid_positions[attention_ordinal];
                    end
                end
            end
        end
    end
endmodule
