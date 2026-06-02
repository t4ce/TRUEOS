#include "kernel_mandel.h"
#define CL_TARGET_OPENCL_VERSION 100
#include "../opencl/cl.h"
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

    // free buffers
    free((void *)out);
    free((void *)real);
    free((void *)img);
}

void main()
{
    cl_platform_id platform_id;
    cl_device_id device_id;
    cl_uint num_devices;
    cl_uint num_platforms;
    cl_int errcode;
    cl_context clContext;
    cl_kernel clKernel;
    cl_command_queue clCommandQue;
    cl_program clProgram;
    cl_mem clOutBuff;
    cl_mem clRealBuff;
    cl_mem clImgBuff;

    // init opencl stuff
    errcode = clGetPlatformIDs(1, &platform_id, &num_platforms);
    if (errcode == CL_SUCCESS)
        printk("number of platforms is %d\n", num_platforms);
    errcode = clGetDeviceIDs(platform_id, CL_DEVICE_TYPE_GPU, 1, &device_id, &num_devices);
    if (errcode == CL_SUCCESS)
        printk("number of devices is %d\n", num_devices);
    clContext = clCreateContext(NULL, 1, &device_id, NULL, NULL, &errcode);
    if (errcode != CL_SUCCESS)
        printk("Error in creating context\n");
    clCommandQue = clCreateCommandQueue(clContext, device_id, 0, &errcode);
    if (errcode != CL_SUCCESS)
        printk("Error in creating command queue\n");

    // allocate buffers
    out = (uint32_t *)gpgpu_aligned_alloc(0x10000, SIZE * SIZE * sizeof(int));
    real = (float *)gpgpu_aligned_alloc(0x10000, SIZE * SIZE * sizeof(float));
    img = (float *)gpgpu_aligned_alloc(0x10000, SIZE * SIZE * sizeof(float));

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

    // allocate opencl buffers
    clOutBuff = clCreateBuffer(clContext, CL_MEM_READ_WRITE, SIZE * SIZE * sizeof(int), NULL, &errcode);
    if (errcode != CL_SUCCESS)
        printk("Error in creating buffer\n");
    clRealBuff = clCreateBuffer(clContext, CL_MEM_READ_WRITE, SIZE * SIZE * sizeof(float), NULL, &errcode);
    if (errcode != CL_SUCCESS)
        printk("Error in creating buffer\n");
    clImgBuff = clCreateBuffer(clContext, CL_MEM_READ_WRITE, SIZE * SIZE * sizeof(float), NULL, &errcode);
    if (errcode != CL_SUCCESS)
        printk("Error in creating buffer\n");

    // init buffers
    errcode = clEnqueueWriteBuffer(clCommandQue, clOutBuff, CL_TRUE, 0, SIZE * SIZE * sizeof(int), out, 0, NULL, NULL);
    if (errcode != CL_SUCCESS)
        printk("Error in writing to buffer\n");
    errcode = clEnqueueWriteBuffer(clCommandQue, clRealBuff, CL_TRUE, 0, SIZE * SIZE * sizeof(float), real, 0, NULL, NULL);
    if (errcode != CL_SUCCESS)
        printk("Error in writing to buffer\n");
    errcode = clEnqueueWriteBuffer(clCommandQue, clImgBuff, CL_TRUE, 0, SIZE * SIZE * sizeof(float), img, 0, NULL, NULL);
    if (errcode != CL_SUCCESS)
        printk("Error in writing to buffer\n");

    // create a program from the kernel source
    const size_t kernel_size = mandel_Gen9core_gen_len;
    const unsigned char *kernel_bin = mandel_Gen9core_gen;
    clProgram = clCreateProgramWithBinary(clContext, 1, &device_id, &kernel_size, &kernel_bin, NULL, &errcode);

    // build the program
    errcode = clBuildProgram(clProgram, 1, &device_id, NULL, NULL, NULL);
    if (errcode != CL_SUCCESS)
        printk("Error in building program\n");

    // create the OpenCL kernel
    clKernel = clCreateKernel(clProgram, "clmain", &errcode);
    if (errcode != CL_SUCCESS)
        printk("Error in creating kernel\n");

    // set kernel args
    errcode = clSetKernelArg(clKernel, 0, sizeof(cl_mem), (void *)&clOutBuff);
    if (errcode != CL_SUCCESS)
        printk("Error in setting kernel arg\n");
    errcode = clSetKernelArg(clKernel, 1, sizeof(cl_mem), (void *)&clRealBuff);
    if (errcode != CL_SUCCESS)
        printk("Error in setting kernel arg\n");
    errcode = clSetKernelArg(clKernel, 2, sizeof(cl_mem), (void *)&clImgBuff);
    if (errcode != CL_SUCCESS)
        printk("Error in setting kernel arg\n");

    // launch the kernel
    size_t globalWorkSize = SIZE * SIZE;
    errcode = clEnqueueNDRangeKernel(clCommandQue, clKernel, 1, NULL, &globalWorkSize, 0, 0, NULL, NULL);
    if (errcode != CL_SUCCESS)
        printk("Error in launching kernel\n");

    // wait for finish
    clFinish(clCommandQue);

    // free stuff
    errcode = clReleaseKernel(clKernel);
    if (errcode != CL_SUCCESS)
        printk("Error in releasing kernel\n");
    errcode = clReleaseMemObject(clOutBuff);
    if (errcode != CL_SUCCESS)
        printk("Error in releasing mem obj\n");
    errcode = clReleaseMemObject(clRealBuff);
    if (errcode != CL_SUCCESS)
        printk("Error in releasing mem obj\n");
    errcode = clReleaseMemObject(clImgBuff);
    if (errcode != CL_SUCCESS)
        printk("Error in releasing mem obj\n");
    errcode = clReleaseProgram(clProgram);
    if (errcode != CL_SUCCESS)
        printk("Error in releasing program\n");
    errcode = clReleaseCommandQueue(clCommandQue);
    if (errcode != CL_SUCCESS)
        printk("Error in releasing command queue\n");
    errcode = clReleaseContext(clContext);
    if (errcode != CL_SUCCESS)
        printk("Error in releasing context\n");

    printk("End");
    while (1)
        ;
}
