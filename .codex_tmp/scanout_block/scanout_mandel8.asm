mov(8)          g1<1>UW         0x76543210V                     { align1 WE_all 1Q };
mov(8)          g1<1>D          g1<8,8,1>UW                     { align1 WE_all 1Q @2 };
add(8)          g2<1>D          g1<8,8,1>D      0D              { align1 1Q @1 };
mul(8)          g3<1>D          g2<8,8,1>D      11D             { align1 1Q @1 };
add(8)          g4<1>D          g3<8,8,1>D      0x00003080D     { align1 1Q @1 };
shl(8)          g5<1>D          g3<8,8,1>D      0x00000008UD    { align1 1Q };
or(8)           g4<1>D          g4<8,8,1>D      g5<8,8,1>D      { align1 1Q @1 };
shl(8)          g127<1>D        g1<8,8,1>D      0x00000002UD    { align1 1Q };
add(8)          g127<1>D        g127<8,8,1>D    0x00840058D     { align1 1Q @1 };
send(8)         nullUD          g127UD          g4UD            0x02026efd                0x00000040
                            hdc1 MsgDesc: (DC untyped surface write, Surface = 253, SIMD8, Mask = 0xe) mlen 1 ex_mlen 1 rlen 0 { align1 1Q @1 };
mov(8)          g127<1>UD       g0<8,8,1>UD                     { align1 WE_all 1Q };
send(8)         nullUD          g127UD          nullUD          0x02000000                0x00000000
                            ts/btd MsgDesc:  mlen 1 ex_mlen 0 rlen 0 { align1 WE_all 1Q @1 EOT };
