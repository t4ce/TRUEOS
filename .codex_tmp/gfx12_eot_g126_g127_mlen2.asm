mov(8)          g126<1>UD       g0<8,8,1>UD                     { align1 WE_all 1Q };
mov(8)          g127<1>UD       g1<8,8,1>UD                     { align1 WE_all 1Q };
send(8)         nullUD          g126UD          nullUD          0x04000000                0x00000000
                            ts/btd MsgDesc:  mlen 2 ex_mlen 0 rlen 0 { align1 WE_all 1Q @1 EOT };
