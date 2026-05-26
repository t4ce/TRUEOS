mov(16)         g6<1>D          3D                              { align1 WE_all 1H };
mul(16)         acc0<1>D        g6<8,8,1>D      g6<8,8,1>D      { align1 1H @1 };
mov(16)         g4<1>D          acc0<8,8,1>D                    { align1 1H @1 };
mov(1)          g127.1<1>D      acc0<0,1,0>D                    { align1 WE_all 1N @1 };
