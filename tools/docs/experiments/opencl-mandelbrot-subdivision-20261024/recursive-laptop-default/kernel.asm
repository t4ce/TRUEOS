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
/* [03D8]  */ (W)     mov (16|M0)              null<1>:ud    0x78D91EA5:ud                             
/* [03E8]  */ (W)     mov (16|M0)              null<1>:ud    0xC133C194:ud                             
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
/* [05C8]  */         cmp (8|M0)    (lt)f1.0   null<1>:ud    r63.0<8;8,1>:ud   0x41:uw             
/* [05D8]  */ (W&f0.0.any8h) addc (1|M0)       r3.0<1>:ud    r2.2<0;1,0>:ud    0x4:ud              {AccWrEn,@2}
/* [05E8]  */         mov (8|M0)               r61.0<1>:f    r31.0<8;8,1>:f                   {Compacted}
/* [05F0]  */ (W&f0.0.any8h) mov (1|M0)        r2.0<1>:ud    acc0.0<0;1,0>:ud                
/* [0600]  */ (f1.0)  cmp (8|M0)    (lt)f1.0   null<1>:ud    r61.0<8;8,1>:ud   0x41:uw              {@2}
/* [0610]  */ (W&f0.0.any8h) add (1|M0)        r2.1<2>:ud    r2.0<0;1,0>:ud    r2.3<0;1,0>:ud   {@2}
/* [0620]  */ (W&f0.0.any8h) mov (1|M0)        r2.0<2>:f     r3.0<0;1,0>:f                    {@5}
/* [0630]  */ (W&f0.0.any8h) mov (2|M0)        r3.0<1>:f     r126.6<2;2,1>:f                 
/* [0640]  */         mov (8|M0)               r62.0<1>:f    r36.0<8;8,1>:f                   {Compacted}
/* [0648]  */         sync.nop                             null                             {Compacted,$4.src}
/* [0650]  */         mov (8|M0)               r72.0<1>:f    r3.1<0;1,0>:f                    {Compacted,@2}
/* [0658]  */         mov (8|M0)               r64.0<2>:f    r26.0<2;1,0>:f                   {Compacted}
/* [0660]  */         mov (8|M0)               r67.0<1>:f    r28.0<8;8,1>:f                   {Compacted}
/* [0668]  */         mov (8|M0)               r66.0<1>:f    r35.0<8;8,1>:f                   {Compacted}
/* [0670]  */         mov (8|M0)               r69.0<1>:f    r29.0<8;8,1>:f                   {Compacted}
/* [0678]  */         mov (8|M0)               r68.0<1>:f    r30.0<8;8,1>:f                   {Compacted}
/* [0680]  */         mov (8|M0)               r71.0<1>:f    r33.0<8;8,1>:f                   {Compacted}
/* [0688]  */         mov (8|M0)               r70.0<1>:f    r34.0<8;8,1>:f                   {Compacted}
/* [0690]  */         mov (8|M0)               r64.1<2>:f    r26.1<2;1,0>:f                   {Compacted}
/* [0698]  */ (W)     csel (4|M0)   (eq)f0.0   r1.0<1>:w     r1.0<4;1>:w       r1.0<4;1>:w       r1.0<1>:w       
/* [06A8]  */ (W&f0.0.any8h) send.dc1 (1|M0)   r74      r2      null    0x0            0x041401FF           {$6} // wr:2+0, rd:1; a64 dword gathering read
/* [06B8]  */         mov (8|M0)               r2.0<1>:w     0x76543210:v                               {$6.src}
/* [06C8]  */ (W)     mov (1|M0)               f0.0<1>:uw    r60.0<0;1,0>:uw                 
/* [06D8]  */         mul (8|M0)               r2.0<1>:w     r2.0<8;8,1>:w     6:w               {@2}
/* [06E8]  */         mov (8|M0)               r2.0<1>:d     r2.0<8;8,1>:uw                   {@1}
/* [06F8]  */         addc (8|M0)              r73.0<1>:ud   r2.0<8;8,1>:ud    r3.0<0;1,0>:ud   {AccWrEn,Compacted,@1}
/* [0700]  */         add (8|M0)               r72.0<1>:ud   acc0.0<8;8,1>:ud  r72.0<8;8,1>:ud 
/* [0710]  */ (W)     csel (4|M0)   (eq)f0.0   r1.0<1>:w     r1.0<4;1>:w       r1.0<4;1>:w       r1.0<1>:w        {@7}
/* [0720]  */ (~f1.0) if (8|M0)                            L2320                  L3864                
L1840:
/* [0730]  */         cmp (8|M0)    (lt)f1.0   null<1>:ud    r61.0<8;8,1>:ud   r63.0<8;8,1>:ud 
/* [0740]  */ (W)     csel (4|M0)   (eq)f0.0   r1.0<1>:w     r1.0<4;1>:w       r1.0<4;1>:w       r1.0<1>:w        {@3}
/* [0750]  */ (~f1.0) if (8|M0)                            L1960                  L2016                
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
/* [0900]  */         else (8|M0)                          L3864                  L3864                
L2320:
/* [0910]  */         mov (8|M0)               r2.0<2>:f     r73.0<8;8,1>:f                   {Compacted}
/* [0918]  */ (W)     mov (1|M0)               r60.0<1>:uw   f0.0<0;1,0>:uw                   {Compacted}
/* [0920]  */ (W)     mov (1|M0)               f0.0<1>:uw    0x0:uw                             
/* [0930]  */         cmp (8|M0)    (eq)f0.0   null<1>:uw    r2.0<0;1,0>:uw    r2.0<0;1,0>:uw   {@3}
/* [0940]  */         mov (8|M0)               r2.1<2>:f     r72.0<8;8,1>:f                   {Compacted}
/* [0948]  */         mov (8|M0)               r4.0<1>:f     r2.1<2;1,0>:f                    {Compacted,@1}
/* [0950]  */ (W&f0.0.any8h) mov (1|M0)        f1.1<1>:uw    0x0:uw                             
/* [0960]  */         cmp (8|M0)    (eq)f1.1   null<1>:uw    r5.0<0;1,0>:uw    r5.0<0;1,0>:uw  
/* [0970]  */ (W&f0.0.any8h) mov (1|M0)        r6.2<1>:uw    f1.1<0;1,0>:uw                  
/* [0980]  */ (W)     mov (1|M0)               f0.0<1>:uw    r60.0<0;1,0>:uw                  {@7}
L2448:
/* [0990]  */ (W)     mov (1|M0)               r60.0<1>:uw   f0.0<0;1,0>:uw                   {Compacted}
/* [0998]  */ (W)     mov (1|M0)               f0.0<1>:uw    0x0:uw                             
/* [09A8]  */         cmp (8|M0)    (eq)f0.0   null<1>:uw    r2.0<0;1,0>:uw    r2.0<0;1,0>:uw  
/* [09B8]  */ (W&f0.0.any8h) fbl (1|M0)        r6.0<1>:ud    r6.2<0;1,0>:uw                   {@5}
/* [09C8]  */ (W&f0.0.any8h) shl (1|M0)        r6.0<1>:ud    r6.0<0;1,0>:ud    2:w               {@1}
/* [09D8]  */ (W&f0.0.any8h) add (1|M0)        a0.0<1>:uw    r6.0<0;1,0>:uw    0x80:uw              {@1}
/* [09E8]  */ (W&f0.0.any8h) mov (1|M0)        r6.0<1>:ud    r[a0.0]<0;1,0>:ud                {@7}
/* [09F8]  */         cmp (8|M0)    (eq)f1.0   null<1>:ud    r6.0<0;1,0>:ud    r4.0<8;8,1>:ud   {@1}
/* [0A08]  */ (W)     csel (4|M0)   (eq)f0.0   r1.0<1>:w     r1.0<4;1>:w       r1.0<4;1>:w       r1.0<1>:w       
/* [0A18]  */ (f1.0)  send.dc1 (8|M0)          r5       r2      null    0x0            0x041404FF           {$7} // wr:2+0, rd:1; a64 byte gathering read 16b
/* [0A28]  */ (W&f0.0.any8h) and (1|M0)        r6.2<1>:uw    ~f1.0<0;1,0>:uw   r6.2<0;1,0>:uw  
/* [0A38]  */ (W&f0.0.any8h) mov (1|M0)        f1.0<1>:uw    r6.2<0;1,0>:uw                   {@1}
/* [0A48]  */ (W)     mov (1|M0)               f0.0<1>:uw    r60.0<0;1,0>:uw                  {@7}
/* [0A58]  */ (W)     csel (4|M0)   (eq)f0.0   r1.0<1>:w     r1.0<4;1>:w       r1.0<4;1>:w       r1.0<1>:w        {@4}
/* [0A68]  */ (f1.0)  while (8|M0)                         L2448                                
L2680:
/* [0A78]  */ (W)     mov (1|M0)               r60.0<1>:uw   f0.0<0;1,0>:uw                   {Compacted}
/* [0A80]  */ (W)     mov (1|M0)               f0.0<1>:uw    0x0:uw                             
/* [0A90]  */         cmp (8|M0)    (eq)f0.0   null<1>:uw    r2.0<0;1,0>:uw    r2.0<0;1,0>:uw  
/* [0AA0]  */         mov (8|M0)               r7.0<1>:d     r5.0<2;1,0>:uw                   {$7.dst}
/* [0AB0]  */ (W&f0.0.any8h) mul (8|M0)        acc0.0<1>:d   r61.0<8;8,1>:d    r63.0<2;1,0>:uw 
/* [0AC0]  */         mach (8|M0)              r13.0<1>:d    r61.0<8;8,1>:d    r63.0<8;8,1>:d   {Compacted}
/* [0AC8]  */ (W)     mov (1|M0)               f0.0<1>:uw    r60.0<0;1,0>:uw                  {@6}
/* [0AD8]  */         cmp (8|M0)    (gt)f0.1   null<1>:ud    r13.0<8;8,1>:ud   r7.0<8;8,1>:ud   {@2}
/* [0AE8]  */ (W)     csel (4|M0)   (eq)f0.0   r1.0<1>:w     r1.0<4;1>:w       r1.0<4;1>:w       r1.0<1>:w        {@7}
/* [0AF8]  */ (f0.1)  if (8|M0)                            L3848                  L3848                
L2824:
/* [0B08]  */         cmp (8|M0)    (eq)f0.0   null<1>:d     r62.0<8;8,1>:d    0:w               {Compacted}
/* [0B10]  */         mov (8|M0)               r15.0<1>:f    r64.0<2;1,0>:f                   {Compacted}
/* [0B18]  */         mov (8|M0)               r14.0<1>:f    r64.1<2;1,0>:f                   {Compacted}
L2848:
/* [0B20]  */         sync.nop                             null                             {Compacted,$9.src}
/* [0B28]  */         math.irem (8|M0)         r2.0<1>:ud    r7.0<8;8,1>:ud    r61.0<8;8,1>:ud  {@7,$8}
/* [0B38]  */         add (8|M0)               r12.0<1>:d    r69.0<8;8,1>:d    r2.0<8;8,1>:d    {Compacted,$8.dst}
/* [0B40]  */         math.iqot (8|M0)         r2.0<1>:ud    r7.0<8;8,1>:ud    r61.0<8;8,1>:ud  {@1,$10}
/* [0B50]  */         add (8|M0)               r10.0<1>:d    r68.0<8;8,1>:d    r2.0<8;8,1>:d    {Compacted,$10.dst}
/* [0B58]  */ (W)     csel (4|M0)   (eq)f0.0   r1.0<1>:w     r1.0<4;1>:w       r1.0<4;1>:w       r1.0<1>:w        {@7}
/* [0B68]  */ (f0.0)  if (8|M0)                            L2960                  L3360                
L2936:
/* [0B78]  */         mov (8|M0)               r6.0<1>:f     0.0:f                               {Compacted}
/* [0B80]  */         else (8|M0)                          L3360                  L3360                
L2960:
/* [0B90]  */         mov (8|M0)               acc0.0<1>:f   r12.0<8;8,1>:ud                  {@6}
/* [0BA0]  */ (W)     mov (1|M0)               r60.0<1>:uw   f0.0<0;1,0>:uw                   {Compacted}
/* [0BA8]  */         mul (8|M0)               acc0.0<1>:f   acc0.0<8;8,1>:f   r66.0<8;8,1>:f   {Compacted}
/* [0BB0]  */ (W)     mov (1|M0)               f0.0<1>:uw    0x0:uw                             
/* [0BC0]  */         add (8|M0)               r9.0<1>:f     acc0.0<8;8,1>:f   r71.0<8;8,1>:f   {Compacted}
/* [0BC8]  */         mov (8|M0)               acc0.0<1>:f   r10.0<8;8,1>:ud                  {@7}
/* [0BD8]  */         mov (8|M0)               r5.0<1>:f     0.0:f                               {Compacted}
/* [0BE0]  */         mul (8|M0)               acc0.0<1>:f   acc0.0<8;8,1>:f   r66.0<8;8,1>:f   {Compacted}
/* [0BE8]  */         mov (8|M0)               r3.0<1>:f     0.0:f                               {Compacted}
/* [0BF0]  */         cmp (8|M0)    (eq)f0.0   null<1>:uw    r2.0<0;1,0>:uw    r2.0<0;1,0>:uw  
/* [0C00]  */         mov (8|M0)               r6.0<1>:f     0.0:f                               {Compacted}
/* [0C08]  */         add (8|M0)               r8.0<1>:f     acc0.0<8;8,1>:f   r70.0<8;8,1>:f   {Compacted}
/* [0C10]  */         mov (8|M0)               r2.0<1>:f     0.0:f                               {Compacted}
/* [0C18]  */ (W&f0.0.any8h) mov (1|M0)        r11.0<1>:f    0.0:f                              
/* [0C28]  */ (W)     mov (1|M0)               f0.0<1>:uw    r60.0<0;1,0>:uw                 
L3128:
/* [0C38]  */         add (8|M0)               acc0.0<1>:f   r3.0<8;8,1>:f     -r5.0<8;8,1>:f   {Compacted,@7}
/* [0C40]  */ (W)     mov (1|M0)               r60.0<1>:uw   f0.0<0;1,0>:uw                   {Compacted}
/* [0C48]  */         add (8|M0)               r4.0<1>:f     acc0.0<8;8,1>:f   r9.0<8;8,1>:f    {Compacted}
/* [0C50]  */         mul (8|M0)               acc0.0<1>:f   r6.0<8;8,1>:f     2.0:f               {Compacted,@7}
/* [0C58]  */ (W)     mov (1|M0)               f0.0<1>:uw    0x0:uw                             
/* [0C68]  */         mul (8|M0)               acc0.0<1>:f   acc0.0<8;8,1>:f   r2.0<8;8,1>:f    {Compacted,@7}
/* [0C70]  */         add (8|M0)               r2.0<1>:f     acc0.0<8;8,1>:f   r8.0<8;8,1>:f    {Compacted,@7}
/* [0C78]  */         mul (8|M0)               r3.0<1>:f     r4.0<8;8,1>:f     r4.0<8;8,1>:f    {Compacted,@5}
/* [0C80]  */         mul (8|M0)               r5.0<1>:f     r2.0<8;8,1>:f     r2.0<8;8,1>:f    {Compacted,@2}
/* [0C88]  */         cmp (8|M0)    (eq)f0.0   null<1>:uw    r2.0<0;1,0>:uw    r2.0<0;1,0>:uw  
/* [0C98]  */         add (8|M0)               acc0.0<1>:f   r3.0<8;8,1>:f     r5.0<8;8,1>:f    {Compacted,@2}
/* [0CA0]  */         cmp (8|M0)    (le)f0.1   null<1>:f     acc0.0<8;8,1>:f   4.0:f              
/* [0CB0]  */ (W&f0.0.any8h) add (1|M0)        r11.0<1>:d    r11.0<0;1,0>:d    1:w              
/* [0CC0]  */ (W)     mov (1|M0)               f0.0<1>:uw    r60.0<0;1,0>:uw                 
/* [0CD0]  */         mov (8|M0)               r6.0<1>:f     r11.0<0;1,0>:f                   {Compacted,@2}
/* [0CD8]  */ (f0.1)  cmp (8|M0)    (lt)f0.1   null<1>:ud    r11.0<0;1,0>:ud   r62.0<8;8,1>:ud 
/* [0CE8]  */ (W)     csel (4|M0)   (eq)f0.0   r1.0<1>:w     r1.0<4;1>:w       r1.0<4;1>:w       r1.0<1>:w       
/* [0CF8]  */ (~f0.1) break (8|M0)                         L3344                  L3344                
L3336:
/* [0D08]  */         mov (8|M0)               r6.0<1>:f     r4.0<8;8,1>:f                    {Compacted}
L3344:
/* [0D10]  */         while (8|M0)                         L3128                                
L3360:
/* [0D20]  */         endif (8|M0)                         L3376                                
L3376:
/* [0D30]  */ (W)     mov (1|M0)               r60.0<1>:uw   f0.0<0;1,0>:uw                   {Compacted}
/* [0D38]  */ (W)     mov (1|M0)               f0.0<1>:uw    0x0:uw                             
/* [0D48]  */         cmp (8|M0)    (eq)f0.0   null<1>:uw    r2.0<0;1,0>:uw    r2.0<0;1,0>:uw  
/* [0D58]  */ (W&f0.0.any8h) mul (8|M0)        acc0.0<1>:d   r10.0<8;8,1>:d    r67.0<2;1,0>:uw 
/* [0D68]  */         mach (8|M0)              r2.0<1>:d     r10.0<8;8,1>:d    r67.0<8;8,1>:d   {Compacted}
/* [0D70]  */ (W&f0.0.any8h) mov (1|M0)        f1.1<1>:uw    0x0:uw                             
/* [0D80]  */         add (8|M0)               acc0.0<1>:d   r2.0<8;8,1>:d     r12.0<8;8,1>:d   {@2}
/* [0D90]  */         cmp (8|M0)    (eq)f1.1   null<1>:uw    r8.0<0;1,0>:uw    r8.0<0;1,0>:uw  
/* [0DA0]  */         shl (8|M0)               r2.0<1>:d     acc0.0<8;8,1>:d   2:w              
/* [0DB0]  */         addc (8|M0)              r5.0<1>:ud    r15.0<8;8,1>:ud   r2.0<8;8,1>:ud   {AccWrEn,Compacted,@1}
/* [0DB8]  */         mov (8|M0)               r4.0<1>:ud    acc0.0<8;8,1>:ud                 {Compacted}
/* [0DC0]  */         add (8|M0)               r2.1<2>:ud    r4.0<8;8,1>:ud    r14.0<8;8,1>:ud  {Compacted,@1}
/* [0DC8]  */         mov (8|M0)               r2.0<2>:f     r5.0<8;8,1>:f                    {Compacted,@3}
/* [0DD0]  */ (W&f0.0.any8h) mov (1|M0)        r5.2<1>:uw    f1.1<0;1,0>:uw                  
/* [0DE0]  */         mov (8|M0)               r4.0<1>:f     r2.1<2;1,0>:f                    {Compacted,@2}
/* [0DE8]  */ (W)     mov (1|M0)               f0.0<1>:uw    r60.0<0;1,0>:uw                 
L3576:
/* [0DF8]  */ (W)     mov (1|M0)               r60.0<1>:uw   f0.0<0;1,0>:uw                   {Compacted}
/* [0E00]  */ (W)     mov (1|M0)               f0.0<1>:uw    0x0:uw                             
/* [0E10]  */         cmp (8|M0)    (eq)f0.0   null<1>:uw    r2.0<0;1,0>:uw    r2.0<0;1,0>:uw  
/* [0E20]  */ (W&f0.0.any8h) fbl (1|M0)        r5.0<1>:ud    r5.2<0;1,0>:uw                   {@6}
/* [0E30]  */ (W&f0.0.any8h) shl (1|M0)        r5.0<1>:ud    r5.0<0;1,0>:ud    2:w               {@1}
/* [0E40]  */ (W&f0.0.any8h) add (1|M0)        a0.0<1>:uw    r5.0<0;1,0>:uw    0x80:uw              {@1}
/* [0E50]  */ (W&f0.0.any8h) mov (1|M0)        r5.0<1>:ud    r[a0.0]<0;1,0>:ud                {@7}
/* [0E60]  */         cmp (8|M0)    (eq)f1.1   null<1>:ud    r5.0<0;1,0>:ud    r4.0<8;8,1>:ud   {@1}
/* [0E70]  */ (W&f0.0.any8h) and (1|M0)        r5.2<1>:uw    ~f1.1<0;1,0>:uw   r5.2<0;1,0>:uw  
/* [0E80]  */ (f1.1)  send.dc1 (8|M0)          null     r2      r6      0x40            0x040681FF           {$9} // wr:2+1, rd:0; a64 dword scattering write
/* [0E90]  */ (W&f0.0.any8h) mov (1|M0)        f1.1<1>:uw    r5.2<0;1,0>:uw                   {@1}
/* [0EA0]  */ (W)     mov (1|M0)               f0.0<1>:uw    r60.0<0;1,0>:uw                  {@7}
/* [0EB0]  */ (W)     csel (4|M0)   (eq)f0.0   r1.0<1>:w     r1.0<4;1>:w       r1.0<4;1>:w       r1.0<1>:w       
/* [0EC0]  */ (f1.1)  while (8|M0)                         L3576                                
L3792:
/* [0ED0]  */         add (8|M0)               r7.0<1>:d     r7.0<8;8,1>:d     r74.0<0;1,0>:d   {Compacted,$6.dst}
/* [0ED8]  */         cmp (8|M0)    (lt)f0.1   null<1>:ud    r7.0<8;8,1>:ud    r13.0<8;8,1>:ud  {@1}
/* [0EE8]  */ (W)     csel (4|M0)   (eq)f0.0   r1.0<1>:w     r1.0<4;1>:w       r1.0<4;1>:w       r1.0<1>:w        {@4}
/* [0EF8]  */ (f0.1)  while (8|M0)                         L2848                                
L3848:
/* [0F08]  */         endif (8|M0)                         L3864                                
L3864:
/* [0F18]  */         endif (8|M0)                         L3880                                
L3880:
/* [0F28]  */ (W)     mov (1|M0)               r60.0<1>:uw   f0.0<0;1,0>:uw                   {Compacted}
/* [0F30]  */ (W)     mov (1|M0)               f0.0<1>:uw    0x0:uw                             
/* [0F40]  */         cmp (8|M0)    (eq)f0.0   null<1>:uw    r2.0<0;1,0>:uw    r2.0<0;1,0>:uw  
/* [0F50]  */ (W)     add (1|M16)              r126.2<1>:ud  r127.3<0;1,0>:ud  0x4:ud             
/* [0F60]  */ (W&f0.0.any8h) mov (2|M0)        r127.6<1>:f   r127.4<2;2,1>:f                 
/* [0F70]  */ (W&f0.0.any8h) mov (2|M0)        r127.4<1>:f   r60.2<2;2,1>:f                  
/* [0F80]  */ (W)     mov (1|M0)               f0.0<1>:uw    r60.0<0;1,0>:uw                  {@6}
/* [0F90]  */ (W)     send.dc0 (16|M16)        r60      r126    null    0x0            0x024804FF           {@1,$11} // wr:1h+0, rd:4; oword block read x8
/* [0FA0]  */ (W)     add (1|M16)              r126.2<1>:ud  r127.3<0;1,0>:ud  0xC:ud              {$11.src}
/* [0FB0]  */ (W)     send.dc0 (16|M16)        r64      r126    null    0x0            0x024804FF           {@1,$12} // wr:1h+0, rd:4; oword block read x8
/* [0FC0]  */ (W)     add (1|M16)              r126.2<1>:ud  r127.3<0;1,0>:ud  0x14:ud              {$12.src}
/* [0FD0]  */ (W)     send.dc0 (16|M16)        r68      r126    null    0x0            0x024804FF           {@1,$13} // wr:1h+0, rd:4; oword block read x8
/* [0FE0]  */ (W)     add (1|M16)              r126.2<1>:ud  r127.3<0;1,0>:ud  0x1C:ud              {$13.src}
/* [0FF0]  */ (W)     send.dc0 (16|M16)        r72      r126    null    0x0            0x024804FF           {@1,$6} // wr:1h+0, rd:4; oword block read x8
/* [1000]  */ (W)     add (1|M16)              r126.2<1>:ud  r127.3<0;1,0>:ud  0x0:ud              {$6.src}
/* [1010]  */ (W)     send.dc0 (8|M16)         r127     r126    null    0x0            0x021802FF           {@1,$14} // wr:1h+0, rd:1; oword block read x2
/* [1020]  */         sync.allrd                           null                             {@1}
/* [1030]  */         sync.allwr                           null                            
/* [1040]  */         ret (8|M0)                           r127.0                           {@1,$14.dst}
L4176:
