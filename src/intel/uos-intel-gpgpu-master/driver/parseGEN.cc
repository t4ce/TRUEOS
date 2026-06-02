#include "parseGEN.h"

// Intel Headers (CURRENT_ICBE_VERSION has to fit to ocloc version!):
// TODO: update headers to newer version, which still works! (newest ocloc seems to generate diff. instr. => break driver)
#include "patch_list.h"
#include "patch_shared.h"

void parseGEN(CrossThreadData_info &info, kernel_config &kconf, uint8_t *instr)
{
    uint8_t *curpos = kconf.binary;

    // get program header
    iOpenCL::SProgramBinaryHeader *programHeader = (iOpenCL::SProgramBinaryHeader *)curpos;
    curpos += sizeof(iOpenCL::SProgramBinaryHeader);
    curpos += programHeader->PatchListSize;
#ifdef DEBUG
    // check binary format and version
    if (programHeader->Magic != iOpenCL::MAGIC_CL)
    {
        // wrong binary format
        info.err = 0x1;
        return;
    }
    if (programHeader->Version != iOpenCL::CURRENT_ICBE_VERSION)
    {
        // wrong version format
        info.err = 0x2;
        return;
    }
#endif // DEBUG

    // get number of kernels
    const uint32_t nKernels = programHeader->NumberOfKernels;

    // get kernel header
    iOpenCL::SKernelBinaryHeaderCommon *kernelHeader = (iOpenCL::SKernelBinaryHeaderCommon *)curpos;
    curpos += sizeof(iOpenCL::SKernelBinaryHeaderCommon);

    // use specific kernel?
    if (kconf.kernelName != nullptr)
    {
        uint32_t j = 0;
        for (uint32_t i = 0; i < nKernels; i++)
        {
            // check kernel name
            for (j = 0; j < kernelHeader->KernelNameSize; j++)
            {
                if (curpos[j] != kconf.kernelName[j])
                {
                    // skip this kernel
                    curpos += kernelHeader->KernelNameSize;
                    curpos += kernelHeader->KernelHeapSize;
                    curpos += kernelHeader->GeneralStateHeapSize;
                    curpos += kernelHeader->DynamicStateHeapSize;
                    curpos += kernelHeader->SurfaceStateHeapSize;
                    curpos += kernelHeader->PatchListSize;

                    // get next kernel header
                    kernelHeader = (iOpenCL::SKernelBinaryHeaderCommon *)curpos;
                    curpos += sizeof(iOpenCL::SKernelBinaryHeaderCommon);

                    break; // abort kernel name check
                }
                if (curpos[j] == '\0') // if we are at the end (KernelNameSize is not necessarily the string length, its the size of the memory block for the string)
                {
                    // indicate that kernel was found
                    j = kernelHeader->KernelNameSize;

                    // quit the loop
                    break;
                }
            }
            if (j == kernelHeader->KernelNameSize)
                break; // we found the kernel!
        }
#ifdef DEBUG
        // did not find kernel name in binary
        if (j != kernelHeader->KernelNameSize)
        {
            info.err = 0x4;
            return;
        }
#endif // DEBUG
    }

#ifdef DEBUG
    // check binary size
    if (kernelHeader->KernelHeapSize > MAX_KERNEL_SIZE)
    {
        // binary exeeded MAX_KERNEL_SIZE
        info.err = 0x3;
        return;
    }
#endif // DEBUG

    // skip kernel name
    curpos += kernelHeader->KernelNameSize;

    // get kernel heap (contains instructions)
    uint8_t *kernelInstr = curpos;
    curpos += kernelHeader->KernelHeapSize;

    // copy kernel instr
    for (uint32_t i = 0; i < kernelHeader->KernelHeapSize; i++)
    {
        *instr++ = kernelInstr[i];
    }

    // skip general state
    curpos += kernelHeader->GeneralStateHeapSize;
    // skip dynamic state
    curpos += kernelHeader->DynamicStateHeapSize;
    // skip surface state
    curpos += kernelHeader->SurfaceStateHeapSize;

    // get patch list
    uint8_t *patchListStart = curpos;
    uint8_t *patchListIter = patchListStart;
    // curpos += kernelHeader->PatchListSize;

    // parse patch list
    do
    {
        // search data buffer patch
        iOpenCL::SPatchItemHeader *patch = (iOpenCL::SPatchItemHeader *)patchListIter;
        if (patch->Token == iOpenCL::PATCH_TOKEN_DATA_PARAMETER_BUFFER)
        {
            // get data buffer patch
            iOpenCL::SPatchDataParameterBuffer *dataPatch = (iOpenCL::SPatchDataParameterBuffer *)patch;
            switch (dataPatch->Type)
            {
            /*case iOpenCL::DATA_PARAMETER_GLOBAL_WORK_OFFSET:
            {
                // get offset
                uint32_t index = dataPatch->SourceOffset / sizeof(uint32_t);
                info.workOffset[index] = dataPatch->Offset;
                break;
            }*/
            case iOpenCL::DATA_PARAMETER_LOCAL_WORK_SIZE:
            {
                // get work dim
                uint32_t index = dataPatch->SourceOffset / sizeof(uint32_t);
                info.workDim[index] = dataPatch->Offset;
                break;
            }
            case iOpenCL::DATA_PARAMETER_ENQUEUED_LOCAL_WORK_SIZE:
            {
                // get enqueued work dim
                uint32_t index = dataPatch->SourceOffset / sizeof(uint32_t);
                info.enqueuedWorkDim[index] = dataPatch->Offset;
                break;
            }
            case iOpenCL::DATA_PARAMETER_BUFFER_STATEFUL:
            {
                // get buffer address position
                kconf.buffConfigs[dataPatch->ArgumentNumber].pos = dataPatch->Offset;
                break;
            }
            case iOpenCL::DATA_PARAMETER_KERNEL_ARGUMENT:
            {
                // get parameter value position
                kconf.buffConfigs[dataPatch->ArgumentNumber].pos = dataPatch->Offset;
                break;
            }
            }
        }
        else if (patch->Token == iOpenCL::PATCH_TOKEN_EXECUTION_ENVIRONMENT)
        {
            // get execution evironment patch
            iOpenCL::SPatchExecutionEnvironment *patchExecEnv = (iOpenCL::SPatchExecutionEnvironment *)patch;

            // set simd mode
            if (patchExecEnv->CompiledSIMD8)
            {
                kconf.simd = 8;
            }
            else if (patchExecEnv->CompiledSIMD16)
            {
                kconf.simd = 16;
            }
            else if (patchExecEnv->CompiledSIMD32)
            {
                kconf.simd = 32;
            }

            // set barriers flag
            kconf.useBarrier = patchExecEnv->HasBarriers;
        }
        else if (patch->Token == iOpenCL::PATCH_TOKEN_STATELESS_GLOBAL_MEMORY_OBJECT_KERNEL_ARGUMENT)
        {
            // get argument patch
            iOpenCL::SPatchStatelessGlobalMemoryObjectKernelArgument *argPatch = (iOpenCL::SPatchStatelessGlobalMemoryObjectKernelArgument *)patch;

            // get buffer address position
            kconf.buffConfigs[argPatch->ArgumentNumber].pos = argPatch->DataParamOffset;
        }

        // advance to next patch
        patchListIter += patch->Size;
    } while (patchListIter - patchListStart < kernelHeader->PatchListSize);
}
