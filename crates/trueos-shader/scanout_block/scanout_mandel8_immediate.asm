mov(8)          g1<1>UW         0x76543210V                     { align1 WE_all 1Q };
mov(8)          g1<1>D          g1<8,8,1>UW                     { align1 WE_all 1Q @2 };
mov(1)          g58.0<1>D       0D                              { align1 WE_all 1N };
mov(1)          g58.1<1>D       0D                              { align1 WE_all 1N };
mov(1)          g58.2<1>D       0D                              { align1 WE_all 1N };
mov(1)          g58.3<1>D       0D                              { align1 WE_all 1N };
mov(1)          g58.4<1>D       0D                              { align1 WE_all 1N };
mov(1)          g58.5<1>D       0D                              { align1 WE_all 1N };
mov(1)          g58.6<1>D       0D                              { align1 WE_all 1N };
mov(1)          g58.7<1>D       0D                              { align1 WE_all 1N };
shl(8)          g56<1>D         g1<8,8,1>D      0x00000002UD    { align1 1Q };
add(8)          g56<1>D         g56<8,8,1>D     0D              { align1 1Q @1 };
send(8)         nullUD          g56UD           g58UD           0x0c3802cc                0x0c3a9a00
                            hdc1 MsgDesc: (DC untyped surface write, Surface = 52, SIMD8, Mask = 0xe) mlen 1 ex_mlen 1 rlen 0 { align1 1Q @1 };
mov(8)          g127<1>UD       g0<8,8,1>UD                     { align1 WE_all 1Q };
send(8)         nullUD          g127UD          nullUD          0x02000000                0x00000000
                            ts/btd MsgDesc:  mlen 1 ex_mlen 0 rlen 0 { align1 WE_all 1Q @1 EOT };
