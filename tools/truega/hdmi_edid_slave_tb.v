module hdmi_edid_slave_tb;
    reg clk = 0, reset = 1, scl = 1, master_low = 0;
    tri1 sda;
    assign sda = master_low ? 1'b0 : 1'bz;
    always #1 clk = !clk;
    wire address_seen;
    hdmi_edid_slave dut (.clk(clk), .reset(reset), .scl(scl),
                         .sda(sda), .address_seen(address_seen));

    reg [7:0] expected [0:255];
    reg [7:0] value;
    integer i;

    task start;
        begin
            scl = 0; master_low = 0; #20;
            scl = 1; #20;
            master_low = 1; #20;
            scl = 0; #20;
        end
    endtask

    task stop;
        begin
            master_low = 1; #20;
            scl = 1; #20;
            master_low = 0; #20;
        end
    endtask

    task write_byte(input [7:0] byte_in, input expect_ack);
        integer bit_number;
        begin
            for (bit_number = 7; bit_number >= 0; bit_number = bit_number - 1) begin
                master_low = !byte_in[bit_number]; #20;
                scl = 1; #20;
                scl = 0; #20;
            end
            master_low = 0; #20;
            scl = 1; #20;
            if (sda !== !expect_ack) $fatal(1, "Unexpected ACK for %h", byte_in);
            scl = 0; #20;
        end
    endtask

    task read_byte(output [7:0] byte_out, input more);
        integer bit_number;
        begin
            master_low = 0;
            for (bit_number = 7; bit_number >= 0; bit_number = bit_number - 1) begin
                #20; scl = 1; #20;
                byte_out[bit_number] = sda;
                scl = 0; #20;
            end
            master_low = more; #20;
            scl = 1; #20;
            scl = 0; #20;
            master_low = 0;
        end
    endtask

    initial begin
        $readmemh("z7-edid.hex", expected);
        #20; reset = 0; #20;
        start; write_byte(8'ha2, 0); stop;
        if (address_seen) $fatal(1, "Wrong address marked as a DDC request");
        start; write_byte(8'ha0, 1); write_byte(8'h00, 1);
        start; write_byte(8'ha1, 1);
        for (i = 0; i < 256; i = i + 1) begin
            read_byte(value, i != 255);
            if (value !== expected[i])
                $fatal(1, "EDID byte %0d: got %h expected %h", i, value, expected[i]);
        end
        stop;
        start; write_byte(8'ha0, 1); write_byte(8'h80, 1);
        start; write_byte(8'ha1, 1); read_byte(value, 0); stop;
        if (value !== expected[128]) $fatal(1, "Extension random read failed");
        if (!address_seen) $fatal(1, "DDC address was not recorded");
        $display("Verified all EDID bytes and extension random read over DDC");
        $finish;
    end
endmodule
