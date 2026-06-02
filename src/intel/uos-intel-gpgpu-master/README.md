# Porting to your own OS
Using the driver in your own OS is fairly easy. All functions, that have to be implemented can be found in the stubs.cc file.
In addition to the stubs.cc implementation, interrupt handling is neccessary. When a GPU interrupt occurs, `GPGPU_Driver::getInstance().handleInterrupt()` and `GPGPU_Driver::getInstance().runNext()` have to be called (in this order). The default interrupt vector is `0x31` and can be changed in the `init` function of the driver (compare [Execution of GPGPU-Tasks](#execution-of-gpgpu-tasks)).

# Compatible Hardware
The GPGPU-Driver does only work with GEN9.5 graphic units. Other generations may work too, but are not tested. The GEN9.5 units are used since the Kably-Lake desktop processor architecture.  
Full list of campatible devices:
* UHD Graphics 600
* UHD Graphics 605
* HD Graphics 610
* HD Graphics 615
* HD Graphics 620
* UHD Graphics 620
* HD Graphics 630
* UHD Graphics 630
* HD Graphics P630
* Iris Plus Graphics 640
* Iris Plus Graphics 650

# Building GPGPU Programs
The GPGPU-Programs are build with the `ocloc` Tool. This can be installed with
```sh
wget https://github.com/intel/compute-runtime/releases/download/19.39.14278/intel-igc-core_1.0.2597_amd64.deb
wget https://github.com/intel/compute-runtime/releases/download/19.39.14278/intel-igc-opencl_1.0.2597_amd64.deb
wget https://github.com/intel/compute-runtime/releases/download/19.39.14278/intel-ocloc_19.39.14278_amd64.deb

sudo dpkg -i *.deb
```
A Program can be build with:
```sh
ocloc -file kernel.cl -device skl
```
This should generate some `kernel_Gen9core` files. We just need the `kernel_Gen9core.gen`.
It contains the GPU-Code and some other information required by the driver.
If we want to use it in the source code we can convert it into a C-Array with the hexeditor `xxd`:  
```sh
xxd -i kernel_Gen9core.gen > kernel.h
```

# Execution of GPGPU-Tasks
First of all the driver has to be initialized with:
```C++
GPGPU_Driver::getInstance().init(0x31); // interrupt vector: 0x31
```

Then we have to create a config structure which contains the information of the GPGPU-Program:
```C++
kernel_config kconf; // kernel config structure
buffer_config buffconf[2]; // buffer config structure

kconf.range[0] = number_of_executions_X;
kconf.range[1] = number_of_executions_Y;
kconf.range[0] = number_of_executions_Z;
kconf.workgroupsize[0] = 0; // 0 = auto
kconf.workgroupsize[1] = 0; // 0 = auto
kconf.workgroupsize[2] = 0; // 0 = auto
kconf.kernel = kernel_binary; // compiled gpu binary
kconf.kernelName = "clmain": // nullptr => use first function

// function, that will be called after the GPGPU-Task has finished
kconf.finish_callback = cleanUp;

// example: kernel void clmain(int* buff1, float* buff2)
kconf.buffCount = 2; // number of buffers
kconf.buffConfigs = buffconf;
kconf.buffConfigs[0].buffer = addr_of_buff1; // address buffer
kconf.buffConfigs[0].buffer_size = size_of_buff1; // buffersize in bytes
kconf.buffConfigs[1].buffer = addr_of_buff2;
kconf.buffConfigs[1].buffer_size = size_of_buff2;

```
**Make sure that buffConfigs has the same order like the parameters of the kernels main method.**  
To gain good performance we should set the maximum gpu-frequency with:
```C++
// enable maximum freuqency
gpgpudriver.setMaxFreq();
```
After that we can enqueue the GPU-Task into the workqueue with:
```C++
// enqueue the GPGPU-Program into workingqueue
GPGPU_Driver::getInstance().enqueueRun(kconf);
```
If the GPU-Task has finished we should set the minimum frequency again to save power and lower the temperature:
```C++
gpgpudriver.setMinFreq();
```
This can be done in the callback function set in the config structure.  
A complete example can be found in the example folder.

# Support
If any questions are still open, feel free to ask:
* Marcel Lütke Dreimann (marcel.luetkedreimann@uos.de)
