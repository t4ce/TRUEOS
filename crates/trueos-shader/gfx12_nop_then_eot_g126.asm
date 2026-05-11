sync nop(8)     null<0,1,0>UB                   { align1 WE_all 1Q @1 };
mov(8)          g126<1>UD       g0<8,8,1>UD     { align1 WE_all 1Q };
send(8)         nullUD          g126UD          nullUD          0x02000000                0x00000000
                            ts/btd MsgDesc:  mlen 1 ex_mlen 0 rlen 0 { align1 WE_all 1Q @1 EOT };
