L0:
/* [0000]  */ (W)     or (1|M0)                sr0.2<1>:ud   sr0.2<0;1,0>:ud   0xFFFFFF00:ud              {@1}
/* [0010]  */ (W)     mov (8|M0)               r125.0<1>:ud  r0.0<1;1,0>:ud                   {@1}
/* [0020]  */ (W)     mov (8|M0)               r126.0<1>:f   r125.0<8;8,1>:f                  {Compacted,@1}
/* [0028]  */ (W)     mov (1|M0)               r127.3<1>:f   0.0:f                               {Compacted}
/* [0030]  */ (W)     mov (1|M0)               r127.2<1>:f   0.0:f                               {Compacted}
/* [0038]  */ (W)     and (1|M0)               r4.0<1>:ud    r0.5<0;1,0>:ud    0x3FF:ud              {Compacted}
/* [0040]  */ (W)     or (1|M0)                cr0.0<1>:ud   cr0.0<0;1,0>:ud   0x4C0:uw              {@1}
/* [0050]  */ (W)     mul (1|M0)               r4.0<1>:ud    r4.0<0;1,0>:ud    0x84B0:uw              {@1}
/* [0060]  */ (W)     mov (1|M0)               r4.1<2>:f     0.0:f                              
/* [0070]  */ (W)     addc (1|M0)              r4.0<1>:ud    r6.0<0;1,0>:ud    r4.0<0;1,0>:ud   {AccWrEn,@2}
/* [0080]  */ (W)     mov (2|M0)               r126.0<1>:f   r6.6<2;2,1>:f                    {Compacted}
/* [0088]  */ (W)     mov (1|M0)               r127.6<1>:f   r4.0<0;1,0>:f                    {Compacted,@2}
/* [0090]  */ (W)     add (1|M0)               r4.0<1>:ud    acc0.0<0;1,0>:ud  r6.1<0;1,0>:ud  
/* [00A0]  */         mov (8|M0)               r2.0<1>:ud    r2.0<8;8,1>:uw                  
/* [00B0]  */ (W)     mov (1|M0)               r127.7<1>:f   r4.0<0;1,0>:f                    {Compacted,@2}
/* [00B8]  */ (W)     addc (1|M0)              r4.0<1>:ud    r127.6<0;1,0>:ud  0x40:ud              {AccWrEn,@4}
/* [00C8]  */ (W)     mov (2|M0)               r127.4<1>:f   r127.6<2;2,1>:f                  {@2}
/* [00D8]  */ (W)     mov (1|M0)               r127.6<1>:f   r4.0<0;1,0>:f                    {Compacted,@2}
/* [00E0]  */ (W)     add (1|M0)               r4.0<1>:ud    acc0.0<0;1,0>:ud  r127.7<0;1,0>:ud
/* [00F0]  */ (W)     mov (2|M0)               r7.0<1>:f     r127.4<2;2,1>:f                  {Compacted,@3}
/* [00F8]  */ (W)     mov (1|M0)               r127.7<1>:f   r4.0<0;1,0>:f                    {Compacted,@2}
/* [0100]  */ (W)     mov (1|M0)               r4.0<2>:f     0.0:f                              
/* [0110]  */ (W)     mov (2|M0)               r8.0<1>:f     r4.0<2;2,1>:f                    {Compacted,@1}
/* [0118]  */ (W)     mov (1|M0)               r4.0<1>:f     r7.0<0;1,0>:f                    {Compacted,@4}
/* [0120]  */ (W)     mov (1|M0)               r4.1<1>:f     r7.1<0;1,0>:f                    {Compacted}
/* [0128]  */ (W)     addc (1|M0)              r7.0<1>:ud    r127.4<0;1,0>:ud  0x10:ud              {AccWrEn}
/* [0138]  */ (W)     send.dc1 (8|M0)          null     r4      r8      0x40            0x020D40FF           {@2,$0} // wr:1h+1, rd:0; a64 unaligned oword block write x1
/* [0148]  */         mov (8|M0)               r4.0<1>:w     0x76543210:v                               {$0.src}
/* [0158]  */         mul (8|M0)               r6.0<1>:d     r4.0<8;8,1>:w     6:w               {@1}
/* [0168]  */ (W)     add (1|M0)               r4.0<1>:ud    acc0.0<0;1,0>:ud  r127.5<0;1,0>:ud
/* [0178]  */         addc (8|M0)              r9.0<1>:ud    r7.0<0;1,0>:ud    r6.0<8;8,1>:ud   {AccWrEn,Compacted,@2}
/* [0180]  */ (W)     mov (1|M0)               r7.1<1>:f     r4.0<0;1,0>:f                    {Compacted,@2}
/* [0188]  */         asr (8|M0)               r4.0<1>:ud    r6.0<8;8,1>:d     0x1F:uw             
/* [0198]  */         mov (8|M0)               r8.0<1>:ud    acc0.0<8;8,1>:ud                 {Compacted}
/* [01A0]  */         add (8|M0)               r4.0<1>:ud    r7.1<0;1,0>:ud    r4.0<8;8,1>:ud   {Compacted,@2}
/* [01A8]  */         add (8|M0)               r6.1<2>:ud    r8.0<8;8,1>:ud    r4.0<8;8,1>:ud   {Compacted,@1}
/* [01B0]  */         mov (8|M0)               r6.0<2>:f     r9.0<8;8,1>:f                    {Compacted,@6}
/* [01B8]  */         mov (8|M0)               r4.0<1>:ud    r1.0<8;8,1>:uw                  
/* [01C8]  */         addc (8|M0)              r10.0<1>:ud   r6.0<2;1,0>:ud    0x2:ud              {AccWrEn,Compacted,@2}
/* [01D0]  */         send.dc1 (8|M0)          null     r6      r4      0x40            0x040684FF           {@2,$1} // wr:2+1, rd:0; a64 byte scattering write 16b
/* [01E0]  */         mov (8|M0)               r4.0<1>:ud    acc0.0<8;8,1>:ud                 {Compacted,$1.src}
/* [01E8]  */ (W)     mov (1|M0)               r126.6<2>:f   r6.0<0;1,0>:f                    {Compacted}
/* [01F0]  */         add (8|M0)               r8.1<2>:ud    r4.0<8;8,1>:ud    r6.1<2;1,0>:ud   {@2}
/* [0200]  */         mov (8|M0)               r8.0<2>:f     r10.0<8;8,1>:f                   {Compacted,@4}
/* [0208]  */         addc (8|M0)              r4.0<1>:ud    r6.0<2;1,0>:ud    0x4:ud              {AccWrEn,Compacted}
/* [0210]  */         send.dc1 (8|M0)          null     r8      r2      0x40            0x040684FF           {@2,$2} // wr:2+1, rd:0; a64 byte scattering write 16b
/* [0220]  */         mov (8|M0)               r2.0<1>:ud    acc0.0<8;8,1>:ud                 {Compacted,$2.src}
/* [0228]  */ (W)     mov (1|M0)               r126.7<2>:f   r6.1<0;1,0>:f                   
/* [0238]  */         add (8|M0)               r8.1<2>:ud    r2.0<8;8,1>:ud    r6.1<2;1,0>:ud   {@2}
/* [0248]  */         mov (8|M0)               r8.0<2>:f     r4.0<8;8,1>:f                    {Compacted,@4}
/* [0250]  */         mov (8|M0)               r2.0<1>:ud    r3.0<8;8,1>:uw                  
/* [0260]  */         send.dc1 (8|M0)          null     r8      r2      0x40            0x040684FF           {@1,$3} // wr:2+1, rd:0; a64 byte scattering write 16b
/* [0270]  */ (W)     add (2|M0)               r2.0<1>:d     r5.2<2;2,1>:d     -513:w               {Compacted,$3.src}
/* [0278]  */ (W)     cmp (8|M0)    (lt)f0.0   null<1>:ud    r2.1<0;1,0>:ud    0xFFFFFE00:ud              {@1}
/* [0288]  */ (W&~f0.0) cmp (8|M0)  (lt)f0.0   null<1>:ud    r2.0<0;1,0>:ud    0xFFFFFE00:ud             
/* [0298]  */ (W&~f0.0) cmp (8|M0)  (ne)f0.0   null<1>:d     r125.1<0;1,0>:d   0:w              
/* [02A8]  */ (W)     csel (4|M0)   (eq)f0.0   r1.0<1>:w     r1.0<4;1>:w       r1.0<4;1>:w       r1.0<1>:w       
/* [02B8]  */ (~f0.0) if (8|M0)                            L848                  L848                
L712:
/* [02C8]  */         mov (8|M0)               r26.0<2>:f    r5.0<0;1,0>:f                    {Compacted}
/* [02D0]  */         mov (8|M0)               r28.0<1>:f    r5.2<0;1,0>:f                    {Compacted}
/* [02D8]  */         mov (8|M0)               r29.0<1>:f    0.0:f                               {Compacted}
/* [02E0]  */         mov (8|M0)               r30.0<1>:f    0.0:f                               {Compacted}
/* [02E8]  */         mov (8|M0)               r31.0<1>:f    r5.2<0;1,0>:f                    {Compacted}
/* [02F0]  */         mov (8|M0)               r32.0<1>:f    r5.3<0;1,0>:f                    {Compacted}
/* [02F8]  */         mov (8|M0)               r33.0<1>:f    r5.4<0;1,0>:f                    {Compacted}
/* [0300]  */         mov (8|M0)               r34.0<1>:f    r5.5<0;1,0>:f                    {Compacted}
/* [0308]  */         mov (8|M0)               r35.0<1>:f    r5.6<0;1,0>:f                    {Compacted}
/* [0310]  */         mov (8|M0)               r36.0<1>:f    r5.7<0;1,0>:f                    {Compacted}
/* [0318]  */         mov (8|M0)               r26.1<2>:f    r5.1<0;1,0>:f                    {Compacted}
/* [0320]  */         sync.allrd                           null                             {@1}
/* [0330]  */         sync.allwr                           null                            
/* [0340]  */         call (8|M0)              r127.0        L1048                                 {@1}
L848:
/* [0350]  */         endif (8|M0)                         L864                                
L864:
/* [0360]  */ (W)     mov (8|M0)               r112.0<1>:f   r125.0<8;8,1>:f                  {Compacted}
/* [0368]  */ (W)     send.dc0 (8|M0)          r2       r125    null    0x0            0x0219E000           {$4} // wr:1h+0, rd:1; synchronized global fence flushing
/* [0378]  */ (W)     mov (8|M0)               null<1>:ud    r2.0<8;8,1>:ud                   {$4.dst}
/* [0388]  */ (W)     send.dc0 (8|M0)          r2       r125    null    0x0            0x0219E0FE           {@1,$5} // wr:1h+0, rd:1; synchronized SLM fence
/* [0398]  */ (W)     mov (8|M0)               null<1>:ud    r2.0<8;8,1>:ud                   {$5.dst}
/* [03A8]  */ (W)     mov (16|M0)              acc0.0<1>:f   0.0:f                              
/* [03B8]  */ (W)     send.ts (1|M0)           null     r112    null    0x0            0x02000010           {EOT,@1} // wr:1+0, rd:0; end of thread
L968:
/* [03C8]  */         nop                    
/* [03D8]  */ (W)     mov (16|M0)              null<1>:ud    0xB94A1D17:ud                             
/* [03E8]  */ (W)     mov (16|M0)              null<1>:ud    0x474A5F47:ud                             
/* [03F8]  */ (W)     mov (16|M0)              null<1>:ud    0x0:ud                             
/* [0408]  */ (W)     mov (16|M0)              null<1>:ud    0x1:ud                             
L1048:
/* [0418]  */ (W)     add (1|M16)              r126.2<1>:ud  r127.2<0;1,0>:ud  0x0:ud             
/* [0428]  */ (W)     send.dc0 (8|M16)         null     r126    r127    0x40            0x020A02FF           {@1,$0} // wr:1h+1, rd:0; oword block write x2
/* [0438]  */ (W)     mov (1|M16)              r127.3<1>:f   r127.2<0;1,0>:f                  {$0.src}
/* [0448]  */ (W)     add (1|M16)              r127.2<1>:ud  r127.2<0;1,0>:ud  0x24:ud             
/* [0458]  */ (W)     add (1|M16)              r126.2<1>:ud  r127.3<0;1,0>:ud  0x4:ud              {@2}
/* [0468]  */ (W)     send.dc0 (16|M16)        null     r126    r60     0x100            0x020A04FF           {@1,$1} // wr:1h+4, rd:0; oword block write x8
/* [0478]  */ (W)     mov (1|M0)               r60.0<1>:uw   f0.0<0;1,0>:uw                   {Compacted,$1.src}
/* [0480]  */ (W)     mov (1|M0)               f0.0<1>:uw    0x0:uw                             
/* [0490]  */         cmp (8|M0)    (eq)f0.0   null<1>:uw    r2.0<0;1,0>:uw    r2.0<0;1,0>:uw  
/* [04A0]  */ (W)     add (1|M16)              r126.2<1>:ud  r127.3<0;1,0>:ud  0xC:ud             
/* [04B0]  */         mov (8|M0)               r63.0<1>:f    r32.0<8;8,1>:f                   {Compacted}
/* [04B8]  */ (W)     send.dc0 (16|M16)        null     r126    r64     0x100            0x020A04FF           {@2,$2} // wr:1h+4, rd:0; oword block write x8
/* [04C8]  */ (W&f0.0.any8h) mov (2|M0)        r60.2<1>:f    r127.4<2;2,1>:f                 
/* [04D8]  */ (W&f0.0.any8h) addc (1|M0)       r2.0<1>:ud    r127.6<0;1,0>:ud  0x10:ud              {AccWrEn}
/* [04E8]  */ (W&f0.0.any8h) mov (2|M0)        r127.4<1>:f   r127.6<2;2,1>:f                 
/* [04F8]  */ (W)     add (1|M16)              r126.2<1>:ud  r127.3<0;1,0>:ud  0x14:ud              {$2.src}
/* [0508]  */ (W&f0.0.any8h) mov (1|M0)        r127.6<1>:f   r2.0<0;1,0>:f                    {@3}
/* [0518]  */ (W&f0.0.any8h) mov (2|M0)        r3.0<1>:f     r127.4<2;2,1>:f                  {@3}
/* [0528]  */ (W&f0.0.any8h) add (1|M0)        r2.0<1>:ud    acc0.0<0;1,0>:ud  r127.7<0;1,0>:ud
/* [0538]  */ (W)     send.dc0 (16|M16)        null     r126    r68     0x100            0x020A04FF           {@4,$3} // wr:1h+4, rd:0; oword block write x8
/* [0548]  */ (W&f0.0.any8h) mov (2|M0)        r4.0<1>:f     r60.2<2;2,1>:f                   {@7}
/* [0558]  */ (W&f0.0.any8h) mov (1|M0)        r2.1<1>:f     r3.1<0;1,0>:f                    {@3}
/* [0568]  */ (W&f0.0.any8h) mov (1|M0)        r127.7<1>:f   r2.0<0;1,0>:f                    {@3}
/* [0578]  */ (W)     add (1|M16)              r126.2<1>:ud  r127.3<0;1,0>:ud  0x1C:ud              {$3.src}
/* [0588]  */ (W&f0.0.any8h) mov (1|M0)        r2.0<1>:f     r3.0<0;1,0>:f                   
/* [0598]  */ (W)     send.dc0 (16|M16)        null     r126    r72     0x100            0x020A04FF           {@2,$4} // wr:1h+4, rd:0; oword block write x8
/* [05A8]  */ (W&f0.0.any8h) send.dc1 (8|M0)   null     r2      r4      0x40            0x020D40FF           {@1,$5} // wr:1h+1, rd:0; a64 unaligned oword block write x1
/* [05B8]  */ (W&f0.0.any8h) mov (2|M0)        r2.2<1>:f     r126.0<2;2,1>:f                  {$5.src}
/* [05C8]  */         mov (8|M0)               r61.0<1>:f    r31.0<8;8,1>:f                   {Compacted}
/* [05D0]  */ (W&f0.0.any8h) addc (1|M0)       r3.0<1>:ud    r2.2<0;1,0>:ud    0x4:ud              {AccWrEn,@2}
/* [05E0]  */         mov (8|M0)               r62.0<1>:f    r36.0<8;8,1>:f                   {Compacted}
/* [05E8]  */ (W&f0.0.any8h) mov (1|M0)        r2.0<1>:ud    acc0.0<0;1,0>:ud                
/* [05F8]  */         mov (8|M0)               r64.0<2>:f    r26.0<2;1,0>:f                   {Compacted}
/* [0600]  */ (W&f0.0.any8h) add (1|M0)        r2.1<2>:ud    r2.0<0;1,0>:ud    r2.3<0;1,0>:ud   {@2}
/* [0610]  */ (W&f0.0.any8h) mov (1|M0)        r2.0<2>:f     r3.0<0;1,0>:f                    {@5}
/* [0620]  */ (W&f0.0.any8h) mov (2|M0)        r3.0<1>:f     r126.6<2;2,1>:f                 
/* [0630]  */         mov (8|M0)               r67.0<1>:f    r28.0<8;8,1>:f                   {Compacted}
/* [0638]  */         sync.nop                             null                             {Compacted,$4.src}
/* [0640]  */         mov (8|M0)               r72.0<1>:f    r3.1<0;1,0>:f                    {Compacted,@2}
/* [0648]  */         mov (8|M0)               r66.0<1>:f    r35.0<8;8,1>:f                   {Compacted}
/* [0650]  */         mov (8|M0)               r69.0<1>:f    r29.0<8;8,1>:f                   {Compacted}
/* [0658]  */         mov (8|M0)               r68.0<1>:f    r30.0<8;8,1>:f                   {Compacted}
/* [0660]  */         mov (8|M0)               r71.0<1>:f    r33.0<8;8,1>:f                   {Compacted}
/* [0668]  */         mov (8|M0)               r70.0<1>:f    r34.0<8;8,1>:f                   {Compacted}
/* [0670]  */         mov (8|M0)               r64.1<2>:f    r26.1<2;1,0>:f                   {Compacted}
/* [0678]  */ (W)     csel (4|M0)   (eq)f0.0   r1.0<1>:w     r1.0<4;1>:w       r1.0<4;1>:w       r1.0<1>:w       
/* [0688]  */ (W&f0.0.any8h) send.dc1 (1|M0)   r74      r2      null    0x0            0x041401FF           {@7,$6} // wr:2+0, rd:1; a64 dword gathering read
/* [0698]  */         mov (8|M0)               r2.0<1>:w     0x76543210:v                               {$6.src}
/* [06A8]  */ (W)     mov (1|M0)               f0.0<1>:uw    r60.0<0;1,0>:uw                 
/* [06B8]  */         cmp (8|M0)    (lt)f0.0   null<1>:ud    r63.0<8;8,1>:ud   0x41:uw             
/* [06C8]  */         mul (8|M0)               r2.0<1>:w     r2.0<8;8,1>:w     6:w               {@3}
/* [06D8]  */         mov (8|M0)               r2.0<1>:d     r2.0<8;8,1>:uw                   {@1}
/* [06E8]  */ (f0.0)  cmp (8|M0)    (lt)f0.0   null<1>:ud    r61.0<8;8,1>:ud   0x41:uw             
/* [06F8]  */         addc (8|M0)              r73.0<1>:ud   r2.0<8;8,1>:ud    r3.0<0;1,0>:ud   {AccWrEn,Compacted,@2}
/* [0700]  */         add (8|M0)               r72.0<1>:ud   acc0.0<8;8,1>:ud  r72.0<8;8,1>:ud 
/* [0710]  */ (W)     csel (4|M0)   (eq)f0.0   r1.0<1>:w     r1.0<4;1>:w       r1.0<4;1>:w       r1.0<1>:w        {@7}
/* [0720]  */ (~f0.0) if (8|M0)                            L2320                  L3288                
L1840:
/* [0730]  */         cmp (8|M0)    (lt)f0.1   null<1>:ud    r61.0<8;8,1>:ud   r63.0<8;8,1>:ud 
/* [0740]  */ (W)     csel (4|M0)   (eq)f0.0   r1.0<1>:w     r1.0<4;1>:w       r1.0<4;1>:w       r1.0<1>:w        {@3}
/* [0750]  */ (~f0.1) if (8|M0)                            L1960                  L2016                
L1888:
/* [0760]  */         shr (8|M0)               r2.0<1>:ud    r61.0<8;8,1>:ud   1:w              
/* [0770]  */         mov (8|M0)               r73.0<1>:f    r63.0<8;8,1>:f                   {Compacted}
/* [0778]  */         add (8|M0)               r61.0<1>:d    r61.0<8;8,1>:d    -r2.0<8;8,1>:d   {Compacted,@2}
/* [0780]  */         add (8|M0)               r69.0<1>:d    r69.0<8;8,1>:d    r2.0<8;8,1>:d    {Compacted}
/* [0788]  */         mov (8|M0)               r72.0<1>:f    r61.0<8;8,1>:f                   {Compacted,@2}
/* [0790]  */         mov (8|M0)               r61.0<1>:f    r2.0<8;8,1>:f                    {Compacted}
/* [0798]  */         else (8|M0)                          L2016                  L2016                
L1960:
/* [07A8]  */         shr (8|M0)               r2.0<1>:ud    r63.0<8;8,1>:ud   1:w              
/* [07B8]  */         mov (8|M0)               r72.0<1>:f    r61.0<8;8,1>:f                   {Compacted,@3}
/* [07C0]  */         add (8|M0)               r63.0<1>:d    r63.0<8;8,1>:d    -r2.0<8;8,1>:d   {Compacted,@2}
/* [07C8]  */         add (8|M0)               r68.0<1>:d    r68.0<8;8,1>:d    r2.0<8;8,1>:d    {Compacted}
/* [07D0]  */         mov (8|M0)               r73.0<1>:f    r63.0<8;8,1>:f                   {Compacted,@2}
/* [07D8]  */         mov (8|M0)               r63.0<1>:f    r2.0<8;8,1>:f                    {Compacted}
L2016:
/* [07E0]  */         endif (8|M0)                         L2032                                
L2032:
/* [07F0]  */         mov (8|M0)               r31.0<1>:f    r61.0<8;8,1>:f                   {Compacted}
/* [07F8]  */         mov (8|M0)               r32.0<1>:f    r63.0<8;8,1>:f                   {Compacted,@3}
/* [0800]  */         mov (8|M0)               r26.0<2>:f    r64.0<2;1,0>:f                   {Compacted}
/* [0808]  */         mov (8|M0)               r28.0<1>:f    r67.0<8;8,1>:f                   {Compacted}
/* [0810]  */         mov (8|M0)               r29.0<1>:f    r2.0<8;8,1>:f                    {Compacted}
/* [0818]  */         mov (8|M0)               r30.0<1>:f    r3.0<8;8,1>:f                    {Compacted}
/* [0820]  */         mov (8|M0)               r33.0<1>:f    r71.0<8;8,1>:f                   {Compacted}
/* [0828]  */         mov (8|M0)               r34.0<1>:f    r70.0<8;8,1>:f                   {Compacted}
/* [0830]  */         mov (8|M0)               r35.0<1>:f    r66.0<8;8,1>:f                   {Compacted}
/* [0838]  */         mov (8|M0)               r36.0<1>:f    r62.0<8;8,1>:f                   {Compacted}
/* [0840]  */         mov (8|M0)               r26.1<2>:f    r64.1<2;1,0>:f                   {Compacted}
/* [0848]  */         sync.allrd                           null                             {@1}
/* [0858]  */         sync.allwr                           null                            
/* [0868]  */         call (8|M0)              r127.0        L1048                                 {@1}
L2168:
/* [0878]  */         mov (8|M0)               r29.0<1>:f    r69.0<8;8,1>:f                   {Compacted}
/* [0880]  */         mov (8|M0)               r30.0<1>:f    r68.0<8;8,1>:f                   {Compacted}
/* [0888]  */         mov (8|M0)               r31.0<1>:f    r72.0<8;8,1>:f                   {Compacted}
/* [0890]  */         mov (8|M0)               r32.0<1>:f    r73.0<8;8,1>:f                   {Compacted}
/* [0898]  */         mov (8|M0)               r26.0<2>:f    r64.0<2;1,0>:f                   {Compacted}
/* [08A0]  */         mov (8|M0)               r28.0<1>:f    r67.0<8;8,1>:f                   {Compacted}
/* [08A8]  */         mov (8|M0)               r33.0<1>:f    r71.0<8;8,1>:f                   {Compacted}
/* [08B0]  */         mov (8|M0)               r34.0<1>:f    r70.0<8;8,1>:f                   {Compacted}
/* [08B8]  */         mov (8|M0)               r35.0<1>:f    r66.0<8;8,1>:f                   {Compacted}
/* [08C0]  */         mov (8|M0)               r36.0<1>:f    r62.0<8;8,1>:f                   {Compacted}
/* [08C8]  */         mov (8|M0)               r26.1<2>:f    r64.1<2;1,0>:f                   {Compacted}
/* [08D0]  */         sync.allrd                           null                             {@1}
/* [08E0]  */         sync.allwr                           null                            
/* [08F0]  */         call (8|M0)              r127.0        L1048                                 {@1}
L2304:
/* [0900]  */         else (8|M0)                          L3288                  L3288                
L2320:
/* [0910]  */         mov (8|M0)               r2.0<2>:f     r73.0<8;8,1>:f                   {Compacted}
/* [0918]  */         mov (8|M0)               r2.1<2>:f     r72.0<8;8,1>:f                   {Compacted}
/* [0920]  */ (W)     mov (1|M0)               r60.0<1>:uw   f0.0<0;1,0>:uw                   {Compacted}
/* [0928]  */         send.dc1 (8|M0)          r4       r2      null    0x0            0x041404FF           {@2,$7} // wr:2+0, rd:1; a64 byte gathering read 16b
/* [0938]  */ (W)     mov (1|M0)               f0.0<1>:uw    0x0:uw                             
/* [0948]  */         cmp (8|M0)    (eq)f0.0   null<1>:uw    r2.0<0;1,0>:uw    r2.0<0;1,0>:uw  
/* [0958]  */ (W&f0.0.any8h) mul (8|M0)        acc0.0<1>:d   r61.0<8;8,1>:d    r63.0<2;1,0>:uw 
/* [0968]  */         mach (8|M0)              r13.0<1>:d    r61.0<8;8,1>:d    r63.0<8;8,1>:d   {Compacted}
/* [0970]  */ (W)     mov (1|M0)               f0.0<1>:uw    r60.0<0;1,0>:uw                  {@5}
/* [0980]  */         mov (8|M0)               r7.0<1>:d     r4.0<2;1,0>:uw                   {$7.dst}
/* [0990]  */         cmp (8|M0)    (gt)f0.0   null<1>:ud    r13.0<8;8,1>:ud   r7.0<8;8,1>:ud   {@1}
/* [09A0]  */ (W)     csel (4|M0)   (eq)f0.0   r1.0<1>:w     r1.0<4;1>:w       r1.0<4;1>:w       r1.0<1>:w       
/* [09B0]  */ (f0.0)  if (8|M0)                            L3272                  L3272                
L2496:
/* [09C0]  */         cmp (8|M0)    (eq)f0.1   null<1>:d     r62.0<8;8,1>:d    0:w              
/* [09D0]  */         mov (8|M0)               r15.0<1>:f    r64.0<2;1,0>:f                   {Compacted}
/* [09D8]  */         mov (8|M0)               r14.0<1>:f    r64.1<2;1,0>:f                   {Compacted}
L2528:
/* [09E0]  */         sync.nop                             null                             {Compacted,$9.src}
/* [09E8]  */         math.irem (8|M0)         r2.0<1>:ud    r7.0<8;8,1>:ud    r61.0<8;8,1>:ud  {@7,$8}
/* [09F8]  */         add (8|M0)               r12.0<1>:d    r69.0<8;8,1>:d    r2.0<8;8,1>:d    {Compacted,$8.dst}
/* [0A00]  */         math.iqot (8|M0)         r2.0<1>:ud    r7.0<8;8,1>:ud    r61.0<8;8,1>:ud  {@1,$10}
/* [0A10]  */         add (8|M0)               r10.0<1>:d    r68.0<8;8,1>:d    r2.0<8;8,1>:d    {Compacted,$10.dst}
/* [0A18]  */ (W)     csel (4|M0)   (eq)f0.0   r1.0<1>:w     r1.0<4;1>:w       r1.0<4;1>:w       r1.0<1>:w        {@7}
/* [0A28]  */ (f0.1)  if (8|M0)                            L2640                  L3040                
L2616:
/* [0A38]  */         mov (8|M0)               r6.0<1>:f     0.0:f                               {Compacted}
/* [0A40]  */         else (8|M0)                          L3040                  L3040                
L2640:
/* [0A50]  */         mov (8|M0)               acc0.0<1>:f   r12.0<8;8,1>:ud                  {@6}
/* [0A60]  */ (W)     mov (1|M0)               r60.0<1>:uw   f0.0<0;1,0>:uw                   {Compacted}
/* [0A68]  */         mul (8|M0)               acc0.0<1>:f   acc0.0<8;8,1>:f   r66.0<8;8,1>:f   {Compacted}
/* [0A70]  */ (W)     mov (1|M0)               f0.0<1>:uw    0x0:uw                             
/* [0A80]  */         add (8|M0)               r9.0<1>:f     acc0.0<8;8,1>:f   r71.0<8;8,1>:f   {Compacted}
/* [0A88]  */         mov (8|M0)               acc0.0<1>:f   r10.0<8;8,1>:ud                  {@7}
/* [0A98]  */         mov (8|M0)               r5.0<1>:f     0.0:f                               {Compacted}
/* [0AA0]  */         mul (8|M0)               acc0.0<1>:f   acc0.0<8;8,1>:f   r66.0<8;8,1>:f   {Compacted}
/* [0AA8]  */         mov (8|M0)               r3.0<1>:f     0.0:f                               {Compacted}
/* [0AB0]  */         cmp (8|M0)    (eq)f0.0   null<1>:uw    r2.0<0;1,0>:uw    r2.0<0;1,0>:uw  
/* [0AC0]  */         mov (8|M0)               r6.0<1>:f     0.0:f                               {Compacted}
/* [0AC8]  */         add (8|M0)               r8.0<1>:f     acc0.0<8;8,1>:f   r70.0<8;8,1>:f   {Compacted}
/* [0AD0]  */         mov (8|M0)               r2.0<1>:f     0.0:f                               {Compacted}
/* [0AD8]  */ (W&f0.0.any8h) mov (1|M0)        r11.0<1>:f    0.0:f                              
/* [0AE8]  */ (W)     mov (1|M0)               f0.0<1>:uw    r60.0<0;1,0>:uw                 
L2808:
/* [0AF8]  */         add (8|M0)               acc0.0<1>:f   r3.0<8;8,1>:f     -r5.0<8;8,1>:f   {Compacted,@7}
/* [0B00]  */ (W)     mov (1|M0)               r60.0<1>:uw   f0.0<0;1,0>:uw                   {Compacted}
/* [0B08]  */         add (8|M0)               r4.0<1>:f     acc0.0<8;8,1>:f   r9.0<8;8,1>:f    {Compacted}
/* [0B10]  */         mul (8|M0)               acc0.0<1>:f   r6.0<8;8,1>:f     2.0:f               {Compacted,@7}
/* [0B18]  */ (W)     mov (1|M0)               f0.0<1>:uw    0x0:uw                             
/* [0B28]  */         mul (8|M0)               acc0.0<1>:f   acc0.0<8;8,1>:f   r2.0<8;8,1>:f    {Compacted,@7}
/* [0B30]  */         add (8|M0)               r2.0<1>:f     acc0.0<8;8,1>:f   r8.0<8;8,1>:f    {Compacted,@7}
/* [0B38]  */         mul (8|M0)               r3.0<1>:f     r4.0<8;8,1>:f     r4.0<8;8,1>:f    {Compacted,@5}
/* [0B40]  */         mul (8|M0)               r5.0<1>:f     r2.0<8;8,1>:f     r2.0<8;8,1>:f    {Compacted,@2}
/* [0B48]  */         cmp (8|M0)    (eq)f0.0   null<1>:uw    r2.0<0;1,0>:uw    r2.0<0;1,0>:uw  
/* [0B58]  */         add (8|M0)               acc0.0<1>:f   r3.0<8;8,1>:f     r5.0<8;8,1>:f    {Compacted,@2}
/* [0B60]  */         cmp (8|M0)    (le)f1.0   null<1>:f     acc0.0<8;8,1>:f   4.0:f              
/* [0B70]  */ (W&f0.0.any8h) add (1|M0)        r11.0<1>:d    r11.0<0;1,0>:d    1:w              
/* [0B80]  */ (W)     mov (1|M0)               f0.0<1>:uw    r60.0<0;1,0>:uw                 
/* [0B90]  */         mov (8|M0)               r6.0<1>:f     r11.0<0;1,0>:f                   {Compacted,@2}
/* [0B98]  */ (f1.0)  cmp (8|M0)    (lt)f1.0   null<1>:ud    r11.0<0;1,0>:ud   r62.0<8;8,1>:ud 
/* [0BA8]  */ (W)     csel (4|M0)   (eq)f0.0   r1.0<1>:w     r1.0<4;1>:w       r1.0<4;1>:w       r1.0<1>:w       
/* [0BB8]  */ (~f1.0) break (8|M0)                         L3024                  L3024                
L3016:
/* [0BC8]  */         mov (8|M0)               r6.0<1>:f     r4.0<8;8,1>:f                    {Compacted}
L3024:
/* [0BD0]  */         while (8|M0)                         L2808                                
L3040:
/* [0BE0]  */         endif (8|M0)                         L3056                                
L3056:
/* [0BF0]  */ (W)     mov (1|M0)               r60.0<1>:uw   f0.0<0;1,0>:uw                   {Compacted}
/* [0BF8]  */ (W)     mov (1|M0)               f0.0<1>:uw    0x0:uw                             
/* [0C08]  */         cmp (8|M0)    (eq)f0.0   null<1>:uw    r2.0<0;1,0>:uw    r2.0<0;1,0>:uw  
/* [0C18]  */         add (8|M0)               r7.0<1>:d     r7.0<8;8,1>:d     r74.0<0;1,0>:d   {Compacted,$6.dst}
/* [0C20]  */         cmp (8|M0)    (lt)f1.1   null<1>:ud    r7.0<8;8,1>:ud    r13.0<8;8,1>:ud  {@1}
/* [0C30]  */ (W&f0.0.any8h) mul (8|M0)        acc0.0<1>:d   r10.0<8;8,1>:d    r67.0<2;1,0>:uw 
/* [0C40]  */         mach (8|M0)              r2.0<1>:d     r10.0<8;8,1>:d    r67.0<8;8,1>:d   {Compacted}
/* [0C48]  */ (W)     mov (1|M0)               f0.0<1>:uw    r60.0<0;1,0>:uw                  {@7}
/* [0C58]  */         add (8|M0)               acc0.0<1>:d   r2.0<8;8,1>:d     r12.0<8;8,1>:d   {@2}
/* [0C68]  */         shl (8|M0)               r2.0<1>:d     acc0.0<8;8,1>:d   2:w              
/* [0C78]  */         addc (8|M0)              r5.0<1>:ud    r15.0<8;8,1>:ud   r2.0<8;8,1>:ud   {AccWrEn,Compacted,@1}
/* [0C80]  */         mov (8|M0)               r4.0<1>:ud    acc0.0<8;8,1>:ud                 {Compacted}
/* [0C88]  */         add (8|M0)               r2.1<2>:ud    r4.0<8;8,1>:ud    r14.0<8;8,1>:ud  {Compacted,@1}
/* [0C90]  */         mov (8|M0)               r2.0<2>:f     r5.0<8;8,1>:f                    {Compacted,@3}
/* [0C98]  */         send.dc1 (8|M0)          null     r2      r6      0x40            0x040681FF           {@1,$9} // wr:2+1, rd:0; a64 dword scattering write
/* [0CA8]  */ (W)     csel (4|M0)   (eq)f0.0   r1.0<1>:w     r1.0<4;1>:w       r1.0<4;1>:w       r1.0<1>:w       
/* [0CB8]  */ (f1.1)  while (8|M0)                         L2528                                
L3272:
/* [0CC8]  */         endif (8|M0)                         L3288                                
L3288:
/* [0CD8]  */         endif (8|M0)                         L3304                                
L3304:
/* [0CE8]  */ (W)     mov (1|M0)               r60.0<1>:uw   f0.0<0;1,0>:uw                   {Compacted}
/* [0CF0]  */ (W)     mov (1|M0)               f0.0<1>:uw    0x0:uw                             
/* [0D00]  */         cmp (8|M0)    (eq)f0.0   null<1>:uw    r2.0<0;1,0>:uw    r2.0<0;1,0>:uw  
/* [0D10]  */ (W)     add (1|M16)              r126.2<1>:ud  r127.3<0;1,0>:ud  0x4:ud             
/* [0D20]  */ (W&f0.0.any8h) mov (2|M0)        r127.6<1>:f   r127.4<2;2,1>:f                 
/* [0D30]  */ (W&f0.0.any8h) mov (2|M0)        r127.4<1>:f   r60.2<2;2,1>:f                  
/* [0D40]  */ (W)     mov (1|M0)               f0.0<1>:uw    r60.0<0;1,0>:uw                  {@6}
/* [0D50]  */ (W)     send.dc0 (16|M16)        r60      r126    null    0x0            0x024804FF           {@1,$11} // wr:1h+0, rd:4; oword block read x8
/* [0D60]  */ (W)     add (1|M16)              r126.2<1>:ud  r127.3<0;1,0>:ud  0xC:ud              {$11.src}
/* [0D70]  */ (W)     send.dc0 (16|M16)        r64      r126    null    0x0            0x024804FF           {@1,$12} // wr:1h+0, rd:4; oword block read x8
/* [0D80]  */ (W)     add (1|M16)              r126.2<1>:ud  r127.3<0;1,0>:ud  0x14:ud              {$12.src}
/* [0D90]  */ (W)     send.dc0 (16|M16)        r68      r126    null    0x0            0x024804FF           {@1,$13} // wr:1h+0, rd:4; oword block read x8
/* [0DA0]  */ (W)     add (1|M16)              r126.2<1>:ud  r127.3<0;1,0>:ud  0x1C:ud              {$13.src}
/* [0DB0]  */ (W)     send.dc0 (16|M16)        r72      r126    null    0x0            0x024804FF           {@1,$6} // wr:1h+0, rd:4; oword block read x8
/* [0DC0]  */ (W)     add (1|M16)              r126.2<1>:ud  r127.3<0;1,0>:ud  0x0:ud              {$6.src}
/* [0DD0]  */ (W)     send.dc0 (8|M16)         r127     r126    null    0x0            0x021802FF           {@1,$14} // wr:1h+0, rd:1; oword block read x2
/* [0DE0]  */         sync.allrd                           null                             {@1}
/* [0DF0]  */         sync.allwr                           null                            
/* [0E00]  */         ret (8|M0)                           r127.0                           {@1,$14.dst}
L3600:
