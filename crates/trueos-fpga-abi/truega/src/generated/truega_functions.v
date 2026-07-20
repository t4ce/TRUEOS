

module truega_functions(function_id,arg0,arg1,led_state,next_led,result,required_input_bytes,output_bytes,valid);
    
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
    reg  [31:0] xor_u32$a;
    reg  [31:0] xor_u32$b;
    wire  [31:0] xor_u32$result;
    
    // Sub module instances
    truega_functions$heartbeat heartbeat(
        .led_state(heartbeat$led_state),
        .next_led(heartbeat$next_led),
        .result(heartbeat$result)
    );
    truega_functions$add_u32 add_u32(
        .a(add_u32$a),
        .b(add_u32$b),
        .result(add_u32$result)
    );
    truega_functions$xor_u32 xor_u32(
        .a(xor_u32$a),
        .b(xor_u32$b),
        .result(xor_u32$result)
    );
    
    // Update code
    always @(*) begin
        heartbeat$led_state = led_state;
        add_u32$a = arg0;
        add_u32$b = arg1;
        xor_u32$a = arg0;
        xor_u32$b = arg1;
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
        else if (function_id == 32'h2) begin
            result = xor_u32$result;
            required_input_bytes = 32'h8;
            output_bytes = 32'h4;
            valid = 1'b1;
        end
    end
    
endmodule // top


module truega_functions$add_u32(a,b,result);
    
    // Module arguments
    input wire  [31:0] a;
    input wire  [31:0] b;
    output reg  [31:0] result;
    
    // Update code
    always @(*) begin
        result = a + b;
    end
    
endmodule // truega_functions$add_u32


module truega_functions$heartbeat(led_state,next_led,result);
    
    // Module arguments
    input wire  [4:0] led_state;
    output reg  [4:0] next_led;
    output reg  [31:0] result;
    
    // Update code
    always @(*) begin
        next_led = led_state + 32'h1;
        result = 32'h54534154;
    end
    
endmodule // truega_functions$heartbeat


module truega_functions$xor_u32(a,b,result);
    
    // Module arguments
    input wire  [31:0] a;
    input wire  [31:0] b;
    output reg  [31:0] result;
    
    // Update code
    always @(*) begin
        result = a ^ b;
    end
    
endmodule // truega_functions$xor_u32
