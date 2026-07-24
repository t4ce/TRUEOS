#define CL_TARGET_OPENCL_VERSION 300

#include "lfm25_igpu.hpp"

#include <CL/cl.h>

#include <array>
#include <fstream>
#include <stdexcept>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

namespace trueos::lfm25 {
namespace {

constexpr cl_uint kIntelVendor = 0x8086;
constexpr std::size_t kMaximumActivationBytes = 4'896;
constexpr std::size_t kMaximumOutputElements = 65'536;
constexpr std::size_t kLocalSize = 16;

[[noreturn]] void fail_opencl(std::string_view operation, cl_int error) {
    throw std::runtime_error(
        std::string(operation) + " failed with OpenCL error " +
        std::to_string(error));
}

void require(cl_int error, std::string_view operation) {
    if (error != CL_SUCCESS) {
        fail_opencl(operation, error);
    }
}

std::vector<std::byte> read_binary(const std::filesystem::path & path) {
    std::ifstream input(path, std::ios::binary | std::ios::ate);
    if (!input) {
        throw std::runtime_error("cannot open IGC SPIR-V " + path.string());
    }
    const auto end = input.tellg();
    if (end <= 0) {
        throw std::runtime_error("empty IGC SPIR-V " + path.string());
    }
    std::vector<std::byte> bytes(static_cast<std::size_t>(end));
    input.seekg(0);
    input.read(
        reinterpret_cast<char *>(bytes.data()),
        static_cast<std::streamsize>(bytes.size()));
    if (!input) {
        throw std::runtime_error("cannot read IGC SPIR-V " + path.string());
    }
    return bytes;
}

template <typename Handle, cl_int (*Release)(Handle)>
class cl_owner {
  public:
    cl_owner() = default;
    explicit cl_owner(Handle handle) : handle_(handle) {}

    ~cl_owner() {
        if (handle_ != nullptr) {
            Release(handle_);
        }
    }

    cl_owner(const cl_owner &) = delete;
    cl_owner & operator=(const cl_owner &) = delete;

    cl_owner(cl_owner && other) noexcept
        : handle_(std::exchange(other.handle_, nullptr)) {}

    cl_owner & operator=(cl_owner && other) noexcept {
        if (this != &other) {
            if (handle_ != nullptr) {
                Release(handle_);
            }
            handle_ = std::exchange(other.handle_, nullptr);
        }
        return *this;
    }

    Handle get() const {
        return handle_;
    }

