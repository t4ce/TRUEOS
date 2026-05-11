mov(16)         acc0.0<1>F      0F                                  { align1 WE_all 1H };
mov(8)          g127<1>UD       g0<8,8,1>UD                         { align1 WE_all 1Q };
send(8)         nullUD          g127UD          nullUD          0x02000000                0x00000000
                            ts/btd MsgDesc:  mlen 1 ex_mlen 0 rlen 0 { align1 WE_all 1Q @1 EOT };
