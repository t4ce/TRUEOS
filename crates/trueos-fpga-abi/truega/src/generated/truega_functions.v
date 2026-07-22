

module truega_scalar_functions(function_id,arg0,arg1,led_state,next_led,result,required_input_bytes,output_bytes,valid);
    
    // Module arguments
    input wire  [15:0] function_id;
    input wire  [31:0] arg0;
    input wire  [31:0] arg1;
    input wire  [4:0] led_state;
    output reg  [4:0] next_led;
    output reg  [31:0] result;
    output reg  [15:0] required_input_bytes;
    output reg  [15:0] output_bytes;
    output reg  valid;
    
    // Stub signals
    reg  [4:0] heartbeat$led_state;
    wire  [4:0] heartbeat$next_led;
    wire  [31:0] heartbeat$result;
    reg  [31:0] add_u32$a;
    reg  [31:0] add_u32$b;
    wire  [31:0] add_u32$result;
    
    // Sub module instances
    truega_scalar_functions$heartbeat heartbeat(
        .led_state(heartbeat$led_state),
        .next_led(heartbeat$next_led),
        .result(heartbeat$result)
    );
    truega_scalar_functions$add_u32 add_u32(
        .a(add_u32$a),
        .b(add_u32$b),
        .result(add_u32$result)
    );
    
    // Update code
    always @(*) begin
        heartbeat$led_state = led_state;
        add_u32$a = arg0;
        add_u32$b = arg1;
        result = 32'h0;
        next_led = led_state;
        required_input_bytes = 32'h0;
        output_bytes = 32'h0;
        valid = 1'b0;
        if (function_id == 32'h0) begin
            result = heartbeat$result;
            next_led = heartbeat$next_led;
            output_bytes = 32'h4;
            valid = 1'b1;
        end
        else if (function_id == 32'h1) begin
            result = add_u32$result;
            required_input_bytes = 32'h8;
            output_bytes = 32'h4;
            valid = 1'b1;
        end
    end
    
endmodule // top


module truega_scalar_functions$add_u32(a,b,result);
    
    // Module arguments
    input wire  [31:0] a;
    input wire  [31:0] b;
    output reg  [31:0] result;
    
    // Update code
    always @(*) begin
        result = a + b;
    end
    
endmodule // truega_scalar_functions$add_u32


module truega_scalar_functions$heartbeat(led_state,next_led,result);
    
    // Module arguments
    input wire  [4:0] led_state;
    output reg  [4:0] next_led;
    output reg  [31:0] result;
    
    // Update code
    always @(*) begin
        if (led_state == 32'h1) begin
            next_led = 32'h2;
        end
        else if (led_state == 32'h2) begin
            next_led = 32'h4;
        end
        else if (led_state == 32'h4) begin
            next_led = 32'h8;
        end
        else if (led_state == 32'h8) begin
            next_led = 32'h10;
        end
        else begin
            next_led = 32'h1;
        end
        result = 32'h54534154;
    end
    
endmodule // truega_scalar_functions$heartbeat

// Common clocked handoff for all three ahead-of-time function slots.
module truega_functions(
    input  wire         clk,
    input  wire         reset_n,
    input  wire         start,
    input  wire [15:0]  function_id,
    input  wire [767:0] input_data,
    input  wire [4:0]   led_state,
    output reg  [4:0]   next_led,
    output reg  [767:0] output_data,
    output reg  [15:0]  required_input_bytes,
    output reg  [15:0]  output_bytes,
    output reg          valid,
    output reg          busy,
    output reg          done,
    output reg          error
);
    wire [4:0] scalar_next_led;
    wire [31:0] scalar_result;
    wire [15:0] scalar_required_input_bytes;
    wire [15:0] scalar_output_bytes;
    wire scalar_valid;
    reg [15:0] active_function;
    reg q8_start;
    wire q8_busy;
    wire q8_done;
    wire signed [31:0] q8_dot;
    wire signed [63:0] q8_term_q30;
    wire signed [63:0] q8_row_q30;
    wire q8_scale_error;
    reg silu_start;
    wire silu_busy;
    wire silu_done;
    wire silu_error;
    wire signed [63:0] silu_result_q30;
    reg active_silu;

    truega_scalar_functions scalar_functions(
        .function_id(function_id),
        .arg0(input_data[31:0]),
        .arg1(input_data[63:32]),
        .led_state(led_state),
        .next_led(scalar_next_led),
        .result(scalar_result),
        .required_input_bytes(scalar_required_input_bytes),
        .output_bytes(scalar_output_bytes),
        .valid(scalar_valid)
    );

    truega_q8_0_cached_pair_slot #(
        .CACHED_PAIR_ENABLE(1)
    ) q8_row_block_slot(
        .clk(clk),
        .reset_n(reset_n),
        .start_i(q8_start),
        .control_i(input_data[31:0]),
        .activation_block_i(input_data[303:32]),
        .weight_block_i(input_data[575:304]),
        .busy_o(q8_busy),
        .done_o(q8_done),
        .dot_o(q8_dot),
        .term_q30_o(q8_term_q30),
        .row_q30_o(q8_row_q30),
        .error_o(q8_scale_error)
    );

    truega_lfm25_silu_q30_slot #(
        .SILU_ENABLE(1)
    ) silu_slot(
        .clk(clk),
        .reset_n(reset_n),
        .start_i(silu_start),
        .gate_q30_i(input_data[95:32]),
        .up_q30_i(input_data[159:96]),
        .busy_o(silu_busy),
        .done_o(silu_done),
        .error_o(silu_error),
        .result_q30_o(silu_result_q30)
    );

    always @* begin
        required_input_bytes = 16'd0;
        output_bytes = 16'd0;
        valid = 1'b0;
        case (function_id)
            16'd0: begin
                required_input_bytes = 16'd0;
                output_bytes = 16'd4;
                valid = 1'b1;
            end
            16'd1: begin
                required_input_bytes = 16'd8;
                output_bytes = 16'd4;
                valid = 1'b1;
            end
            16'd2: begin
                required_input_bytes = 16'd72;
                output_bytes = 16'd20;
                valid = 1'b1;
            end
            default: begin end
        endcase
    end

    always @(posedge clk) begin
        if (!reset_n) begin
            active_function <= 16'd0;
            q8_start <= 1'b0;
            silu_start <= 1'b0;
            active_silu <= 1'b0;
            next_led <= 5'b00001;
            output_data <= 768'd0;
            busy <= 1'b0;
            done <= 1'b0;
            error <= 1'b0;
        end else begin
            q8_start <= 1'b0;
            silu_start <= 1'b0;
            done <= 1'b0;
            if (start && !busy) begin
                active_function <= function_id;
                output_data <= 768'd0;
                next_led <= led_state;
                error <= 1'b0;
                busy <= 1'b1;
                case (function_id)
                    16'd0, 16'd1: begin
                        output_data[31:0] <= scalar_result;
                        next_led <= scalar_next_led;
                        busy <= 1'b0;
                        done <= 1'b1;
                    end
                    16'd2: begin
                        active_silu <= input_data[3];
                        if (input_data[3])
                            silu_start <= 1'b1;
                        else
                            q8_start <= 1'b1;
                    end
                    default: begin
                        busy <= 1'b0;
                        done <= 1'b1;
                        error <= 1'b1;
                    end
                endcase
            end else if (busy && active_function == 16'd2 && active_silu && silu_done) begin
                output_data <= 768'd0;
                output_data[159:96] <= silu_result_q30;
                busy <= 1'b0;
                done <= 1'b1;
                error <= silu_error;
            end else if (busy && active_function == 16'd2 && !active_silu && q8_done) begin
                output_data <= 768'd0;
                output_data[31:0] <= q8_dot;
                output_data[95:32] <= q8_term_q30;
                output_data[159:96] <= q8_row_q30;
                busy <= 1'b0;
                done <= 1'b1;
                error <= q8_scale_error;
            end
        end
    end
    wire unused_silu_busy = silu_busy;
endmodule

// Exact native Q8_0/FFN compute sources fused into this generated bundle.
// Exact 32-lane signed Q8_0 integer dot product.
//
// The lanes are accumulated serially so synthesis cannot infer signed partial-
// sum RAMs or silently zero-extend a negative tree node.  Each 16-bit product
// is sign-extended by explicit bit replication, then added as raw two's-
// complement bits in a 21-bit accumulator.  Lane selection, multiplication,
// and accumulation occupy separate registered stages.  The enclosing serialized
// block slot keeps both quant inputs stable until valid_o.  One accepted block
// completes after 33 work cycles; valid_i is ignored while a block is active.
module truega_q8_0_dot32 (
    input  wire                 clk,
    input  wire                 reset_n,
    input  wire                 valid_i,
    input  wire [255:0]         activation_quants_i,
    input  wire [255:0]         weight_quants_i,
    output wire                 valid_o,
    output wire signed [20:0]   dot_o
);
    localparam STATE_IDLE = 2'd0;
    localparam STATE_RUN = 2'd1;
    localparam STATE_LAST_PRODUCT = 2'd2;
    localparam STATE_DRAIN = 2'd3;

    reg [5:0] lane_index;
    reg [1:0] state;
    reg valid_reg;
    reg [7:0] activation_lane_reg;
    reg [7:0] weight_lane_reg;
    reg [15:0] product_reg;
    reg [20:0] accumulator;
    reg [20:0] dot_reg;

    wire [7:0] activation_lane_bits;
    wire [7:0] weight_lane_bits;
    wire signed [7:0] activation_lane;
    wire signed [7:0] weight_lane;
    wire signed [15:0] lane_product;
    wire [15:0] current_product_bits;
    wire [20:0] registered_product_extended;
    wire [20:0] accumulator_next;

    assign activation_lane_bits = activation_quants_i[lane_index*8 +: 8];
    assign weight_lane_bits = weight_quants_i[lane_index*8 +: 8];
    assign activation_lane = activation_lane_reg;
    assign weight_lane = weight_lane_reg;
    assign lane_product = activation_lane * weight_lane;
    assign current_product_bits = lane_product;
    assign registered_product_extended = {{5{product_reg[15]}}, product_reg};
    assign accumulator_next = accumulator + registered_product_extended;

    assign valid_o = valid_reg;
    assign dot_o = dot_reg;

    always @(posedge clk) begin
        if (!reset_n) begin
            lane_index <= 6'd0;
            state <= STATE_IDLE;
            valid_reg <= 1'b0;
            activation_lane_reg <= 8'd0;
            weight_lane_reg <= 8'd0;
            product_reg <= 16'd0;
            accumulator <= 21'd0;
            dot_reg <= 21'd0;
        end else begin
            valid_reg <= 1'b0;

            case (state)
                STATE_IDLE: begin
                    if (valid_i) begin
                        activation_lane_reg <= activation_quants_i[7:0];
                        weight_lane_reg <= weight_quants_i[7:0];
                        lane_index <= 6'd1;
                        product_reg <= 16'd0;
                        accumulator <= 21'd0;
                        state <= STATE_RUN;
                    end
                end

                STATE_RUN: begin
                    product_reg <= current_product_bits;
                    activation_lane_reg <= activation_lane_bits;
                    weight_lane_reg <= weight_lane_bits;
                    if (lane_index != 6'd1)
                        accumulator <= accumulator_next;
                    if (lane_index == 6'd31) begin
                        state <= STATE_LAST_PRODUCT;
                    end else begin
                        lane_index <= lane_index + 1'b1;
                    end
                end

                STATE_LAST_PRODUCT: begin
                    product_reg <= current_product_bits;
                    accumulator <= accumulator_next;
                    state <= STATE_DRAIN;
                end

                STATE_DRAIN: begin
                    dot_reg <= accumulator_next;
                    accumulator <= 21'd0;
                    lane_index <= 6'd0;
                    state <= STATE_IDLE;
                    valid_reg <= 1'b1;
                end

                default: begin
                    state <= STATE_IDLE;
                    valid_reg <= 1'b0;
                    product_reg <= 16'd0;
                    accumulator <= 21'd0;
                end
            endcase
        end
    end
endmodule

// Multi-cycle conversion of one exact Q8_0 integer dot product to signed Q30.
//
// Each Q8_0 scale is a non-negative IEEE binary16 value.  Normal and
// subnormal values are decoded without converting through floating point.
// Right shifts use round-to-nearest, ties-to-even.  The iterative shifter keeps
// the combinational path short enough for use beside the 100 MHz PCIe shell.
// start_i is accepted only while idle; later starts are ignored until done_o.
// done_o pulses for one cycle with busy_o low and the result already registered.
module truega_q8_0_scale_q30_seq (
    input  wire                 clk,
    input  wire                 reset_n,
    input  wire                 start_i,
    input  wire signed [20:0]   dot_i,
    input  wire [15:0]          activation_scale_f16_i,
    input  wire [15:0]          weight_scale_f16_i,
    output reg                  busy_o,
    output reg                  done_o,
    output reg  signed [63:0]   term_q30_o,
    output reg                  scale_error_o
);
    localparam [3:0] STATE_IDLE        = 4'd0;
    localparam [3:0] STATE_VALIDATE    = 4'd1;
    localparam [3:0] STATE_MUL_SCALE   = 4'd2;
    localparam [3:0] STATE_MUL_DOT     = 4'd3;
    localparam [3:0] STATE_PREP_SHIFT  = 4'd4;
    localparam [3:0] STATE_SHIFT_LEFT  = 4'd5;
    localparam [3:0] STATE_SHIFT_RIGHT = 4'd6;
    localparam [3:0] STATE_ROUND       = 4'd7;
    localparam [3:0] STATE_COMMIT      = 4'd8;

    wire activation_invalid = activation_scale_f16_i[15]
                            || activation_scale_f16_i[14:10] == 5'h1f;
    wire weight_invalid = weight_scale_f16_i[15]
                        || weight_scale_f16_i[14:10] == 5'h1f;
    wire [10:0] activation_significand_decoded =
        activation_scale_f16_i[14:10] == 5'd0
            ? {1'b0, activation_scale_f16_i[9:0]}
            : {1'b1, activation_scale_f16_i[9:0]};
    wire [10:0] weight_significand_decoded =
        weight_scale_f16_i[14:10] == 5'd0
            ? {1'b0, weight_scale_f16_i[9:0]}
            : {1'b1, weight_scale_f16_i[9:0]};
    wire [5:0] activation_exponent_decoded =
        activation_scale_f16_i[14:10] == 5'd0
            ? 6'd1
            : {1'b0, activation_scale_f16_i[14:10]};
    wire [5:0] weight_exponent_decoded =
        weight_scale_f16_i[14:10] == 5'd0
            ? 6'd1
            : {1'b0, weight_scale_f16_i[14:10]};
    wire signed [7:0] scale_shift_decoded =
        $signed({2'b00, activation_exponent_decoded})
      + $signed({2'b00, weight_exponent_decoded}) - 8'sd20;

    reg [3:0] state;
    reg signed [20:0] dot_reg;
    reg [10:0] activation_significand_reg;
    reg [10:0] weight_significand_reg;
    reg [21:0] significand_product_reg;
    reg signed [42:0] raw_product_reg;
    reg signed [7:0] scale_shift_reg;
    reg invalid_reg;
    reg [5:0] shift_count_reg;
    reg [63:0] magnitude_reg;
    reg negative_reg;
    reg guard_reg;
    reg sticky_reg;

    wire signed [22:0] significand_product_signed =
        $signed({1'b0, significand_product_reg});
    wire signed [42:0] dot_scale_product =
        dot_reg * significand_product_signed;
    wire signed [63:0] raw_extended =
        {{21{raw_product_reg[42]}}, raw_product_reg};
    wire [63:0] raw_magnitude = raw_extended[63]
        ? (~raw_extended + 64'd1)
        : raw_extended;
    wire round_increment = guard_reg && (sticky_reg || magnitude_reg[0]);
    wire [63:0] rounded_magnitude = magnitude_reg
        + (round_increment ? 64'd1 : 64'd0);

    always @(posedge clk) begin
        if (!reset_n) begin
            state <= STATE_IDLE;
            dot_reg <= 21'sd0;
            activation_significand_reg <= 11'd0;
            weight_significand_reg <= 11'd0;
            significand_product_reg <= 22'd0;
            raw_product_reg <= 43'sd0;
            scale_shift_reg <= 8'sd0;
            invalid_reg <= 1'b0;
            shift_count_reg <= 6'd0;
            magnitude_reg <= 64'd0;
            negative_reg <= 1'b0;
            guard_reg <= 1'b0;
            sticky_reg <= 1'b0;
            busy_o <= 1'b0;
            done_o <= 1'b0;
            term_q30_o <= 64'sd0;
            scale_error_o <= 1'b0;
        end else begin
            done_o <= 1'b0;
            case (state)
                STATE_IDLE: begin
                    busy_o <= 1'b0;
                    if (start_i) begin
                        busy_o <= 1'b1;
                        term_q30_o <= 64'sd0;
                        scale_error_o <= 1'b0;
                        guard_reg <= 1'b0;
                        sticky_reg <= 1'b0;
                        dot_reg <= dot_i;
                        activation_significand_reg <= activation_significand_decoded;
                        weight_significand_reg <= weight_significand_decoded;
                        scale_shift_reg <= scale_shift_decoded;
                        invalid_reg <= activation_invalid || weight_invalid;
                        state <= STATE_VALIDATE;
                    end
                end

                STATE_VALIDATE: begin
                    if (invalid_reg) begin
                        scale_error_o <= 1'b1;
                        state <= STATE_COMMIT;
                    end else if (activation_significand_reg == 11'd0
                              || weight_significand_reg == 11'd0
                              || dot_reg == 21'sd0) begin
                        state <= STATE_COMMIT;
                    end else if (scale_shift_reg > 8'sd20) begin
                        scale_error_o <= 1'b1;
                        state <= STATE_COMMIT;
                    end else begin
                        state <= STATE_MUL_SCALE;
                    end
                end

                STATE_MUL_SCALE: begin
                    significand_product_reg <= activation_significand_reg
                                             * weight_significand_reg;
                    state <= STATE_MUL_DOT;
                end

                STATE_MUL_DOT: begin
                    raw_product_reg <= dot_scale_product;
                    state <= STATE_PREP_SHIFT;
                end

                STATE_PREP_SHIFT: begin
                    magnitude_reg <= raw_magnitude;
                    negative_reg <= raw_extended[63];
                    guard_reg <= 1'b0;
                    sticky_reg <= 1'b0;
                    if (scale_shift_reg > 0) begin
                        shift_count_reg <= scale_shift_reg[5:0];
                        state <= STATE_SHIFT_LEFT;
                    end else if (scale_shift_reg < 0) begin
                        shift_count_reg <= 6'd0 - scale_shift_reg[5:0];
                        state <= STATE_SHIFT_RIGHT;
                    end else begin
                        shift_count_reg <= 6'd0;
                        state <= STATE_ROUND;
                    end
                end

                STATE_SHIFT_LEFT: begin
                    magnitude_reg <= magnitude_reg << 1;
                    shift_count_reg <= shift_count_reg - 1'b1;
                    if (shift_count_reg == 6'd1)
                        state <= STATE_ROUND;
                end

                STATE_SHIFT_RIGHT: begin
                    magnitude_reg <= magnitude_reg >> 1;
                    shift_count_reg <= shift_count_reg - 1'b1;
                    if (shift_count_reg == 6'd1) begin
                        guard_reg <= magnitude_reg[0];
                        state <= STATE_ROUND;
                    end else begin
                        sticky_reg <= sticky_reg || magnitude_reg[0];
                    end
                end

                STATE_ROUND: begin
                    term_q30_o <= negative_reg
                        ? -$signed(rounded_magnitude)
                        : $signed(rounded_magnitude);
                    state <= STATE_COMMIT;
                end

                STATE_COMMIT: begin
                    busy_o <= 1'b0;
                    done_o <= 1'b1;
                    state <= STATE_IDLE;
                end

                default: begin
                    state <= STATE_IDLE;
                    busy_o <= 1'b0;
                    done_o <= 1'b0;
                    term_q30_o <= 64'sd0;
                    scale_error_o <= 1'b1;
                end
            endcase
        end
    end
endmodule

// One serialized Q8_0 block operation with reusable start/busy/done signalling.
// Native blocks are unchanged: bits [15:0] hold the little-endian binary16
// scale and bits [16 + lane*8 +: 8] hold signed quant lane `lane`.
// A rising edge accepts start_i only with busy_o low.  Attempts while busy are
// ignored.  Accepted block inputs must remain stable while busy_o is high.
// done_o pulses for one cycle after both registered outputs are valid.
module truega_q8_0_block_slot (
    input  wire                 clk,
    input  wire                 reset_n,
    input  wire                 start_i,
    input  wire [271:0]         activation_block_i,
    input  wire [271:0]         weight_block_i,
    output reg                  busy_o,
    output reg                  done_o,
    output reg  signed [31:0]   dot_o,
    output reg  signed [63:0]   term_q30_o,
    output reg                  scale_error_o
);
    wire accept = start_i && !busy_o;
    wire dot_valid;
    wire signed [20:0] dot;
    reg [15:0] activation_scale_reg;
    reg [15:0] weight_scale_reg;
    wire scaler_start;
    wire scaler_busy;
    wire scaler_done;
    wire signed [63:0] scaler_term;
    wire scaler_error;

    assign scaler_start = dot_valid && busy_o && !scaler_busy;

    truega_q8_0_dot32 dot32 (
        .clk(clk),
        .reset_n(reset_n),
        .valid_i(accept),
        .activation_quants_i(activation_block_i[271:16]),
        .weight_quants_i(weight_block_i[271:16]),
        .valid_o(dot_valid),
        .dot_o(dot)
    );

    truega_q8_0_scale_q30_seq scale_q30 (
        .clk(clk),
        .reset_n(reset_n),
        .start_i(scaler_start),
        .dot_i(dot),
        .activation_scale_f16_i(activation_scale_reg),
        .weight_scale_f16_i(weight_scale_reg),
        .busy_o(scaler_busy),
        .done_o(scaler_done),
        .term_q30_o(scaler_term),
        .scale_error_o(scaler_error)
    );

    always @(posedge clk) begin
        if (!reset_n) begin
            activation_scale_reg <= 16'd0;
            weight_scale_reg <= 16'd0;
            busy_o <= 1'b0;
            done_o <= 1'b0;
            dot_o <= 32'sd0;
            term_q30_o <= 64'sd0;
            scale_error_o <= 1'b0;
        end else begin
            done_o <= 1'b0;
            if (accept) begin
                activation_scale_reg <= activation_block_i[15:0];
                weight_scale_reg <= weight_block_i[15:0];
                busy_o <= 1'b1;
                dot_o <= 32'sd0;
                term_q30_o <= 64'sd0;
                scale_error_o <= 1'b0;
            end
            if (dot_valid && busy_o)
                dot_o <= {{11{dot[20]}}, dot};
            if (scaler_done) begin
                busy_o <= 1'b0;
                done_o <= 1'b1;
                term_q30_o <= scaler_term;
                scale_error_o <= scaler_error;
            end
        end
    end
endmodule

// Stateful Q8_0 row sequencer for one block per 72-byte inline BAR call.
//
// The caller supplies a four-byte little-endian control header followed by the
// unchanged 34-byte activation and weight blocks:
//   byte 0: bit 0 = first, bit 1 = last; all other bits must be zero
//           bit 2 = wide row (144 blocks instead of 32)
//   byte 1: block index, 0..31 normally or 0..143 in wide mode
//   byte 2..3: reserved, must be zero
//
// Every accepted call returns the exact block dot, exact block Q30 term, and
// the signed Q30 row accumulator after that term.  A normal row is 0..31;
// first|last with index zero preserves the existing one-block diagnostic.  A
// new valid first block explicitly restarts an incomplete row.  Invalid order
// aborts row state and retires with error_o asserted.
//
// ROW_DIAGNOSTIC_ENABLE defaults to zero.  The active generated function
// wrapper must opt in together with its paired Rust ABI change.
module truega_q8_0_row_block_slot #(
    parameter ROW_DIAGNOSTIC_ENABLE = 0
) (
    input  wire                 clk,
    input  wire                 reset_n,
    input  wire                 start_i,
    input  wire [31:0]          control_i,
    input  wire [271:0]         activation_block_i,
    input  wire [271:0]         weight_block_i,
    output reg                  busy_o,
    output reg                  done_o,
    output reg                  error_o,
    output reg  signed [31:0]   dot_o,
    output reg  signed [63:0]   term_q30_o,
    output reg  signed [63:0]   row_q30_o
);
    wire first_i = control_i[0];
    wire last_i = control_i[1];
    wire wide_i = control_i[2];
    wire [7:0] block_index_i = control_i[15:8];
    wire control_reserved = (control_i[31:16] != 16'd0)
                         || (control_i[7:3] != 5'd0);
    wire accept = ROW_DIAGNOSTIC_ENABLE && start_i && !busy_o;
    reg row_active;
    reg [7:0] expected_index;
    reg signed [63:0] accumulator;
    reg active_first;
    reg active_last;
    reg active_wide;
    reg [271:0] activation_block_reg;
    reg [271:0] weight_block_reg;
    reg block_start;
    wire block_busy;
    wire block_done;
    wire signed [31:0] block_dot;
    wire signed [63:0] block_term_q30;
    wire block_scale_error;

    wire [7:0] final_index_i = wide_i ? 8'd143 : 8'd31;
    wire sequence_valid = !control_reserved
                       && (block_index_i <= final_index_i)
                       && (first_i
                           ? (block_index_i == 8'd0)
                           : (row_active
                              && (wide_i == active_wide)
                              && (block_index_i == expected_index)))
                       && (last_i
                           ? ((first_i && (block_index_i == 8'd0))
                              || (block_index_i == final_index_i))
                           : (block_index_i != final_index_i));

    truega_q8_0_block_slot block_slot (
        .clk(clk),
        .reset_n(reset_n),
        .start_i(block_start),
        .activation_block_i(activation_block_reg),
        .weight_block_i(weight_block_reg),
        .busy_o(block_busy),
        .done_o(block_done),
        .dot_o(block_dot),
        .term_q30_o(block_term_q30),
        .scale_error_o(block_scale_error)
    );

    always @(posedge clk) begin
        if (!reset_n) begin
            busy_o <= 1'b0;
            done_o <= 1'b0;
            error_o <= 1'b0;
            dot_o <= 32'sd0;
            term_q30_o <= 64'sd0;
            row_q30_o <= 64'sd0;
            row_active <= 1'b0;
            expected_index <= 8'd0;
            accumulator <= 64'sd0;
            active_first <= 1'b0;
            active_last <= 1'b0;
            active_wide <= 1'b0;
            activation_block_reg <= 272'd0;
            weight_block_reg <= 272'd0;
            block_start <= 1'b0;
        end else begin
            done_o <= 1'b0;
            block_start <= 1'b0;

            if (accept) begin
                dot_o <= 32'sd0;
                term_q30_o <= 64'sd0;
                row_q30_o <= 64'sd0;
                if (!sequence_valid) begin
                    busy_o <= 1'b0;
                    done_o <= 1'b1;
                    error_o <= 1'b1;
                    row_active <= 1'b0;
                    expected_index <= 8'd0;
                    accumulator <= 64'sd0;
                end else begin
                    busy_o <= 1'b1;
                    error_o <= 1'b0;
                    active_first <= first_i;
                    active_last <= last_i;
                    active_wide <= wide_i;
                    activation_block_reg <= activation_block_i;
                    weight_block_reg <= weight_block_i;
                    block_start <= 1'b1;
                    if (first_i) begin
                        row_active <= 1'b0;
                        expected_index <= 8'd0;
                        accumulator <= 64'sd0;
                    end
                end
            end else if (busy_o && block_done) begin
                busy_o <= 1'b0;
                done_o <= 1'b1;
                error_o <= block_scale_error;
                dot_o <= block_dot;
                term_q30_o <= block_term_q30;
                if (active_first) begin
                    accumulator <= block_term_q30;
                    row_q30_o <= block_term_q30;
                end else begin
                    accumulator <= accumulator + block_term_q30;
                    row_q30_o <= accumulator + block_term_q30;
                end

                if (block_scale_error || active_last) begin
                    row_active <= 1'b0;
                    expected_index <= 8'd0;
                end else begin
                    row_active <= 1'b1;
                    expected_index <= active_first ? 8'd1 : expected_index + 8'd1;
                end
            end
        end
    end

    wire unused_block_busy = block_busy;
endmodule

// Cached-activation Q8_0 row sequencer.
//
// The original inline operation remains unchanged when control bits 5:4 are zero.
// Two additional fixed operations reuse the same 72-byte input envelope:
//   bit 4: cache activation_block_i at block_index (weight_block_i is ignored)
//   bit 5: process two consecutive cached activations; activation_block_i carries
//          weight block N and weight_block_i carries weight block N+1
//
// Pair mode retires only after both exact block operations have accumulated.  Its
// diagnostic dot/term are those of N+1 and row_q30 is the accumulator after both
// terms.  This halves work-package/MSI traffic without changing native Q8_0 blocks,
// the fixed envelopes, or the proven single-block compatibility path.
module truega_q8_0_cached_pair_slot #(
    parameter CACHED_PAIR_ENABLE = 0
) (
    input  wire                 clk,
    input  wire                 reset_n,
    input  wire                 start_i,
    input  wire [31:0]          control_i,
    input  wire [271:0]         activation_block_i,
    input  wire [271:0]         weight_block_i,
    output reg                  busy_o,
    output reg                  done_o,
    output reg                  error_o,
    output reg  signed [31:0]   dot_o,
    output reg  signed [63:0]   term_q30_o,
    output reg  signed [63:0]   row_q30_o
);
    localparam [3:0] STATE_IDLE        = 4'd0;
    localparam [3:0] STATE_DECODE      = 4'd1;
    localparam [3:0] STATE_SINGLE      = 4'd2;
    localparam [3:0] STATE_SINGLE_WAIT = 4'd3;
    localparam [3:0] STATE_PAIR_READ0  = 4'd4;
    localparam [3:0] STATE_PAIR_START0 = 4'd5;
    localparam [3:0] STATE_PAIR_WAIT0  = 4'd6;
    localparam [3:0] STATE_PAIR_START1 = 4'd7;
    localparam [3:0] STATE_PAIR_WAIT1  = 4'd8;

    wire accept = CACHED_PAIR_ENABLE && start_i && !busy_o;

    // A synchronous read and unreset storage allow Gowin to infer block RAM.  Only
    // validity is reset; cached data is never consumed until both pair entries have
    // been explicitly loaded by the host.
    reg [271:0] activation_cache [0:143];
    reg [143:0] cache_valid;
    reg [7:0] cache_read_index;
    reg [271:0] cache_read_data;

    reg [3:0] state;
    reg [31:0] active_control;
    reg [271:0] payload0_reg;
    reg [271:0] payload1_reg;
    reg [31:0] row_control;
    reg [271:0] row_activation;
    reg [271:0] row_weight;
    reg row_start;
    wire row_busy;
    wire row_done;
    wire row_error;
    wire signed [31:0] row_dot;
    wire signed [63:0] row_term_q30;
    wire signed [63:0] row_accumulator_q30;

    wire active_wide = active_control[2];
    wire active_cache_load = active_control[4];
    wire active_cached_pair = active_control[5];
    wire [7:0] active_block_index = active_control[15:8];
    wire [7:0] active_final_index = active_wide ? 8'd143 : 8'd31;
    wire active_control_valid = active_control[31:16] == 16'd0
                             && active_control[7:6] == 2'd0
                             && !(active_cache_load && active_cached_pair);
    wire cache_load_valid = active_control_valid && active_cache_load
                          && active_block_index <= active_final_index;
    wire cached_pair_valid = active_control_valid && active_cached_pair
                           && active_block_index < active_final_index
                           && active_block_index[0] == 1'b0;
    wire cache_write = state == STATE_DECODE && cache_load_valid;

    always @(posedge clk) begin
        if (cache_write)
            activation_cache[active_block_index] <= payload0_reg;
        cache_read_data <= activation_cache[cache_read_index];
    end

    truega_q8_0_row_block_slot #(
        .ROW_DIAGNOSTIC_ENABLE(1)
    ) row_slot (
        .clk(clk),
        .reset_n(reset_n),
        .start_i(row_start),
        .control_i(row_control),
        .activation_block_i(row_activation),
        .weight_block_i(row_weight),
        .busy_o(row_busy),
        .done_o(row_done),
        .error_o(row_error),
        .dot_o(row_dot),
        .term_q30_o(row_term_q30),
        .row_q30_o(row_accumulator_q30)
    );

    always @(posedge clk) begin
        if (!reset_n) begin
            state <= STATE_IDLE;
            cache_valid <= 144'd0;
            cache_read_index <= 8'd0;
            active_control <= 32'd0;
            payload0_reg <= 272'd0;
            payload1_reg <= 272'd0;
            row_control <= 32'd0;
            row_activation <= 272'd0;
            row_weight <= 272'd0;
            row_start <= 1'b0;
            busy_o <= 1'b0;
            done_o <= 1'b0;
            error_o <= 1'b0;
            dot_o <= 32'sd0;
            term_q30_o <= 64'sd0;
            row_q30_o <= 64'sd0;
        end else begin
            row_start <= 1'b0;
            done_o <= 1'b0;

            case (state)
                STATE_IDLE: begin
                    busy_o <= 1'b0;
                    if (accept) begin
                        dot_o <= 32'sd0;
                        term_q30_o <= 64'sd0;
                        row_q30_o <= 64'sd0;
                        error_o <= 1'b0;
                        active_control <= control_i;
                        payload0_reg <= activation_block_i;
                        payload1_reg <= weight_block_i;
                        busy_o <= 1'b1;
                        state <= STATE_DECODE;
                    end
                end

                // Decode only registered package data. Besides simplifying the
                // slot boundary, this prevents BAR input fanout from reaching the
                // block-RAM address/write-enable path in one 100 MHz cycle.
                STATE_DECODE: begin
                        // Select the registered index independently of validation;
                        // invalid operations never consume the RAM output. This
                        // keeps cache-valid reduction logic off the BRAM address CE.
                        cache_read_index <= active_block_index;
                        if (active_cache_load) begin
                            busy_o <= 1'b0;
                            done_o <= 1'b1;
                            if (cache_load_valid) begin
                                cache_valid[active_block_index] <= 1'b1;
                            end else begin
                                error_o <= 1'b1;
                            end
                            state <= STATE_IDLE;
                        end else if (active_cached_pair) begin
                            if (!cached_pair_valid
                                    || !cache_valid[active_block_index]
                                    || !cache_valid[active_block_index + 8'd1]) begin
                                busy_o <= 1'b0;
                                done_o <= 1'b1;
                                error_o <= 1'b1;
                                state <= STATE_IDLE;
                            end else begin
                                state <= STATE_PAIR_READ0;
                            end
                        end else if (!active_control_valid) begin
                            busy_o <= 1'b0;
                            done_o <= 1'b1;
                            error_o <= 1'b1;
                            state <= STATE_IDLE;
                        end else begin
                            state <= STATE_SINGLE;
                        end
                end

                STATE_SINGLE: begin
                    row_control <= active_control;
                    row_activation <= payload0_reg;
                    row_weight <= payload1_reg;
                    row_start <= 1'b1;
                    state <= STATE_SINGLE_WAIT;
                end

                STATE_SINGLE_WAIT: begin
                    if (row_done) begin
                        busy_o <= 1'b0;
                        done_o <= 1'b1;
                        error_o <= row_error;
                        dot_o <= row_dot;
                        term_q30_o <= row_term_q30;
                        row_q30_o <= row_accumulator_q30;
                        state <= STATE_IDLE;
                    end
                end

                // Wait one full clock after selecting the synchronous cache port.
                STATE_PAIR_READ0: begin
                    state <= STATE_PAIR_START0;
                end

                STATE_PAIR_START0: begin
                    row_control <= {active_control[31:16], active_control[15:8],
                                    5'd0, active_control[2], 1'b0, active_control[0]};
                    row_activation <= cache_read_data;
                    row_weight <= payload0_reg;
                    row_start <= 1'b1;
                    cache_read_index <= active_control[15:8] + 8'd1;
                    state <= STATE_PAIR_WAIT0;
                end

                STATE_PAIR_WAIT0: begin
                    if (row_done) begin
                        if (row_error) begin
                            busy_o <= 1'b0;
                            done_o <= 1'b1;
                            error_o <= 1'b1;
                            state <= STATE_IDLE;
                        end else begin
                            state <= STATE_PAIR_START1;
                        end
                    end
                end

                STATE_PAIR_START1: begin
                    row_control <= {active_control[31:16],
                                    active_control[15:8] + 8'd1,
                                    5'd0, active_control[2], active_control[1], 1'b0};
                    row_activation <= cache_read_data;
                    row_weight <= payload1_reg;
                    row_start <= 1'b1;
                    state <= STATE_PAIR_WAIT1;
                end

                STATE_PAIR_WAIT1: begin
                    if (row_done) begin
                        busy_o <= 1'b0;
                        done_o <= 1'b1;
                        error_o <= row_error;
                        dot_o <= row_dot;
                        term_q30_o <= row_term_q30;
                        row_q30_o <= row_accumulator_q30;
                        state <= STATE_IDLE;
                    end
                end

                default: begin
                    state <= STATE_IDLE;
                    busy_o <= 1'b0;
                    done_o <= 1'b1;
                    error_o <= 1'b1;
                end
            endcase
        end
    end

    wire unused_row_busy = row_busy;
endmodule

// Fixed layer-0 LFM2.5 SiLU(gate) * up datapath.
//
// Inputs and output are signed Q30.  The sigmoid is the odd ninth-order
// expansion around zero, evaluated with one shared sequential multiplier:
//   1/2 + x*(1/4 - x^2/48 + x^4/480 - 17*x^6/80640
//              + 31*x^8/1451520)
// The sealed layer-0 gate is inside +/-1.01; +/-1.125 is enforced so this
// circuit cannot silently operate outside its verified approximation domain.
module truega_lfm25_silu_q30_slot #(
    parameter SILU_ENABLE = 0
) (
    input  wire                clk,
    input  wire                reset_n,
    input  wire                start_i,
    input  wire signed [63:0]  gate_q30_i,
    input  wire signed [63:0]  up_q30_i,
    output reg                 busy_o,
    output reg                 done_o,
    output reg                 error_o,
    output reg signed [63:0]   result_q30_o
);
    localparam signed [63:0] GATE_LIMIT_Q30 = 64'sd1207959552; // 1.125
    localparam signed [63:0] UP_LIMIT_Q30   = 64'sd2147483648; // 2.0
    localparam signed [63:0] HALF_Q30 = 64'sd536870912;
    localparam signed [63:0] C1_Q30 = 64'sd268435456;
    localparam signed [63:0] C3_Q30 = -64'sd22369621;
    localparam signed [63:0] C5_Q30 = 64'sd2236962;
    localparam signed [63:0] C7_Q30 = -64'sd226359;
    localparam signed [63:0] C9_Q30 = 64'sd22931;

    localparam [3:0] ST_IDLE = 4'd0;
    localparam [3:0] ST_X2   = 4'd1;
    localparam [3:0] ST_P7   = 4'd2;
    localparam [3:0] ST_P5   = 4'd3;
    localparam [3:0] ST_P3   = 4'd4;
    localparam [3:0] ST_P1   = 4'd5;
    localparam [3:0] ST_SIG  = 4'd6;
    localparam [3:0] ST_SILU = 4'd7;
    localparam [3:0] ST_OUT  = 4'd8;

    reg [3:0] state;
    reg signed [63:0] gate_q30;
    reg signed [63:0] up_q30;
    reg signed [63:0] x2_q30;
    reg signed [63:0] polynomial_q30;
    reg signed [63:0] sigmoid_q30;
    reg signed [63:0] silu_q30;

    reg signed [39:0] multiply_left;
    reg signed [39:0] multiply_right;
    reg multiply_start;
    reg multiply_waiting;
    wire multiply_busy;
    wire multiply_done;
    wire signed [63:0] multiply_q30;
    wire input_range_valid = (gate_q30_i >= -GATE_LIMIT_Q30)
                          && (gate_q30_i <= GATE_LIMIT_Q30)
                          && (up_q30_i >= -UP_LIMIT_Q30)
                          && (up_q30_i <= UP_LIMIT_Q30);

    always @* begin
        multiply_left = 40'sd0;
        multiply_right = 40'sd0;
        case (state)
            ST_X2: begin
                multiply_left = gate_q30[39:0];
                multiply_right = gate_q30[39:0];
            end
            ST_P7, ST_P5, ST_P3, ST_P1: begin
                multiply_left = x2_q30[39:0];
                multiply_right = polynomial_q30[39:0];
            end
            ST_SIG: begin
                multiply_left = gate_q30[39:0];
                multiply_right = polynomial_q30[39:0];
            end
            ST_SILU: begin
                multiply_left = gate_q30[39:0];
                multiply_right = sigmoid_q30[39:0];
            end
            ST_OUT: begin
                multiply_left = silu_q30[39:0];
                multiply_right = up_q30[39:0];
            end
            default: begin end
        endcase
    end

    truega_signed_mul_q30_seq multiply (
        .clk(clk),
        .reset_n(reset_n),
        .start_i(multiply_start),
        .left_i(multiply_left),
        .right_i(multiply_right),
        .busy_o(multiply_busy),
        .done_o(multiply_done),
        .result_q30_o(multiply_q30)
    );

    always @(posedge clk) begin
        if (!reset_n) begin
            state <= ST_IDLE;
            busy_o <= 1'b0;
            done_o <= 1'b0;
            error_o <= 1'b0;
            result_q30_o <= 64'sd0;
            gate_q30 <= 64'sd0;
            up_q30 <= 64'sd0;
            x2_q30 <= 64'sd0;
            polynomial_q30 <= 64'sd0;
            sigmoid_q30 <= 64'sd0;
            silu_q30 <= 64'sd0;
            multiply_start <= 1'b0;
            multiply_waiting <= 1'b0;
        end else begin
            done_o <= 1'b0;
            multiply_start <= 1'b0;
            if (SILU_ENABLE && start_i && !busy_o) begin
                result_q30_o <= 64'sd0;
                if (!input_range_valid) begin
                    state <= ST_IDLE;
                    busy_o <= 1'b0;
                    done_o <= 1'b1;
                    error_o <= 1'b1;
                    multiply_waiting <= 1'b0;
                end else begin
                    gate_q30 <= gate_q30_i;
                    up_q30 <= up_q30_i;
                    state <= ST_X2;
                    busy_o <= 1'b1;
                    error_o <= 1'b0;
                    multiply_waiting <= 1'b0;
                end
            end else if (busy_o) begin
                if (!multiply_waiting) begin
                    multiply_start <= 1'b1;
                    multiply_waiting <= 1'b1;
                end else if (multiply_done) begin
                    multiply_waiting <= 1'b0;
                    case (state)
                        ST_X2: begin
                            x2_q30 <= multiply_q30;
                            polynomial_q30 <= C9_Q30;
                            state <= ST_P7;
                        end
                        ST_P7: begin
                            polynomial_q30 <= C7_Q30 + multiply_q30;
                            state <= ST_P5;
                        end
                        ST_P5: begin
                            polynomial_q30 <= C5_Q30 + multiply_q30;
                            state <= ST_P3;
                        end
                        ST_P3: begin
                            polynomial_q30 <= C3_Q30 + multiply_q30;
                            state <= ST_P1;
                        end
                        ST_P1: begin
                            polynomial_q30 <= C1_Q30 + multiply_q30;
                            state <= ST_SIG;
                        end
                        ST_SIG: begin
                            sigmoid_q30 <= HALF_Q30 + multiply_q30;
                            state <= ST_SILU;
                        end
                        ST_SILU: begin
                            silu_q30 <= multiply_q30;
                            state <= ST_OUT;
                        end
                        ST_OUT: begin
                            result_q30_o <= multiply_q30;
                            busy_o <= 1'b0;
                            done_o <= 1'b1;
                            state <= ST_IDLE;
                        end
                        default: begin
                            busy_o <= 1'b0;
                            done_o <= 1'b1;
                            error_o <= 1'b1;
                            state <= ST_IDLE;
                        end
                    endcase
                end
            end
        end
    end

    wire unused_multiply_busy = multiply_busy;
endmodule

// Exact signed 40x40 multiply followed by round-to-nearest-ties-even at Q30.
// One multiplier bit is consumed per cycle; magnitude rounding and sign are
// separately registered so no multiplier/round/add chain crosses one clock.
module truega_signed_mul_q30_seq (
    input  wire                clk,
    input  wire                reset_n,
    input  wire                start_i,
    input  wire signed [39:0]  left_i,
    input  wire signed [39:0]  right_i,
    output reg                 busy_o,
    output reg                 done_o,
    output reg signed [63:0]   result_q30_o
);
    localparam [1:0] PHASE_MULTIPLY = 2'd0;
    localparam [1:0] PHASE_ROUND    = 2'd1;
    localparam [1:0] PHASE_SIGN     = 2'd2;

    reg [1:0] phase;
    reg [5:0] bit_index;
    reg negative;
    reg [79:0] multiplicand;
    reg [39:0] multiplier;
    reg [79:0] accumulator;
    reg [79:0] product_magnitude;
    reg [49:0] rounded_magnitude;

    wire [39:0] left_magnitude = left_i[39] ? (~left_i[39:0] + 40'd1) : left_i[39:0];
    wire [39:0] right_magnitude = right_i[39] ? (~right_i[39:0] + 40'd1) : right_i[39:0];
    wire [79:0] addend = multiplier[0] ? multiplicand : 80'd0;
    wire [79:0] accumulator_next = accumulator + addend;
    wire [49:0] quotient = product_magnitude[79:30];
    wire [29:0] remainder = product_magnitude[29:0];
    wire round_increment = (remainder > 30'h20000000)
                        || ((remainder == 30'h20000000) && quotient[0]);

    always @(posedge clk) begin
        if (!reset_n) begin
            busy_o <= 1'b0;
            done_o <= 1'b0;
            result_q30_o <= 64'sd0;
            phase <= PHASE_MULTIPLY;
            bit_index <= 6'd0;
            negative <= 1'b0;
            multiplicand <= 80'd0;
            multiplier <= 40'd0;
            accumulator <= 80'd0;
            product_magnitude <= 80'd0;
            rounded_magnitude <= 50'd0;
        end else begin
            done_o <= 1'b0;
            if (start_i && !busy_o) begin
                busy_o <= 1'b1;
                phase <= PHASE_MULTIPLY;
                bit_index <= 6'd0;
                negative <= left_i[39] ^ right_i[39];
                multiplicand <= {40'd0, left_magnitude};
                multiplier <= right_magnitude;
                accumulator <= 80'd0;
            end else if (busy_o) begin
                case (phase)
                    PHASE_MULTIPLY: begin
                        accumulator <= accumulator_next;
                        multiplicand <= multiplicand << 1;
                        multiplier <= multiplier >> 1;
                        if (bit_index == 6'd39) begin
                            product_magnitude <= accumulator_next;
                            phase <= PHASE_ROUND;
                        end else begin
                            bit_index <= bit_index + 6'd1;
                        end
                    end
                    PHASE_ROUND: begin
                        rounded_magnitude <= quotient + round_increment;
                        phase <= PHASE_SIGN;
                    end
                    PHASE_SIGN: begin
                        result_q30_o <= negative
                            ? -$signed({14'd0, rounded_magnitude})
                            :  $signed({14'd0, rounded_magnitude});
                        busy_o <= 1'b0;
                        done_o <= 1'b1;
                        phase <= PHASE_MULTIPLY;
                    end
                    default: begin
                        busy_o <= 1'b0;
                        done_o <= 1'b1;
                        result_q30_o <= 64'sd0;
                        phase <= PHASE_MULTIPLY;
                    end
                endcase
            end
        end
    end
endmodule


// Read-only build manifest paired with the generated host interface.
module truega_firmware_manifest(word_index,data);
input wire [4:0] word_index;
output reg [31:0] data;
always @(*) begin
data = 32'h00000000;
case (word_index)
            5'd0: data = 32'h4D465754;
            5'd1: data = 32'h00030001;
            5'd2: data = 32'h00000100;
            5'd3: data = 32'h00000000;
            5'd4: data = 32'hE2383F8B;
            5'd5: data = 32'hD10A6C85;
            5'd6: data = 32'hCE2D6A73;
            5'd7: data = 32'hF02E801E;
            5'd8: data = 32'h3957F391;
            5'd9: data = 32'hC0CB4020;
            5'd10: data = 32'hA77D65FB;
            5'd11: data = 32'h0A806CC6;
            5'd12: data = 32'h00000000;
            5'd13: data = 32'h00000004;
            5'd14: data = 32'h82C72268;
            5'd15: data = 32'h63D2650B;
            5'd16: data = 32'h00080001;
            5'd17: data = 32'h00000004;
            5'd18: data = 32'h379E9CDF;
            5'd19: data = 32'hE32D0CD1;
            5'd20: data = 32'h00480002;
            5'd21: data = 32'h00000014;
            5'd22: data = 32'h59401DB0;
            5'd23: data = 32'h308BEADA;
            5'd24: data = 32'h00000000;
            5'd25: data = 32'h00000000;
            5'd26: data = 32'h00000000;
            5'd27: data = 32'h00000000;
            5'd28: data = 32'h00000000;
            5'd29: data = 32'h00000000;
            5'd30: data = 32'h00000000;
            5'd31: data = 32'h00000000;
            default: data = 32'h00000000;
endcase
end
endmodule