  private:
    Handle handle_ = nullptr;
};

using context_owner = cl_owner<cl_context, clReleaseContext>;
using queue_owner = cl_owner<cl_command_queue, clReleaseCommandQueue>;
using program_owner = cl_owner<cl_program, clReleaseProgram>;
using kernel_owner = cl_owner<cl_kernel, clReleaseKernel>;
using buffer_owner = cl_owner<cl_mem, clReleaseMemObject>;
using event_owner = cl_owner<cl_event, clReleaseEvent>;

std::string device_string(cl_device_id device, cl_device_info parameter) {
    std::size_t bytes = 0;
    require(
        clGetDeviceInfo(device, parameter, 0, nullptr, &bytes),
        "clGetDeviceInfo(size)");
    if (bytes == 0) {
        return {};
    }
    std::string result(bytes, '\0');
    require(
        clGetDeviceInfo(device, parameter, result.size(), result.data(), nullptr),
        "clGetDeviceInfo(value)");
    while (!result.empty() && result.back() == '\0') {
        result.pop_back();
    }
    return result;
}

std::pair<cl_platform_id, cl_device_id> select_intel_gpu() {
    cl_uint platform_count = 0;
    require(clGetPlatformIDs(0, nullptr, &platform_count), "clGetPlatformIDs(count)");
    if (platform_count == 0) {
        throw std::runtime_error("no OpenCL platforms found");
    }
    std::vector<cl_platform_id> platforms(platform_count);
    require(
        clGetPlatformIDs(platform_count, platforms.data(), nullptr),
        "clGetPlatformIDs(values)");
    for (cl_platform_id platform : platforms) {
        cl_uint device_count = 0;
        const cl_int count_error =
            clGetDeviceIDs(platform, CL_DEVICE_TYPE_GPU, 0, nullptr, &device_count);
        if (count_error == CL_DEVICE_NOT_FOUND) {
            continue;
        }
        require(count_error, "clGetDeviceIDs(count)");
        std::vector<cl_device_id> devices(device_count);
        require(
            clGetDeviceIDs(
                platform,
                CL_DEVICE_TYPE_GPU,
                device_count,
                devices.data(),
                nullptr),
            "clGetDeviceIDs(values)");
        for (cl_device_id device : devices) {
            cl_uint vendor = 0;
            require(
                clGetDeviceInfo(
                    device,
                    CL_DEVICE_VENDOR_ID,
                    sizeof(vendor),
                    &vendor,
                    nullptr),
                "clGetDeviceInfo(vendor)");
            if (vendor == kIntelVendor) {
                return {platform, device};
            }
        }
    }
    throw std::runtime_error("no Intel OpenCL GPU found");
}

std::string build_log(cl_program program, cl_device_id device) {
    std::size_t bytes = 0;
    if (clGetProgramBuildInfo(
            program,
            device,
            CL_PROGRAM_BUILD_LOG,
            0,
            nullptr,
            &bytes) != CL_SUCCESS ||
        bytes == 0) {
        return {};
    }
    std::string result(bytes, '\0');
    if (clGetProgramBuildInfo(
            program,
            device,
            CL_PROGRAM_BUILD_LOG,
            result.size(),
            result.data(),
            nullptr) != CL_SUCCESS) {
        return {};
    }
    while (!result.empty() && result.back() == '\0') {
        result.pop_back();
    }
    return result;
}

} // namespace

struct intel_igc_projector::implementation {
    implementation(
        const std::filesystem::path & spirv_path,
        const void * native_weights,
        std::size_t native_weight_bytes) {
        if (native_weights == nullptr || native_weight_bytes == 0) {
            throw std::runtime_error("empty TRUEGA native weight mapping");
        }
        const auto [selected_platform, selected_device] = select_intel_gpu();
        platform = selected_platform;
        device = selected_device;
        name = device_string(device, CL_DEVICE_NAME);
        const std::string il_version = device_string(device, CL_DEVICE_IL_VERSION);
        if (il_version.find("SPIR-V") == std::string::npos) {
            throw std::runtime_error(
                "Intel GPU does not advertise SPIR-V ingestion: " + il_version);
        }

        cl_int error = CL_SUCCESS;
        const std::array<cl_context_properties, 3> context_properties = {
            CL_CONTEXT_PLATFORM,
            reinterpret_cast<cl_context_properties>(platform),
            0,
        };
        context = context_owner(clCreateContext(
            context_properties.data(), 1, &device, nullptr, nullptr, &error));
        require(error, "clCreateContext");

        const std::array<cl_queue_properties, 3> queue_properties = {
            CL_QUEUE_PROPERTIES,
            CL_QUEUE_PROFILING_ENABLE,
            0,
        };
        queue = queue_owner(clCreateCommandQueueWithProperties(
            context.get(), device, queue_properties.data(), &error));
        require(error, "clCreateCommandQueueWithProperties");

        const auto spirv = read_binary(spirv_path);
        program = program_owner(clCreateProgramWithIL(
            context.get(), spirv.data(), spirv.size(), &error));
        require(error, "clCreateProgramWithIL");
        error = clBuildProgram(program.get(), 1, &device, nullptr, nullptr, nullptr);
        if (error != CL_SUCCESS) {
            throw std::runtime_error(
                "Intel IGC SPIR-V build failed with OpenCL error " +
                std::to_string(error) + ": " + build_log(program.get(), device));
        }

        kernel = kernel_owner(clCreateKernel(program.get(), "lfm25_q8_project", &error));
        require(error, "clCreateKernel(lfm25_q8_project)");

        weights = buffer_owner(clCreateBuffer(
            context.get(),
            CL_MEM_READ_ONLY | CL_MEM_USE_HOST_PTR,
            native_weight_bytes,
            const_cast<void *>(native_weights),
            &error));
        require(error, "clCreateBuffer(weights)");
        activation = buffer_owner(clCreateBuffer(
            context.get(),
            CL_MEM_READ_ONLY,
            kMaximumActivationBytes,
            nullptr,
            &error));
        require(error, "clCreateBuffer(activation)");
        output = buffer_owner(clCreateBuffer(
            context.get(),
            CL_MEM_WRITE_ONLY,
            kMaximumOutputElements * sizeof(float),
            nullptr,
            &error));
        require(error, "clCreateBuffer(output)");

        cl_mem weights_handle = weights.get();
        cl_mem activation_handle = activation.get();
        cl_mem output_handle = output.get();
        require(
            clSetKernelArg(kernel.get(), 0, sizeof(weights_handle), &weights_handle),
            "clSetKernelArg(weights)");
        require(
            clSetKernelArg(kernel.get(), 1, sizeof(activation_handle), &activation_handle),
            "clSetKernelArg(activation)");
        require(
            clSetKernelArg(kernel.get(), 2, sizeof(output_handle), &output_handle),
            "clSetKernelArg(output)");
    }

