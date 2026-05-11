mov(8)          g2<1>D          -1059162327D                    { align1 WE_all 1Q };
mov(8)          g6<1>D          16909060D                       { align1 WE_all 1Q };
mov(8)          g7<1>D          16843009D                       { align1 WE_all 1Q };
dp4a(8)         g4<1>D          g2<8,8,1>D     g6<8,8,1>D     g7<1,1,1>D { align1 1Q @1 };
mov(8)          g127<1>UD       0x00840058UD                    { align1 WE_all 1Q };
send(8)         nullUD          g127UD          g4UD            0x02026efd                0x00000040
                            hdc1 MsgDesc: (DC untyped surface write, Surface = 253, SIMD8, Mask = 0xe) mlen 1 ex_mlen 1 rlen 0 { align1 1Q @1 };
mov(8)          g127<1>UD       g0<8,8,1>UD                     { align1 WE_all 1Q };
send(8)         nullUD          g127UD          nullUD          0x02000000                0x00000000
                            ts/btd MsgDesc:  mlen 1 ex_mlen 0 rlen 0 { align1 WE_all 1Q @1 EOT };
