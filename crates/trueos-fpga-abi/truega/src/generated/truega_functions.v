

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
            5'd4: data = 32'h5DAF6234;
            5'd5: data = 32'hA47D6F08;
            5'd6: data = 32'hB74E326F;
            5'd7: data = 32'hBD0E6676;
            5'd8: data = 32'h5273C3D6;
            5'd9: data = 32'hC4F9869B;
            5'd10: data = 32'h036F462E;
            5'd11: data = 32'h4772E56F;
            5'd12: data = 32'h00000000;
            5'd13: data = 32'h00000004;
            5'd14: data = 32'h82C72268;
            5'd15: data = 32'h63D2650B;
            5'd16: data = 32'h00080001;
            5'd17: data = 32'h00000004;
            5'd18: data = 32'h379E9CDF;
            5'd19: data = 32'hE32D0CD1;
            5'd20: data = 32'h00080002;
            5'd21: data = 32'h00000004;
            5'd22: data = 32'h26D65E41;
            5'd23: data = 32'hAFCAD32A;
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
