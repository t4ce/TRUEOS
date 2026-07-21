

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

    truega_q8_0_row_block_slot #(
        .ROW_DIAGNOSTIC_ENABLE(1)
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
            next_led <= 5'b00001;
            output_data <= 768'd0;
            busy <= 1'b0;
            done <= 1'b0;
            error <= 1'b0;
        end else begin
            q8_start <= 1'b0;
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
                        q8_start <= 1'b1;
                    end
                    default: begin
                        busy <= 1'b0;
                        done <= 1'b1;
                        error <= 1'b1;
                    end
                endcase
            end else if (busy && active_function == 16'd2 && q8_done) begin
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
endmodule

// Exact native Q8_0 compute sources fused into this generated bundle.
// Exact 32-lane signed Q8_0 integer dot product.
// Six-cycle latency, one unchanged 34-byte native-image block per cycle.
module truega_q8_0_dot32 (
    input  wire                 clk,
    input  wire                 reset_n,
    input  wire                 valid_i,
    input  wire [255:0]         activation_quants_i,
    input  wire [255:0]         weight_quants_i,
    output wire                 valid_o,
    output wire signed [20:0]   dot_o
);
    reg signed [15:0] product [0:31];
    reg signed [16:0] sum_1 [0:15];
    reg signed [17:0] sum_2 [0:7];
    reg signed [18:0] sum_3 [0:3];
    reg signed [19:0] sum_4 [0:1];
    reg signed [20:0] sum_5;
    reg [5:0] valid_pipe;
    integer lane;

    assign valid_o = valid_pipe[5];
    assign dot_o = sum_5;

    always @(posedge clk) begin
        if (!reset_n) begin
            valid_pipe <= 6'b0;
            sum_5 <= 21'sd0;
            for (lane = 0; lane < 32; lane = lane + 1)
                product[lane] <= 16'sd0;
            for (lane = 0; lane < 16; lane = lane + 1)
                sum_1[lane] <= 17'sd0;
            for (lane = 0; lane < 8; lane = lane + 1)
                sum_2[lane] <= 18'sd0;
            for (lane = 0; lane < 4; lane = lane + 1)
                sum_3[lane] <= 19'sd0;
            for (lane = 0; lane < 2; lane = lane + 1)
                sum_4[lane] <= 20'sd0;
        end else begin
            valid_pipe <= {valid_pipe[4:0], valid_i};

            for (lane = 0; lane < 32; lane = lane + 1)
                product[lane] <= $signed(activation_quants_i[lane*8 +: 8])
                               * $signed(weight_quants_i[lane*8 +: 8]);

            for (lane = 0; lane < 16; lane = lane + 1)
                sum_1[lane] <= {{1{product[lane*2][15]}}, product[lane*2]}
                             + {{1{product[lane*2 + 1][15]}}, product[lane*2 + 1]};
            for (lane = 0; lane < 8; lane = lane + 1)
                sum_2[lane] <= {{1{sum_1[lane*2][16]}}, sum_1[lane*2]}
                             + {{1{sum_1[lane*2 + 1][16]}}, sum_1[lane*2 + 1]};
            for (lane = 0; lane < 4; lane = lane + 1)
                sum_3[lane] <= {{1{sum_2[lane*2][17]}}, sum_2[lane*2]}
                             + {{1{sum_2[lane*2 + 1][17]}}, sum_2[lane*2 + 1]};
            for (lane = 0; lane < 2; lane = lane + 1)
                sum_4[lane] <= {{1{sum_3[lane*2][18]}}, sum_3[lane*2]}
                             + {{1{sum_3[lane*2 + 1][18]}}, sum_3[lane*2 + 1]};
            sum_5 <= {{1{sum_4[0][19]}}, sum_4[0]}
                   + {{1{sum_4[1][19]}}, sum_4[1]};
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
// ignored.  done_o pulses for one cycle after both registered outputs are valid.
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
//   byte 1: block index, 0..31
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
    wire [7:0] block_index_i = control_i[15:8];
    wire control_reserved = (control_i[31:16] != 16'd0)
                         || (control_i[7:2] != 6'd0);
    wire accept = ROW_DIAGNOSTIC_ENABLE && start_i && !busy_o;
    reg row_active;
    reg [5:0] expected_index;
    reg signed [63:0] accumulator;
    reg active_first;
    reg active_last;
    reg [271:0] activation_block_reg;
    reg [271:0] weight_block_reg;
    reg block_start;
    wire block_busy;
    wire block_done;
    wire signed [31:0] block_dot;
    wire signed [63:0] block_term_q30;
    wire block_scale_error;

    wire sequence_valid = !control_reserved
                       && (block_index_i < 8'd32)
                       && (first_i
                           ? (block_index_i == 8'd0)
                           : (row_active
                              && (block_index_i == expected_index)))
                       && (last_i
                           ? ((first_i && (block_index_i == 8'd0))
                              || (block_index_i == 8'd31))
                           : (block_index_i != 8'd31));

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
            expected_index <= 6'd0;
            accumulator <= 64'sd0;
            active_first <= 1'b0;
            active_last <= 1'b0;
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
                    expected_index <= 6'd0;
                    accumulator <= 64'sd0;
                end else begin
                    busy_o <= 1'b1;
                    error_o <= 1'b0;
                    active_first <= first_i;
                    active_last <= last_i;
                    activation_block_reg <= activation_block_i;
                    weight_block_reg <= weight_block_i;
                    block_start <= 1'b1;
                    if (first_i) begin
                        row_active <= 1'b0;
                        expected_index <= 6'd0;
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
                    expected_index <= 6'd0;
                end else begin
                    row_active <= 1'b1;
                    expected_index <= active_first ? 6'd1 : expected_index + 6'd1;
                end
            end
        end
    end

    wire unused_block_busy = block_busy;
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
            5'd4: data = 32'hB2D3F237;
            5'd5: data = 32'h0FB7EB4C;
            5'd6: data = 32'h20C08542;
            5'd7: data = 32'h52C1D50B;
            5'd8: data = 32'h8C95393B;
            5'd9: data = 32'h3503694F;
            5'd10: data = 32'h42B6982F;
            5'd11: data = 32'h7F1B02BC;
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
            5'd22: data = 32'h146A1E49;
            5'd23: data = 32'h954041AD;
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
