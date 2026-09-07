/* Host-only cross-check against the pinned Mesa ISL; no DRM device is opened. */
#include <stdio.h>
#include "isl/isl.h"
#include "dev/intel_device_info.h"
int main(void) {
   struct intel_device_info info;
   if (!intel_get_device_info_from_pci_id(0xa780, &info)) return 1;
   struct isl_device dev;
   isl_device_init(&dev, &info);
   const unsigned sizes[][2] = {{1,1},{128,64},{129,65},{784,441},{785,443},{2560,1440}};
   for (unsigned i = 0; i < sizeof(sizes)/sizeof(sizes[0]); i++) {
      struct isl_surf depth, hiz;
      bool ok = isl_surf_init(&dev, &depth, .dim=ISL_SURF_DIM_2D,
            .format=ISL_FORMAT_R32_FLOAT, .width=sizes[i][0], .height=sizes[i][1],
            .depth=1, .levels=1, .array_len=1, .samples=1,
            .usage=ISL_SURF_USAGE_DEPTH_BIT, .tiling_flags=ISL_TILING_Y0_BIT);
      if (!ok || !isl_surf_get_hiz_surf(&dev, &depth, &hiz)) return 2;
      printf("%u %u pitch=%u qpitch=%u bytes=%llu alignment=%u\n", sizes[i][0],sizes[i][1],
             hiz.row_pitch_B, isl_surf_get_array_pitch_sa_rows(&hiz)/4,
             (unsigned long long)hiz.size_B, hiz.alignment_B);
      unsigned pitch = (sizes[i][0]+127)/128*128;
      unsigned qpitch = (sizes[i][1]+15)/16*4;
      unsigned bytes = pitch*((sizes[i][1]+63)/64*32);
      if (hiz.row_pitch_B != pitch || isl_surf_get_array_pitch_sa_rows(&hiz)/4 != qpitch || hiz.size_B != bytes)
         return 3;
   }
   return 0;
}
