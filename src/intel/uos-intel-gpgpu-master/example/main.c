#include "kernel_mandel.h"
#include "../CConnector.h"
#include "../stubs.h"

#define SIZE 1024

// io buffers
uint32_t *out;
float *real;
float *img;

void renderAscii(volatile uint32_t *out, uint32_t res)
{
    // render ASCII Art (64x64)
    for (uint32_t y = 0; y < 64; y++)
    {
        for (uint32_t x = 0; x < 64; x++)
        {
            // mean
            uint32_t m = 0;

            const uint32_t size = res / 64;

            // sum up sizexsize field
            for (uint32_t ly = 0; ly < size; ly++)
            {
                for (uint32_t lx = 0; lx < size; lx++)
                {
                    uint32_t finalx = (x * size) + lx;
                    uint32_t finaly = (y * size) + ly;
                    m += out[finalx + res * finaly];
                }
            }

            // print mean
            m /= (size * size);
            if (m < 8)
                printk(" ");
            else
                printk("+");
        }
        printk("\n");
        for (int i = 0; i < 999999999; i++)
            asm("nop"); // "scroll effekt"
    }
}

void cleanUp()
{
    // print result
    renderAscii(out, SIZE);

    // set gpu frequency to minimum
    gpgpu_setMinFreq();

    // free buffers
    free((void *)out);
    free((void *)real);
    free((void *)img);
}

void main()
{
    // init driver
    gpgpu_init(0x31);

    // create kernel and buffer config struct
    struct kernel_config kconf;
    struct buffer_config buffconf[3];

    INIT_KERNEL_CONFIG(kconf);
    INIT_BUFFER_CONFIG(buffconf[0]);
    INIT_BUFFER_CONFIG(buffconf[1]);
    INIT_BUFFER_CONFIG(buffconf[2]);

    kconf.range[0] = SIZE * SIZE; // number of executions
    kconf.workgroupsize[0] = 0;   // 0 = auto
    kconf.binary = mandel_Gen9core_gen;
    kconf.finish_callback = cleanUp;
    // kconf.kernelName = "clmain": // NULL => use first function

    // allocate buffers
    out = (uint32_t *)gpgpu_aligned_alloc(0x10000, kconf.range[0] * sizeof(int));
    real = (float *)gpgpu_aligned_alloc(0x10000, kconf.range[0] * sizeof(float));
    img = (float *)gpgpu_aligned_alloc(0x10000, kconf.range[0] * sizeof(float));

    // config buffers
    kconf.buffCount = 3;
    kconf.buffConfigs = buffconf;
    kconf.buffConfigs[0].buffer = (uint32_t *)out;
    kconf.buffConfigs[0].buffer_size = kconf.range[0] * sizeof(int);
    kconf.buffConfigs[1].buffer = real;
    kconf.buffConfigs[1].buffer_size = kconf.range[0] * sizeof(float);
    kconf.buffConfigs[2].buffer = img;
    kconf.buffConfigs[2].buffer_size = kconf.range[0] * sizeof(float);

    // step width
    float step = 4.0f / (SIZE - 1);

    // (x, y) = [-2, 2]x[-2, 2]
    int idx = 0;
    int fixfloat_y = 1;
    for (float y = -2; y <= 2; y += step)
    {
        for (float x = -2; x <= 2; x += step)
        {
            real[idx] = x;
            img[idx] = y;
            idx++;
        }
        idx = SIZE * fixfloat_y++; // fix floats
    }

    // set maximum freuqency
    gpgpu_setMaxFreq();

    // start gpu task
    gpgpu_enqueueRun(&kconf);

    printk("End");
    while (1)
        ;
}
