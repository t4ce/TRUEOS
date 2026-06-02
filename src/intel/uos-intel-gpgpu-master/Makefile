# targets
LINK_CXX_TARGET ?= uos-intel-gpgpu-link-cxx.o
LINK_C_TARGET ?= uos-intel-gpgpu-link-c.o
LINK_CL_TARGET ?= uos-intel-gpgpu-link-ocl.o
LINK_CL_CXX_TARGET ?= uos-intel-gpgpu-link-ocl-cxx.o
EXAMPLE_CXX_TARGET ?= uos-intel-gpgpu-example-cxx
EXAMPLE_C_TARGET ?= uos-intel-gpgpu-example-c
EXAMPLE_OCL_TARGET ?= uos-intel-gpgpu-example-ocl

# commands
MKDIR_P ?= mkdir -p
CC ?= gcc
CXX ?= g++
LD ?= ld

# dirs
BUILD_DIR ?= ./build

# source, object and dependency files
SRCS = driver/gpgpu_driver.cc driver/gtt.cc driver/ggtt32.cc driver/ppgtt32.cc driver/parseGEN.cc driver/ringbuffer.cc
OBJS = $(SRCS:%.cc=$(BUILD_DIR)/%.o)
DEPS = $(OBJS:%.o=%.d)
SRCS_OTHER = example/main.c example/main.cc example/mainOCL.c opencl/cl.c CConnector.cc cpp_support.cc stubc.cc
OBJS_OTHER_CC = $(SRCS_OTHER:%.cc=$(BUILD_DIR)/%.o)
OBJS_OTHER_ALL += $(OBJS_OTHER_CC:%.c=$(BUILD_DIR)/%.o)
DEPS += $(OBJS_OTHER_ALL:%.o=%.d)

# flags
CFLAGS ?= -ffreestanding -Wall -Wextra -Wpedantic -Werror -fno-exceptions -Wconversion
CPPFLAGS ?= -ffreestanding -Wall -Wextra -Wpedantic -Weffc++ -Werror -Wsuggest-override -Wconversion -fno-exceptions -fno-rtti -fno-threadsafe-statics
LDFLAGS ?= -ffreestanding -Wall -Wextra -Wpedantic -Weffc++ -Werror -Wsuggest-override -Wconversion -fno-exceptions -fno-rtti -fno-threadsafe-statics -nostdlib -lgcc
LDFLAGS_C ?= -ffreestanding -Wall -Wextra -Wpedantic -Werror -fno-exceptions -nostdlib -lgcc

#CPPFLAGS += -O3
#CFLAGS += -O3

# all target
all: $(BUILD_DIR)/$(EXAMPLE_CXX_TARGET) $(BUILD_DIR)/$(EXAMPLE_C_TARGET) $(BUILD_DIR)/$(EXAMPLE_OCL_TARGET) $(BUILD_DIR)/$(LINK_CL_CXX_TARGET)

# example C++ target
$(BUILD_DIR)/$(EXAMPLE_CXX_TARGET): $(BUILD_DIR)/$(LINK_CXX_TARGET) example/main.cc $(BUILD_DIR)/cpp_support.o $(BUILD_DIR)/stubs.o
	$(CXX) example/main.cc $(BUILD_DIR)/stubs.o $(BUILD_DIR)/cpp_support.o $(BUILD_DIR)/$(LINK_CXX_TARGET) -o $@ $(LDFLAGS)

# example C target
$(BUILD_DIR)/$(EXAMPLE_C_TARGET): $(BUILD_DIR)/$(LINK_C_TARGET) example/main.c $(BUILD_DIR)/stubs.o
	$(CC) example/main.c $(BUILD_DIR)/stubs.o $(BUILD_DIR)/$(LINK_C_TARGET) -o $@ $(LDFLAGS_C)

# example OCL target
$(BUILD_DIR)/$(EXAMPLE_OCL_TARGET): $(BUILD_DIR)/$(LINK_CL_TARGET) example/mainOCL.c $(BUILD_DIR)/stubs.o
	$(CC) example/mainOCL.c $(BUILD_DIR)/stubs.o $(BUILD_DIR)/$(LINK_CL_TARGET) -o $@ $(LDFLAGS_C)

# C++ link target
$(BUILD_DIR)/$(LINK_CXX_TARGET): $(OBJS)
	$(LD) -r -o $(BUILD_DIR)/$(LINK_CXX_TARGET) $(OBJS)

# C link target
$(BUILD_DIR)/$(LINK_C_TARGET): $(BUILD_DIR)/$(LINK_CXX_TARGET) $(BUILD_DIR)/CConnector.o $(BUILD_DIR)/cpp_support.o
	$(LD) -r -o $(BUILD_DIR)/$(LINK_C_TARGET) $(BUILD_DIR)/$(LINK_CXX_TARGET) $(BUILD_DIR)/cpp_support.o $(BUILD_DIR)/CConnector.o

# OCL link target
$(BUILD_DIR)/$(LINK_CL_TARGET): $(BUILD_DIR)/$(LINK_C_TARGET) $(BUILD_DIR)/opencl/cl.o
	$(LD) -r -o $(BUILD_DIR)/$(LINK_CL_TARGET) $(BUILD_DIR)/$(LINK_C_TARGET) $(BUILD_DIR)/opencl/cl.o

# OCL cxx link target
$(BUILD_DIR)/$(LINK_CL_CXX_TARGET): $(BUILD_DIR)/$(LINK_CXX_TARGET) $(BUILD_DIR)/opencl/cl.o  $(BUILD_DIR)/CConnector.o
	$(LD) -r -o $(BUILD_DIR)/$(LINK_CL_CXX_TARGET) $(BUILD_DIR)/$(LINK_CXX_TARGET) $(BUILD_DIR)/CConnector.o $(BUILD_DIR)/opencl/cl.o

# object target
$(BUILD_DIR)/%.o: %.cc
	$(MKDIR_P) $(dir $@)
	$(CXX) $(CXX_FLAGS) -MMD -c $< -o $@
	$(CXX) $(CPPFLAGS) $(CXXFLAGS) -c $< -o $@

$(BUILD_DIR)/%.o: %.c
	$(MKDIR_P) $(dir $@)
	$(CC) $(CFLAGS) -MMD -c $< -o $@
	$(CC) $(CFLAGS) -c $< -o $@

# include dep targets
-include $(DEPS)

# clean target
.PHONY: all clean

clean:
	$(RM) -r $(BUILD_DIR)
