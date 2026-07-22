`timescale 1ns/1ps
module truega_lfm25_resident_tensor_store_tb;
    reg clk = 0;
    always #5 clk = ~clk;
    reg reset_n = 0;
    reg session_begin = 0;
    reg [31:0] session_begin_epoch = 0;
    wire [31:0] epoch;
    wire session_begin_done;
    wire session_begin_error;

    reg q30_tbegin = 0;
    reg q30_tcommit = 0;
    reg [3:0] q30_tslot = 0;
    reg [31:0] q30_tepoch = 0;
    wire q30_tbegin_ready;
    wire q30_tcommit_ready;
    reg q8_tbegin = 0;
    reg q8_tcommit = 0;
    reg [3:0] q8_tslot = 0;
    reg [31:0] q8_tepoch = 0;
    wire q8_tbegin_ready;
    wire q8_tcommit_ready;

    reg q30_wvalid = 0;
    reg [3:0] q30_wslot = 0;
    reg [9:0] q30_windex = 0;
    reg signed [63:0] q30_wdata = 0;
    reg [31:0] q30_wepoch = 0;
    wire q30_wready;
    reg q30_rvalid = 0;
    reg [3:0] q30_rslot = 0;
    reg [9:0] q30_rindex = 0;
    reg [31:0] q30_repoch = 0;
    wire q30_rsp;
    wire signed [63:0] q30_rdata;
    wire q30_rerror;

    reg q8_wvalid = 0;
    reg [3:0] q8_wslot = 0;
    reg [4:0] q8_wblock = 0;
    reg [271:0] q8_wdata = 0;
    reg [31:0] q8_wepoch = 0;
    wire q8_wready;
    reg q8_rvalid = 0;
    reg [3:0] q8_rslot = 0;
    reg [4:0] q8_rblock = 0;
    reg [31:0] q8_repoch = 0;
    wire q8_rsp;
    wire [271:0] q8_rdata;
    wire q8_rerror;

    truega_lfm25_resident_tensor_store #(.Q30_SLOTS(2), .Q8_SLOTS(2)) dut (
        .clk(clk), .reset_n(reset_n),
        .session_begin_i(session_begin),
        .session_begin_epoch_i(session_begin_epoch),
        .session_epoch_o(epoch), .session_begin_done_o(session_begin_done),
        .session_begin_error_o(session_begin_error),
        .q30_transaction_begin_i(q30_tbegin),
        .q30_transaction_commit_i(q30_tcommit),
        .q30_transaction_slot_i(q30_tslot),
        .q30_transaction_epoch_i(q30_tepoch),
        .q30_transaction_begin_ready_o(q30_tbegin_ready),
        .q30_transaction_commit_ready_o(q30_tcommit_ready),
        .q8_transaction_begin_i(q8_tbegin),
        .q8_transaction_commit_i(q8_tcommit),
        .q8_transaction_slot_i(q8_tslot),
        .q8_transaction_epoch_i(q8_tepoch),
        .q8_transaction_begin_ready_o(q8_tbegin_ready),
        .q8_transaction_commit_ready_o(q8_tcommit_ready),
        .q30_write_valid_i(q30_wvalid), .q30_write_slot_i(q30_wslot),
        .q30_write_index_i(q30_windex), .q30_write_data_i(q30_wdata),
        .q30_write_epoch_i(q30_wepoch), .q30_write_ready_o(q30_wready),
        .q30_read_valid_i(q30_rvalid), .q30_read_slot_i(q30_rslot),
        .q30_read_index_i(q30_rindex), .q30_read_epoch_i(q30_repoch),
        .q30_read_rsp_valid_o(q30_rsp), .q30_read_data_o(q30_rdata),
        .q30_read_error_o(q30_rerror),
        .q8_write_valid_i(q8_wvalid), .q8_write_slot_i(q8_wslot),
        .q8_write_block_i(q8_wblock), .q8_write_data_i(q8_wdata),
        .q8_write_epoch_i(q8_wepoch), .q8_write_ready_o(q8_wready),
        .q8_read_valid_i(q8_rvalid), .q8_read_slot_i(q8_rslot),
        .q8_read_block_i(q8_rblock), .q8_read_epoch_i(q8_repoch),
        .q8_read_rsp_valid_o(q8_rsp), .q8_read_data_o(q8_rdata),
        .q8_read_error_o(q8_rerror)
    );

    integer failures = 0;
    integer index;
    reg [31:0] first_epoch;

    task pulse_q30_read;
        input [31:0] requested_epoch;
        begin
            @(negedge clk);
            q30_repoch = requested_epoch;
            q30_rvalid = 1;
            @(negedge clk);
            q30_rvalid = 0;
        end
    endtask

    task pulse_q8_read;
        input [31:0] requested_epoch;
        begin
            @(negedge clk);
            q8_repoch = requested_epoch;
            q8_rvalid = 1;
            @(negedge clk);
            q8_rvalid = 0;
        end
    endtask

    initial begin
        repeat (3) @(posedge clk);
        reset_n = 1;
        @(negedge clk);
        session_begin_epoch = 32'h1234_0001;
        session_begin = 1;
        @(negedge clk);
        session_begin = 0;
        if (!session_begin_done || session_begin_error)
            failures = failures + 1;
        first_epoch = epoch;

        @(negedge clk);
        q30_tslot = 1;
        q30_tepoch = first_epoch;
        q30_tbegin = 1;
        q8_tslot = 1;
        q8_tepoch = first_epoch;
        q8_tbegin = 1;
        if (!q30_tbegin_ready || !q8_tbegin_ready)
            failures = failures + 1;
        @(negedge clk);
        q30_tbegin = 0;
        q8_tbegin = 0;

        // A begun but incomplete transaction is never readable.
        q30_wslot = 1;
        q30_windex = 10'd0;
        q30_wdata = 64'sd1000;
        q30_wepoch = first_epoch;
        q30_wvalid = 1;
        @(negedge clk);
        q30_wvalid = 0;
        q30_rslot = 1;
        q30_rindex = 10'd0;
        pulse_q30_read(first_epoch);
        if (q30_rsp || !q30_rerror)
            failures = failures + 1;

        // Complete both payloads in strict sequential order, then commit.
        for (index = 1; index < 1024; index = index + 1) begin
            @(negedge clk);
            q30_windex = index[9:0];
            q30_wdata = index == 777 ? -64'sd123456789 : index;
            q30_wvalid = 1;
            if (!q30_wready)
                failures = failures + 1;
        end
        @(negedge clk);
        q30_wvalid = 0;

        for (index = 0; index < 32; index = index + 1) begin
            @(negedge clk);
            q8_wslot = 1;
            q8_wblock = index[4:0];
            q8_wdata = index == 31
                ? {16'h3555, 256'h0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef}
                : index;
            q8_wepoch = first_epoch;
            q8_wvalid = 1;
            if (!q8_wready)
                failures = failures + 1;
        end
        @(negedge clk);
        q8_wvalid = 0;

        if (!q30_tcommit_ready || !q8_tcommit_ready)
            failures = failures + 1;
        q30_tcommit = 1;
        q8_tcommit = 1;
        @(negedge clk);
        q30_tcommit = 0;
        q8_tcommit = 0;

        q30_rslot = 1;
        q30_rindex = 10'd777;
        pulse_q30_read(first_epoch);
        if (!q30_rsp || q30_rerror || q30_rdata !== -64'sd123456789)
            failures = failures + 1;

        q8_rslot = 1;
        q8_rblock = 5'd31;
        pulse_q8_read(first_epoch);
        if (!q8_rsp || q8_rerror
                || q8_rdata !== {16'h3555, 256'h0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef})
            failures = failures + 1;

        @(negedge clk);
        session_begin_epoch = 32'h1234_0002;
        session_begin = 1;
        @(negedge clk);
        session_begin = 0;
        if (!session_begin_done || session_begin_error || epoch == first_epoch)
            failures = failures + 1;

        // A new-session partial write invalidates the destination and cannot
        // expose either old-session payload or the new partial payload.
        q30_tepoch = epoch;
        q30_tbegin = 1;
        @(negedge clk);
        q30_tbegin = 0;
        q30_wepoch = epoch;
        q30_windex = 10'd0;
        q30_wdata = 64'sd999;
        q30_wvalid = 1;
        @(negedge clk);
        q30_wvalid = 0;
        q30_rindex = 10'd0;
        pulse_q30_read(epoch);
        if (q30_rsp || !q30_rerror || q30_tcommit_ready)
            failures = failures + 1;

        // Reusing the active epoch and epoch zero are both rejected without
        // changing the installed session.
        session_begin_epoch = epoch;
        session_begin = 1;
        @(negedge clk);
        session_begin = 0;
        if (!session_begin_done || !session_begin_error
                || epoch != 32'h1234_0002)
            failures = failures + 1;
        session_begin_epoch = 32'd0;
        session_begin = 1;
        @(negedge clk);
        session_begin = 0;
        if (!session_begin_done || !session_begin_error
                || epoch != 32'h1234_0002)
            failures = failures + 1;

        pulse_q30_read(first_epoch);
        if (q30_rsp || !q30_rerror)
            failures = failures + 1;
        pulse_q8_read(first_epoch);
        if (q8_rsp || !q8_rerror)
            failures = failures + 1;

        // A stale writer cannot resurrect a logically cleared slot.
        @(negedge clk);
        q30_wepoch = first_epoch;
        q30_wvalid = 1;
        if (q30_wready)
            failures = failures + 1;
        @(negedge clk);
        q30_wvalid = 0;

        if (failures == 0)
            $display("PASS lfm25_resident_tensor_store typed=q30+q8 epoch=stale-reject payload_reset=logical");
        else
            $display("FAIL lfm25_resident_tensor_store failures=%0d", failures);
        $finish;
    end
endmodule