    std::vector<float> project(
        std::uint32_t weight_offset,
        std::uint32_t columns,
        std::uint32_t rows,
        std::span<const std::byte> q8_activation) {
        const std::size_t expected_activation =
            static_cast<std::size_t>(columns) / 32 * 34;
        if (q8_activation.size() != expected_activation ||
            q8_activation.size() > kMaximumActivationBytes ||
            rows == 0 ||
            rows > kMaximumOutputElements ||
            rows % kLocalSize != 0) {
            throw std::runtime_error("Intel IGC projection shape rejected");
        }
        require(
            clEnqueueWriteBuffer(
                queue.get(),
                activation.get(),
                CL_TRUE,
                0,
                q8_activation.size(),
                q8_activation.data(),
                0,
                nullptr,
                nullptr),
            "clEnqueueWriteBuffer(activation)");
        require(
            clSetKernelArg(
                kernel.get(), 3, sizeof(weight_offset), &weight_offset),
            "clSetKernelArg(weight_offset)");
        require(
            clSetKernelArg(kernel.get(), 4, sizeof(columns), &columns),
            "clSetKernelArg(columns)");
        require(
            clSetKernelArg(kernel.get(), 5, sizeof(rows), &rows),
            "clSetKernelArg(rows)");

        const std::size_t global = rows;
        const std::size_t local = kLocalSize;
        cl_event raw_event = nullptr;
        require(
            clEnqueueNDRangeKernel(
                queue.get(),
                kernel.get(),
                1,
                nullptr,
                &global,
                &local,
                0,
                nullptr,
                &raw_event),
            "clEnqueueNDRangeKernel(lfm25_q8_project)");
        event_owner event(raw_event);

        std::vector<float> result(rows);
        require(
            clEnqueueReadBuffer(
                queue.get(),
                output.get(),
                CL_TRUE,
                0,
                result.size() * sizeof(float),
                result.data(),
                0,
                nullptr,
                nullptr),
            "clEnqueueReadBuffer(output)");
        cl_ulong started = 0;
        cl_ulong finished = 0;
        require(
            clGetEventProfilingInfo(
                event.get(),
                CL_PROFILING_COMMAND_START,
                sizeof(started),
                &started,
                nullptr),
            "clGetEventProfilingInfo(start)");
        require(
            clGetEventProfilingInfo(
                event.get(),
                CL_PROFILING_COMMAND_END,
                sizeof(finished),
                &finished,
                nullptr),
            "clGetEventProfilingInfo(end)");
        if (finished >= started) {
            kernel_ns += finished - started;
        }
        ++launch_count;
        return result;
    }

    cl_platform_id platform = nullptr;
    cl_device_id device = nullptr;
    std::string name;
    context_owner context;
    queue_owner queue;
    program_owner program;
    kernel_owner kernel;
    buffer_owner weights;
    buffer_owner activation;
    buffer_owner output;
    std::uint64_t launch_count = 0;
    std::uint64_t kernel_ns = 0;
};

intel_igc_projector::intel_igc_projector(
    const std::filesystem::path & spirv_path,
    const void * native_weights,
    std::size_t native_weight_bytes)
    : implementation_(std::make_unique<implementation>(
          spirv_path, native_weights, native_weight_bytes)) {}

intel_igc_projector::~intel_igc_projector() = default;

std::vector<float> intel_igc_projector::project(
    std::uint32_t weight_offset,
    std::uint32_t columns,
    std::uint32_t rows,
    std::span<const std::byte> q8_activation) {
    return implementation_->project(
        weight_offset, columns, rows, q8_activation);
}

const std::string & intel_igc_projector::device_name() const {
    return implementation_->name;
}

std::uint64_t intel_igc_projector::launches() const {
    return implementation_->launch_count;
}

std::uint64_t intel_igc_projector::kernel_nanoseconds() const {
    return implementation_->kernel_ns;
}

} // namespace trueos::lfm25
