#define CL_TARGET_OPENCL_VERSION 300

#include "lfm25_igpu.hpp"

#include <CL/cl.h>
#include <openssl/evp.h>

#include <array>
#include <fstream>
#include <memory>
#include <stdexcept>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

namespace trueos::lfm25 {
namespace {

constexpr cl_uint kIntelVendor = 0x8086;
constexpr std::size_t kMaximumActivationBytes = 5'184;
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

std::string platform_string(cl_platform_id platform, cl_platform_info parameter) {
    std::size_t bytes = 0;
    require(
        clGetPlatformInfo(platform, parameter, 0, nullptr, &bytes),
        "clGetPlatformInfo(size)");
    if (bytes == 0) {
        return {};
    }
    std::string result(bytes, '\0');
    require(
        clGetPlatformInfo(
            platform, parameter, result.size(), result.data(), nullptr),
        "clGetPlatformInfo(value)");
    while (!result.empty() && result.back() == '\0') {
        result.pop_back();
    }
    return result;
}

std::string sha256(std::span<const unsigned char> bytes) {
    std::unique_ptr<EVP_MD_CTX, decltype(&EVP_MD_CTX_free)> context(
        EVP_MD_CTX_new(), EVP_MD_CTX_free);
    if (!context ||
        EVP_DigestInit_ex(context.get(), EVP_sha256(), nullptr) != 1 ||
        EVP_DigestUpdate(context.get(), bytes.data(), bytes.size()) != 1) {
        throw std::runtime_error("cannot hash Intel IGC program binary");
    }
    std::array<unsigned char, EVP_MAX_MD_SIZE> digest{};
    unsigned int digest_bytes = 0;
    if (EVP_DigestFinal_ex(context.get(), digest.data(), &digest_bytes) != 1 ||
        digest_bytes != 32) {
        throw std::runtime_error("cannot finalize Intel IGC program binary hash");
    }
    constexpr char digits[] = "0123456789abcdef";
    std::string result(digest_bytes * 2, '\0');
    for (unsigned int index = 0; index < digest_bytes; ++index) {
        result[index * 2] = digits[digest[index] >> 4];
        result[index * 2 + 1] = digits[digest[index] & 0x0f];
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
        std::size_t native_weight_bytes,
        std::span<const packed_q8_tensor_spec> packed_tensors)
        : model_bytes(native_weight_bytes) {
        if (native_weights == nullptr || native_weight_bytes == 0) {
            throw std::runtime_error("empty LFM native weight mapping");
        }
        if (packed_tensors.empty()) {
            throw std::runtime_error("Intel IGC weight-layout contract rejected");
        }
        const auto [selected_platform, selected_device] = select_intel_gpu();
        platform = selected_platform;
        device = selected_device;
        name = device_string(device, CL_DEVICE_NAME);
        platform_name = platform_string(platform, CL_PLATFORM_NAME);
        driver_version = device_string(device, CL_DRIVER_VERSION);
        il_version = device_string(device, CL_DEVICE_IL_VERSION);
        extensions = device_string(device, CL_DEVICE_EXTENSIONS);
        if (il_version.find("SPIR-V") == std::string::npos) {
            throw std::runtime_error(
                "Intel GPU does not advertise SPIR-V ingestion: " + il_version);
        }
        if (extensions.find("cl_khr_integer_dot_product") == std::string::npos) {
            throw std::runtime_error(
                "Intel GPU does not advertise packed integer dot products");
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
        cl_program_binary_type binary_type = CL_PROGRAM_BINARY_TYPE_NONE;
        require(
            clGetProgramBuildInfo(
                program.get(),
                device,
                CL_PROGRAM_BINARY_TYPE,
                sizeof(binary_type),
                &binary_type,
                nullptr),
            "clGetProgramBuildInfo(binary type)");
        if (binary_type != CL_PROGRAM_BINARY_TYPE_EXECUTABLE) {
            throw std::runtime_error(
                "Intel IGC did not return an executable program binary");
        }
        require(
            clGetProgramInfo(
                program.get(),
                CL_PROGRAM_BINARY_SIZES,
                sizeof(program_binary_size),
                &program_binary_size,
                nullptr),
            "clGetProgramInfo(binary size)");
        if (program_binary_size == 0) {
            throw std::runtime_error("Intel IGC returned an empty program binary");
        }
        std::vector<unsigned char> program_binary(program_binary_size);
        unsigned char * binary_pointer = program_binary.data();
        require(
            clGetProgramInfo(
                program.get(),
                CL_PROGRAM_BINARIES,
                sizeof(binary_pointer),
                &binary_pointer,
                nullptr),
            "clGetProgramInfo(binary)");
        program_binary_digest = sha256(program_binary);

        kernel = kernel_owner(clCreateKernel(program.get(), "lfm25_q8_project_packed", &error));
        require(error, "clCreateKernel(fixed LFM25 projection)");

        packed_model = pack_q8x16_model(
            {
                static_cast<const std::byte *>(native_weights),
                native_weight_bytes,
            },
            packed_tensors);
        void * weight_storage = packed_model.bytes.data();
        layout_name = "pair1088-x16-dp4a";

        weights = buffer_owner(clCreateBuffer(
            context.get(),
            CL_MEM_READ_ONLY | CL_MEM_COPY_HOST_PTR,
            native_weight_bytes,
            weight_storage,
            &error));
        require(error, "clCreateBuffer(weights)");
        std::vector<std::byte>().swap(packed_model.bytes);
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
            rows == 0 ||
            rows > kMaximumOutputElements ||
            rows % kLocalSize != 0) {
            throw std::runtime_error("Intel IGC projection shape rejected");
        }
        const auto packed_activation = pack_q8x16_activation(q8_activation, columns);
        const auto activation_payload =
            std::as_bytes(std::span<const std::uint32_t>(packed_activation));
        if (activation_payload.size() > kMaximumActivationBytes) {
            throw std::runtime_error("Intel IGC activation allocation exceeded");
        }
        require(
            clEnqueueWriteBuffer(
                queue.get(),
                activation.get(),
                CL_TRUE,
                0,
                activation_payload.size(),
                activation_payload.data(),
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
            "clEnqueueNDRangeKernel(lfm25_q8_project_packed)");
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
        weight_bytes +=
            static_cast<std::uint64_t>(rows)
            * (static_cast<std::uint64_t>(columns) / 32U)
            * 34U;
        ++launch_count;
        return result;
    }

    cl_platform_id platform = nullptr;
    cl_device_id device = nullptr;
    std::string name;
    std::string platform_name;
    std::string driver_version;
    std::string il_version;
    std::string extensions;
    std::string layout_name;
    std::string program_binary_digest;
    std::size_t program_binary_size = 0;
    context_owner context;
    queue_owner queue;
    program_owner program;
    kernel_owner kernel;
    buffer_owner weights;
    buffer_owner activation;
    buffer_owner output;
    std::size_t model_bytes = 0;
    packed_q8_model packed_model;
    std::uint64_t launch_count = 0;
    std::uint64_t kernel_ns = 0;
    std::uint64_t weight_bytes = 0;
};

intel_igc_projector::intel_igc_projector(
    const std::filesystem::path & spirv_path,
    const void * native_weights,
    std::size_t native_weight_bytes,
    std::span<const packed_q8_tensor_spec> packed_tensors)
    : implementation_(std::make_unique<implementation>(
          spirv_path,
          native_weights,
          native_weight_bytes,
          packed_tensors)) {}

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

const std::string & intel_igc_projector::platform_name() const {
    return implementation_->platform_name;
}

const std::string & intel_igc_projector::driver_version() const {
    return implementation_->driver_version;
}

const std::string & intel_igc_projector::il_version() const {
    return implementation_->il_version;
}

std::size_t intel_igc_projector::program_binary_bytes() const {
    return implementation_->program_binary_size;
}

const std::string & intel_igc_projector::program_binary_sha256() const {
    return implementation_->program_binary_digest;
}

std::uint64_t intel_igc_projector::launches() const {
    return implementation_->launch_count;
}

std::uint64_t intel_igc_projector::kernel_nanoseconds() const {
    return implementation_->kernel_ns;
}

std::uint64_t intel_igc_projector::projected_weight_bytes() const {
    return implementation_->weight_bytes;
}

const std::string & intel_igc_projector::weight_layout() const {
    return implementation_->layout_name;
}

std::size_t intel_igc_projector::resident_model_bytes() const {
    return implementation_->model_bytes;
}

std::uint64_t intel_igc_projector::packed_subnormal_scales() const {
    return implementation_->packed_model.subnormal_scales;
}

const std::string & intel_igc_projector::packed_model_sha256() const {
    return implementation_->packed_model.sha256;
}

} // namespace trueos::lfm25
