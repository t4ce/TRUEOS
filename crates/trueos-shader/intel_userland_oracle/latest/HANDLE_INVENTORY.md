# Latest Oracle Handle Inventory

This inventory is derived from `latest/log.txt` and the preserved files under
`latest/dumps/`.

Observed handle IDs in this capture:

- Present: `1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12`
- Not present: `0, 13..20`

## Submit groups

### 1-object prelude submit

- `batch_start=0x2000`
- object[0] = handle `9`
- preserved batch-like dump:
  `dumps/000534_pre_exec_handle_9_off_0x2000_len_0x2000.bin`

This is the already-tested prelude blob. It is not the real 11-object compute
batch.

### 11-object compute submit

- `batch_start=0x4000`
- object order:
  - object[0] = handle `11`
  - object[1] = handle `12`
  - object[2] = handle `3`
  - object[3] = handle `6`
  - object[4] = handle `5`
  - object[5] = handle `7`
  - object[6] = handle `4`
  - object[7] = handle `2`
  - object[8] = handle `1`
  - object[9] = handle `8`
  - object[10] = handle `9`

Important gap:

- object[0] handle `11` is the batch object for the real compute submit
- preserved dump only covers `offset=0x0 len=0x1000`
- required bytes would be at `batch_start=0x4000`, which are not preserved here

### 2-object follow-up submit

- object[0] = handle `9`
- object[1] = handle `10`
- `batch_start=0x0`
- `batch_len=0x8`

## Handle summary

| Handle | Created | Pre-exec dump | Munmap dump | Notes |
| --- | --- | --- | --- | --- |
| 1 | yes | `000621_pre_exec_handle_1_off_0x0_len_0x200000.bin` | `000836_munmap_handle_1_off_0x0_len_0x200000.bin` | in 11-object compute submit |
| 2 | yes | `000619_pre_exec_handle_2_off_0x0_len_0x200000.bin` | `000823_munmap_handle_2_off_0x0_len_0x200000.bin` | in 11-object compute submit |
| 3 | yes | `000609_pre_exec_handle_3_off_0x0_len_0x200000.bin` | `000808_munmap_handle_3_off_0x0_len_0x200000.bin` | in 11-object compute submit |
| 4 | yes | `000617_pre_exec_handle_4_off_0x0_len_0x200000.bin` | `000778_munmap_handle_4_off_0x0_len_0x200000.bin` | in 11-object compute submit |
| 5 | yes | `000613_pre_exec_handle_5_off_0x0_len_0x200000.bin` | `000793_munmap_handle_5_off_0x0_len_0x200000.bin` | in 11-object compute submit |
| 6 | yes | `000611_pre_exec_handle_6_off_0x0_len_0x200000.bin` | `000763_munmap_handle_6_off_0x0_len_0x200000.bin` | in 11-object compute submit |
| 7 | yes | `000615_pre_exec_handle_7_off_0x0_len_0x200000.bin` | `000748_munmap_handle_7_off_0x0_len_0x200000.bin` | in 11-object compute submit |
| 8 | yes | `000623_pre_exec_handle_8_off_0x0_len_0x200000.bin` | `000735_munmap_handle_8_off_0x0_len_0x200000.bin` | in 11-object compute submit |
| 9 | yes | `000534_pre_exec_handle_9_off_0x2000_len_0x2000.bin` | `000520_munmap_handle_9_off_0x0_len_0x2000.bin`, `000579_munmap_handle_9_off_0x0_len_0x4000.bin`, `000722_munmap_handle_9_off_0x0_len_0x2000.bin` | appears in prelude, 11-object submit, and 2-object follow-up |
| 10 | yes | `000655_pre_exec_handle_10_off_0x0_len_0x1000.bin` | `000725_munmap_handle_10_off_0x0_len_0x1000.bin` | in 2-object follow-up submit |
| 11 | yes | `000605_pre_exec_handle_11_off_0x0_len_0x1000.bin` | `000707_munmap_handle_11_off_0x0_len_0x1000.bin` | real compute batch object, but wrong offset preserved |
| 12 | yes | `000607_pre_exec_handle_12_off_0x0_len_0x1000.bin` | `000714_munmap_handle_12_off_0x0_len_0x1000.bin` | in 11-object compute submit |

## Immediate conclusion

If we want "all handles", they are already enumerated above for this capture.
If we want the real compute batch bytes, the missing recapture target is still:

- handle `11`
- offset `0x4000`
- from the 11-object compute submit
