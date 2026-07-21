#include <cublas_v2.h>
#include <cuda_runtime.h>
#if LUMEN_HAS_CUDNN
#include <cudnn.h>
#endif

#include <cmath>
#include <cstdint>
#include <mutex>
#include <sstream>
#include <string>
#include <unordered_map>
#include <utility>
#include <vector>

namespace {

thread_local std::string g_last_error;

constexpr int kUnaryRelu = 0;
constexpr int kUnarySigmoid = 1;
constexpr int kUnaryTanh = 2;
constexpr int kUnarySilu = 3;
constexpr int kUnaryGelu = 4;

constexpr int kBinaryAdd = 0;
constexpr int kBinarySub = 1;
constexpr int kBinaryMul = 2;

float* handle_to_ptr(uint64_t handle) {
    return reinterpret_cast<float*>(handle);
}

void set_error(const std::string& message) {
    g_last_error = message;
}

void set_cuda_error(const char* prefix, cudaError_t status) {
    std::ostringstream oss;
    oss << prefix << ": " << cudaGetErrorString(status);
    set_error(oss.str());
}

void set_cublas_error(const char* prefix, cublasStatus_t status) {
    std::ostringstream oss;
    oss << prefix << ": cuBLAS status " << static_cast<int>(status);
    set_error(oss.str());
}

#if LUMEN_HAS_CUDNN
void set_cudnn_error(const char* prefix, cudnnStatus_t status) {
    std::ostringstream oss;
    oss << prefix << ": " << cudnnGetErrorString(status);
    set_error(oss.str());
}
#endif

struct CublasHandle {
    cublasHandle_t handle = nullptr;
    bool owns = true;

    ~CublasHandle() {
        if (owns && handle != nullptr) {
            cublasDestroy(handle);
        }
    }
};

#if LUMEN_HAS_CUDNN
struct CudnnHandle {
    cudnnHandle_t handle = nullptr;
    bool owns = true;

    ~CudnnHandle() {
        if (owns && handle != nullptr) {
            cudnnDestroy(handle);
        }
    }
};

struct CudnnTensorDescriptor {
    cudnnTensorDescriptor_t desc = nullptr;

    ~CudnnTensorDescriptor() {
        if (desc != nullptr) {
            cudnnDestroyTensorDescriptor(desc);
        }
    }
};

struct CudnnActivationDescriptor {
    cudnnActivationDescriptor_t desc = nullptr;

    ~CudnnActivationDescriptor() {
        if (desc != nullptr) {
            cudnnDestroyActivationDescriptor(desc);
        }
    }
};

struct CudnnFilterDescriptor {
    cudnnFilterDescriptor_t desc = nullptr;

    ~CudnnFilterDescriptor() {
        if (desc != nullptr) {
            cudnnDestroyFilterDescriptor(desc);
        }
    }
};

struct CudnnConvolutionDescriptor {
    cudnnConvolutionDescriptor_t desc = nullptr;

    ~CudnnConvolutionDescriptor() {
        if (desc != nullptr) {
            cudnnDestroyConvolutionDescriptor(desc);
        }
    }
};
#endif

struct CudaWorkspace {
    void* ptr = nullptr;

    ~CudaWorkspace() {
        if (ptr != nullptr) {
            cudaFree(ptr);
        }
    }

    bool allocate(size_t bytes, const char* context) {
        if (bytes == 0) {
            return true;
        }
        cudaError_t status = cudaMalloc(&ptr, bytes);
        if (status != cudaSuccess) {
            set_cuda_error(context, status);
            return false;
        }
        return true;
    }
};

struct ReusableCudaWorkspace {
    void* ptr = nullptr;
    size_t capacity = 0;

    ~ReusableCudaWorkspace() {
        if (ptr != nullptr) {
            cudaFree(ptr);
        }
    }

    bool ensure(size_t bytes, const char* context) {
        if (bytes <= capacity) {
            return true;
        }
        if (ptr != nullptr) {
            cudaFree(ptr);
            ptr = nullptr;
            capacity = 0;
        }
        if (bytes == 0) {
            return true;
        }
        cudaError_t status = cudaMalloc(&ptr, bytes);
        if (status != cudaSuccess) {
            set_cuda_error(context, status);
            return false;
        }
        capacity = bytes;
        return true;
    }
};

struct CudaBufferPool {
    std::mutex mutex;
    std::unordered_map<int, std::unordered_map<size_t, std::vector<void*>>> free_lists_by_device;
    std::unordered_map<int, size_t> cached_bytes_by_device;
};

constexpr size_t kMaxCudaBufferPoolBytes = 256ull * 1024ull * 1024ull;
constexpr size_t kMaxPooledCudaBufferBytes = 64ull * 1024ull * 1024ull;

CudaBufferPool& cuda_buffer_pool() {
    static CudaBufferPool* pool = new CudaBufferPool();
    return *pool;
}

bool is_poolable_cuda_buffer(size_t bytes) {
    return bytes > 0 && bytes <= kMaxPooledCudaBufferBytes;
}

bool current_cuda_device(int& device) {
    cudaError_t status = cudaGetDevice(&device);
    return status == cudaSuccess;
}

bool cuda_pointer_device(void* ptr, int& device) {
    cudaPointerAttributes attrs;
    cudaError_t status = cudaPointerGetAttributes(&attrs, ptr);
    if (status != cudaSuccess) {
        cudaGetLastError();
        return false;
    }
    device = attrs.device;
    return true;
}

void free_cuda_ptr_on_device(void* ptr, int device) {
    int current = 0;
    bool restore = current_cuda_device(current) && current != device;
    if (restore) {
        cudaSetDevice(device);
    }
    cudaFree(ptr);
    if (restore) {
        cudaSetDevice(current);
    }
}

bool try_take_pooled_cuda_buffer(size_t bytes, void** out) {
    if (!is_poolable_cuda_buffer(bytes)) {
        return false;
    }

    int device = 0;
    if (!current_cuda_device(device)) {
        return false;
    }

    CudaBufferPool& pool = cuda_buffer_pool();
    std::lock_guard<std::mutex> lock(pool.mutex);
    auto device_it = pool.free_lists_by_device.find(device);
    if (device_it == pool.free_lists_by_device.end()) {
        return false;
    }
    auto size_it = device_it->second.find(bytes);
    if (size_it == device_it->second.end() || size_it->second.empty()) {
        return false;
    }

    *out = size_it->second.back();
    size_it->second.pop_back();
    auto cached_it = pool.cached_bytes_by_device.find(device);
    if (cached_it != pool.cached_bytes_by_device.end() && cached_it->second >= bytes) {
        cached_it->second -= bytes;
    }
    return true;
}

void release_cuda_buffer(uint64_t handle, size_t len) {
    if (handle == 0) {
        return;
    }

    if (len > static_cast<size_t>(-1) / sizeof(float)) {
        cudaFree(handle_to_ptr(handle));
        return;
    }
    size_t bytes = len * sizeof(float);
    if (is_poolable_cuda_buffer(bytes)) {
        int device = 0;
        void* ptr = handle_to_ptr(handle);
        if (!cuda_pointer_device(ptr, device)) {
            cudaFree(ptr);
            return;
        }

        CudaBufferPool& pool = cuda_buffer_pool();
        std::lock_guard<std::mutex> lock(pool.mutex);
        size_t cached_bytes = pool.cached_bytes_by_device[device];
        if (cached_bytes <= kMaxCudaBufferPoolBytes &&
            bytes <= kMaxCudaBufferPoolBytes - cached_bytes) {
            pool.free_lists_by_device[device][bytes].push_back(ptr);
            pool.cached_bytes_by_device[device] += bytes;
            return;
        }
    }

    cudaFree(handle_to_ptr(handle));
}

void clear_cuda_buffer_pool() {
    std::vector<std::pair<int, void*>> to_free;
    CudaBufferPool& pool = cuda_buffer_pool();
    {
        std::lock_guard<std::mutex> lock(pool.mutex);
        for (auto& device_entry : pool.free_lists_by_device) {
            int device = device_entry.first;
            for (auto& size_entry : device_entry.second) {
                for (void* ptr : size_entry.second) {
                    to_free.push_back({device, ptr});
                }
            }
        }
        pool.free_lists_by_device.clear();
        pool.cached_bytes_by_device.clear();
    }

    for (auto& entry : to_free) {
        free_cuda_ptr_on_device(entry.second, entry.first);
    }
}

bool validate_dims(size_t m, size_t n, size_t k) {
    if (m == 0 || n == 0 || k == 0) {
        set_error("CUDA matmul dimensions must be greater than zero");
        return false;
    }
    return true;
}

bool init_cublas(CublasHandle& handle) {
    thread_local CublasHandle cached;
    if (cached.handle == nullptr) {
        cublasStatus_t status = cublasCreate(&cached.handle);
        if (status != CUBLAS_STATUS_SUCCESS) {
            set_cublas_error("failed to create cuBLAS handle", status);
            return false;
        }
    }
    handle.handle = cached.handle;
    handle.owns = false;
    return true;
}

#if LUMEN_HAS_CUDNN
bool init_cudnn(CudnnHandle& handle) {
    thread_local CudnnHandle cached;
    if (cached.handle == nullptr) {
        cudnnStatus_t status = cudnnCreate(&cached.handle);
        if (status != CUDNN_STATUS_SUCCESS) {
            set_cudnn_error("failed to create cuDNN handle", status);
            return false;
        }
    }
    handle.handle = cached.handle;
    handle.owns = false;
    return true;
}

bool init_tensor_descriptor_4d(
    CudnnTensorDescriptor& desc,
    int n,
    int c,
    int h,
    int w) {
    cudnnStatus_t status = cudnnCreateTensorDescriptor(&desc.desc);
    if (status != CUDNN_STATUS_SUCCESS) {
        set_cudnn_error("failed to create cuDNN tensor descriptor", status);
        return false;
    }
    status = cudnnSetTensor4dDescriptor(desc.desc, CUDNN_TENSOR_NCHW, CUDNN_DATA_FLOAT, n, c, h, w);
    if (status != CUDNN_STATUS_SUCCESS) {
        set_cudnn_error("failed to initialize cuDNN tensor descriptor", status);
        return false;
    }
    return true;
}

bool init_activation_descriptor(
    CudnnActivationDescriptor& desc,
    cudnnActivationMode_t mode) {
    cudnnStatus_t status = cudnnCreateActivationDescriptor(&desc.desc);
    if (status != CUDNN_STATUS_SUCCESS) {
        set_cudnn_error("failed to create cuDNN activation descriptor", status);
        return false;
    }
    status = cudnnSetActivationDescriptor(desc.desc, mode, CUDNN_PROPAGATE_NAN, 0.0);
    if (status != CUDNN_STATUS_SUCCESS) {
        set_cudnn_error("failed to initialize cuDNN activation descriptor", status);
        return false;
    }
    return true;
}

bool init_filter_descriptor_4d(
    CudnnFilterDescriptor& desc,
    int out_channels,
    int in_channels,
    int kernel_h,
    int kernel_w) {
    cudnnStatus_t status = cudnnCreateFilterDescriptor(&desc.desc);
    if (status != CUDNN_STATUS_SUCCESS) {
        set_cudnn_error("failed to create cuDNN filter descriptor", status);
        return false;
    }
    status = cudnnSetFilter4dDescriptor(
        desc.desc,
        CUDNN_DATA_FLOAT,
        CUDNN_TENSOR_NCHW,
        out_channels,
        in_channels,
        kernel_h,
        kernel_w);
    if (status != CUDNN_STATUS_SUCCESS) {
        set_cudnn_error("failed to initialize cuDNN filter descriptor", status);
        return false;
    }
    return true;
}

bool init_convolution_descriptor_2d(
    CudnnConvolutionDescriptor& desc,
    int pad_h,
    int pad_w,
    int stride_h,
    int stride_w) {
    cudnnStatus_t status = cudnnCreateConvolutionDescriptor(&desc.desc);
    if (status != CUDNN_STATUS_SUCCESS) {
        set_cudnn_error("failed to create cuDNN convolution descriptor", status);
        return false;
    }
    status = cudnnSetConvolution2dDescriptor(
        desc.desc,
        pad_h,
        pad_w,
        stride_h,
        stride_w,
        1,
        1,
        CUDNN_CROSS_CORRELATION,
        CUDNN_DATA_FLOAT);
    if (status != CUDNN_STATUS_SUCCESS) {
        set_cudnn_error("failed to initialize cuDNN convolution descriptor", status);
        return false;
    }
    return true;
}

constexpr size_t kMaxCudnnConvWorkspaceBytes = 256ull * 1024ull * 1024ull;

bool workspace_fits(size_t bytes) {
    return bytes <= kMaxCudnnConvWorkspaceBytes;
}

bool select_cudnn_fwd_algo(
    cudnnHandle_t handle,
    cudnnTensorDescriptor_t input_desc,
    cudnnFilterDescriptor_t filter_desc,
    cudnnConvolutionDescriptor_t conv_desc,
    cudnnTensorDescriptor_t output_desc,
    cudnnConvolutionFwdAlgo_t& algo,
    size_t& workspace_bytes) {
    algo = CUDNN_CONVOLUTION_FWD_ALGO_IMPLICIT_GEMM;
    workspace_bytes = 0;

    int max_count = 0;
    cudnnStatus_t status = cudnnGetConvolutionForwardAlgorithmMaxCount(handle, &max_count);
    if (status == CUDNN_STATUS_SUCCESS && max_count > 0) {
        std::vector<cudnnConvolutionFwdAlgoPerf_t> results(static_cast<size_t>(max_count));
        int returned = 0;
        status = cudnnGetConvolutionForwardAlgorithm_v7(
            handle,
            input_desc,
            filter_desc,
            conv_desc,
            output_desc,
            max_count,
            &returned,
            results.data());
        if (status == CUDNN_STATUS_SUCCESS) {
            for (int i = 0; i < returned; ++i) {
                if (results[static_cast<size_t>(i)].status != CUDNN_STATUS_SUCCESS ||
                    !workspace_fits(results[static_cast<size_t>(i)].memory)) {
                    continue;
                }
                size_t bytes = 0;
                cudnnStatus_t workspace_status = cudnnGetConvolutionForwardWorkspaceSize(
                    handle,
                    input_desc,
                    filter_desc,
                    conv_desc,
                    output_desc,
                    results[static_cast<size_t>(i)].algo,
                    &bytes);
                if (workspace_status == CUDNN_STATUS_SUCCESS && workspace_fits(bytes)) {
                    algo = results[static_cast<size_t>(i)].algo;
                    workspace_bytes = bytes;
                    return true;
                }
            }
        }
    }

    status = cudnnGetConvolutionForwardWorkspaceSize(
        handle,
        input_desc,
        filter_desc,
        conv_desc,
        output_desc,
        algo,
        &workspace_bytes);
    if (status != CUDNN_STATUS_SUCCESS) {
        set_cudnn_error("failed to query cuDNN conv2d forward workspace", status);
        return false;
    }
    if (!workspace_fits(workspace_bytes)) {
        set_error("cuDNN conv2d forward workspace exceeds the configured limit");
        return false;
    }
    return true;
}

bool select_cudnn_bwd_data_algo(
    cudnnHandle_t handle,
    cudnnFilterDescriptor_t filter_desc,
    cudnnTensorDescriptor_t grad_output_desc,
    cudnnConvolutionDescriptor_t conv_desc,
    cudnnTensorDescriptor_t grad_input_desc,
    cudnnConvolutionBwdDataAlgo_t& algo,
    size_t& workspace_bytes) {
    algo = CUDNN_CONVOLUTION_BWD_DATA_ALGO_0;
    workspace_bytes = 0;

    int max_count = 0;
    cudnnStatus_t status = cudnnGetConvolutionBackwardDataAlgorithmMaxCount(handle, &max_count);
    if (status == CUDNN_STATUS_SUCCESS && max_count > 0) {
        std::vector<cudnnConvolutionBwdDataAlgoPerf_t> results(static_cast<size_t>(max_count));
        int returned = 0;
        status = cudnnGetConvolutionBackwardDataAlgorithm_v7(
            handle,
            filter_desc,
            grad_output_desc,
            conv_desc,
            grad_input_desc,
            max_count,
            &returned,
            results.data());
        if (status == CUDNN_STATUS_SUCCESS) {
            for (int i = 0; i < returned; ++i) {
                if (results[static_cast<size_t>(i)].status != CUDNN_STATUS_SUCCESS ||
                    !workspace_fits(results[static_cast<size_t>(i)].memory)) {
                    continue;
                }
                size_t bytes = 0;
                cudnnStatus_t workspace_status = cudnnGetConvolutionBackwardDataWorkspaceSize(
                    handle,
                    filter_desc,
                    grad_output_desc,
                    conv_desc,
                    grad_input_desc,
                    results[static_cast<size_t>(i)].algo,
                    &bytes);
                if (workspace_status == CUDNN_STATUS_SUCCESS && workspace_fits(bytes)) {
                    algo = results[static_cast<size_t>(i)].algo;
                    workspace_bytes = bytes;
                    return true;
                }
            }
        }
    }

    status = cudnnGetConvolutionBackwardDataWorkspaceSize(
        handle,
        filter_desc,
        grad_output_desc,
        conv_desc,
        grad_input_desc,
        algo,
        &workspace_bytes);
    if (status != CUDNN_STATUS_SUCCESS) {
        set_cudnn_error("failed to query cuDNN conv2d backward data workspace", status);
        return false;
    }
    if (!workspace_fits(workspace_bytes)) {
        set_error("cuDNN conv2d backward data workspace exceeds the configured limit");
        return false;
    }
    return true;
}

bool select_cudnn_bwd_filter_algo(
    cudnnHandle_t handle,
    cudnnTensorDescriptor_t input_desc,
    cudnnTensorDescriptor_t grad_output_desc,
    cudnnConvolutionDescriptor_t conv_desc,
    cudnnFilterDescriptor_t grad_weight_desc,
    cudnnConvolutionBwdFilterAlgo_t& algo,
    size_t& workspace_bytes) {
    algo = CUDNN_CONVOLUTION_BWD_FILTER_ALGO_0;
    workspace_bytes = 0;

    int max_count = 0;
    cudnnStatus_t status = cudnnGetConvolutionBackwardFilterAlgorithmMaxCount(handle, &max_count);
    if (status == CUDNN_STATUS_SUCCESS && max_count > 0) {
        std::vector<cudnnConvolutionBwdFilterAlgoPerf_t> results(static_cast<size_t>(max_count));
        int returned = 0;
        status = cudnnGetConvolutionBackwardFilterAlgorithm_v7(
            handle,
            input_desc,
            grad_output_desc,
            conv_desc,
            grad_weight_desc,
            max_count,
            &returned,
            results.data());
        if (status == CUDNN_STATUS_SUCCESS) {
            for (int i = 0; i < returned; ++i) {
                if (results[static_cast<size_t>(i)].status != CUDNN_STATUS_SUCCESS ||
                    !workspace_fits(results[static_cast<size_t>(i)].memory)) {
                    continue;
                }
                size_t bytes = 0;
                cudnnStatus_t workspace_status = cudnnGetConvolutionBackwardFilterWorkspaceSize(
                    handle,
                    input_desc,
                    grad_output_desc,
                    conv_desc,
                    grad_weight_desc,
                    results[static_cast<size_t>(i)].algo,
                    &bytes);
                if (workspace_status == CUDNN_STATUS_SUCCESS && workspace_fits(bytes)) {
                    algo = results[static_cast<size_t>(i)].algo;
                    workspace_bytes = bytes;
                    return true;
                }
            }
        }
    }

    status = cudnnGetConvolutionBackwardFilterWorkspaceSize(
        handle,
        input_desc,
        grad_output_desc,
        conv_desc,
        grad_weight_desc,
        algo,
        &workspace_bytes);
    if (status != CUDNN_STATUS_SUCCESS) {
        set_cudnn_error("failed to query cuDNN conv2d backward filter workspace", status);
        return false;
    }
    if (!workspace_fits(workspace_bytes)) {
        set_error("cuDNN conv2d backward filter workspace exceeds the configured limit");
        return false;
    }
    return true;
}

bool cudnn_activation_mode_for_op(int op, cudnnActivationMode_t& mode) {
    switch (op) {
        case kUnaryRelu:
            mode = CUDNN_ACTIVATION_RELU;
            return true;
        case kUnarySigmoid:
            mode = CUDNN_ACTIVATION_SIGMOID;
            return true;
        case kUnaryTanh:
            mode = CUDNN_ACTIVATION_TANH;
            return true;
#ifdef CUDNN_ACTIVATION_SWISH
        case kUnarySilu:
            mode = CUDNN_ACTIVATION_SWISH;
            return true;
#endif
        default:
            return false;
    }
}
#endif

bool sync_cuda(const char* context) {
    cudaError_t status = cudaDeviceSynchronize();
    if (status != cudaSuccess) {
        set_cuda_error(context, status);
        return false;
    }
    return true;
}

bool upload_size_metadata(
    const char* context,
    const size_t* host,
    size_t len,
    size_t** device) {
    cudaError_t status = cudaMalloc(reinterpret_cast<void**>(device), len * sizeof(size_t));
    if (status != cudaSuccess) {
        std::ostringstream oss;
        oss << "failed to allocate CUDA " << context;
        set_cuda_error(oss.str().c_str(), status);
        return false;
    }
    status = cudaMemcpy(*device, host, len * sizeof(size_t), cudaMemcpyHostToDevice);
    if (status != cudaSuccess) {
        cudaFree(*device);
        *device = nullptr;
        std::ostringstream oss;
        oss << "failed to upload CUDA " << context;
        set_cuda_error(oss.str().c_str(), status);
        return false;
    }
    return true;
}

__device__ float gelu_approx(float x) {
    constexpr float c = 0.7978845608f;
    constexpr float k = 0.044715f;
    float x3 = x * x * x;
    return 0.5f * x * (1.0f + tanhf(c * (x + k * x3)));
}

__device__ float gelu_approx_grad(float x) {
    constexpr float c = 0.7978845608f;
    constexpr float k = 0.044715f;
    float x2 = x * x;
    float x3 = x2 * x;
    float inner = c * (x + k * x3);
    float tanh_i = tanhf(inner);
    float sech2 = 1.0f - tanh_i * tanh_i;
    return 0.5f * (1.0f + tanh_i) + 0.5f * x * sech2 * c * (1.0f + 3.0f * k * x2);
}

__global__ void argmax_rows_kernel(
    const float* input,
    size_t* out_indices,
    size_t rows,
    size_t cols) {
    constexpr int block_size = 256;
    __shared__ float best_values[block_size];
    __shared__ size_t best_indices[block_size];

    size_t row = blockIdx.x;
    if (row >= rows) {
        return;
    }

    float best_value = -INFINITY;
    size_t best_index = 0;
    const float* row_ptr = input + row * cols;
    for (size_t col = threadIdx.x; col < cols; col += blockDim.x) {
        float value = row_ptr[col];
        if (value > best_value || (value == best_value && col < best_index)) {
            best_value = value;
            best_index = col;
        }
    }

    best_values[threadIdx.x] = best_value;
    best_indices[threadIdx.x] = best_index;
    __syncthreads();

    for (int stride = blockDim.x / 2; stride > 0; stride >>= 1) {
        if (threadIdx.x < stride) {
            float other_value = best_values[threadIdx.x + stride];
            size_t other_index = best_indices[threadIdx.x + stride];
            if (other_value > best_values[threadIdx.x] ||
                (other_value == best_values[threadIdx.x] &&
                 other_index < best_indices[threadIdx.x])) {
                best_values[threadIdx.x] = other_value;
                best_indices[threadIdx.x] = other_index;
            }
        }
        __syncthreads();
    }

    if (threadIdx.x == 0) {
        out_indices[row] = best_indices[0];
    }
}

__global__ void unary_kernel(const float* input, float* out, size_t len, int op) {
    size_t idx = blockIdx.x * blockDim.x + threadIdx.x;
    if (idx >= len) {
        return;
    }

    float x = input[idx];
    float value = x;
    switch (op) {
        case kUnaryRelu:
            value = x > 0.0f ? x : 0.0f;
            break;
        case kUnarySigmoid:
            value = 1.0f / (1.0f + expf(-x));
            break;
        case kUnaryTanh:
            value = tanhf(x);
            break;
        case kUnarySilu: {
            float sig = 1.0f / (1.0f + expf(-x));
            value = x * sig;
            break;
        }
        case kUnaryGelu:
            value = gelu_approx(x);
            break;
        default:
            value = x;
            break;
    }
    out[idx] = value;
}

__global__ void unary_backward_kernel(
    const float* input,
    const float* output,
    const float* grad,
    float* out,
    size_t len,
    int op) {
    size_t idx = blockIdx.x * blockDim.x + threadIdx.x;
    if (idx >= len) {
        return;
    }

    float x = input[idx];
    float y = output[idx];
    float g = grad[idx];
    float dx = g;
    switch (op) {
        case kUnaryRelu:
            dx = x > 0.0f ? g : 0.0f;
            break;
        case kUnarySigmoid:
            dx = g * y * (1.0f - y);
            break;
        case kUnaryTanh:
            dx = g * (1.0f - y * y);
            break;
        case kUnarySilu: {
            float sig = 1.0f / (1.0f + expf(-x));
            dx = g * (sig + x * sig * (1.0f - sig));
            break;
        }
        case kUnaryGelu:
            dx = g * gelu_approx_grad(x);
            break;
        default:
            dx = g;
            break;
    }
    out[idx] = dx;
}

__global__ void binary_kernel(const float* lhs, const float* rhs, float* out, size_t len, int op) {
    size_t idx = blockIdx.x * blockDim.x + threadIdx.x;
    if (idx >= len) {
        return;
    }

    switch (op) {
        case kBinaryAdd:
            out[idx] = lhs[idx] + rhs[idx];
            break;
        case kBinarySub:
            out[idx] = lhs[idx] - rhs[idx];
            break;
        case kBinaryMul:
            out[idx] = lhs[idx] * rhs[idx];
            break;
        default:
            out[idx] = lhs[idx];
            break;
    }
}

__global__ void binary_backward_kernel(
    const float* lhs,
    const float* rhs,
    const float* grad,
    float* grad_lhs,
    float* grad_rhs,
    size_t len,
    int op) {
    size_t idx = blockIdx.x * blockDim.x + threadIdx.x;
    if (idx >= len) {
        return;
    }

    float g = grad[idx];
    switch (op) {
        case kBinaryAdd:
            grad_lhs[idx] = g;
            grad_rhs[idx] = g;
            break;
        case kBinarySub:
            grad_lhs[idx] = g;
            grad_rhs[idx] = -g;
            break;
        case kBinaryMul:
            grad_lhs[idx] = g * rhs[idx];
            grad_rhs[idx] = g * lhs[idx];
            break;
        default:
            grad_lhs[idx] = g;
            grad_rhs[idx] = 0.0f;
            break;
    }
}

__global__ void binary_broadcast_kernel(
    const float* lhs,
    const float* rhs,
    float* out,
    const size_t* out_shape,
    const size_t* out_strides,
    const size_t* lhs_shape,
    const size_t* lhs_strides,
    const size_t* rhs_shape,
    const size_t* rhs_strides,
    size_t ndim,
    size_t len,
    int op) {
    size_t out_idx = blockIdx.x * blockDim.x + threadIdx.x;
    if (out_idx >= len) {
        return;
    }

    size_t remaining = out_idx;
    size_t lhs_idx = 0;
    size_t rhs_idx = 0;
    for (size_t i = 0; i < ndim; ++i) {
        size_t coord = remaining / out_strides[i];
        remaining %= out_strides[i];
        if (lhs_shape[i] != 1) {
            lhs_idx += coord * lhs_strides[i];
        }
        if (rhs_shape[i] != 1) {
            rhs_idx += coord * rhs_strides[i];
        }
    }

    float a = lhs[lhs_idx];
    float b = rhs[rhs_idx];
    switch (op) {
        case kBinaryAdd:
            out[out_idx] = a + b;
            break;
        case kBinarySub:
            out[out_idx] = a - b;
            break;
        case kBinaryMul:
            out[out_idx] = a * b;
            break;
        default:
            out[out_idx] = a;
            break;
    }
}

__global__ void binary_broadcast_backward_kernel(
    const float* lhs,
    const float* rhs,
    const float* grad,
    float* grad_lhs,
    float* grad_rhs,
    const size_t* out_shape,
    const size_t* out_strides,
    const size_t* lhs_shape,
    const size_t* lhs_strides,
    const size_t* rhs_shape,
    const size_t* rhs_strides,
    size_t ndim,
    size_t len,
    int op) {
    size_t out_idx = blockIdx.x * blockDim.x + threadIdx.x;
    if (out_idx >= len) {
        return;
    }

    size_t remaining = out_idx;
    size_t lhs_idx = 0;
    size_t rhs_idx = 0;
    for (size_t i = 0; i < ndim; ++i) {
        size_t coord = remaining / out_strides[i];
        remaining %= out_strides[i];
        if (lhs_shape[i] != 1) {
            lhs_idx += coord * lhs_strides[i];
        }
        if (rhs_shape[i] != 1) {
            rhs_idx += coord * rhs_strides[i];
        }
    }

    float g = grad[out_idx];
    switch (op) {
        case kBinaryAdd:
            atomicAdd(grad_lhs + lhs_idx, g);
            atomicAdd(grad_rhs + rhs_idx, g);
            break;
        case kBinarySub:
            atomicAdd(grad_lhs + lhs_idx, g);
            atomicAdd(grad_rhs + rhs_idx, -g);
            break;
        case kBinaryMul:
            atomicAdd(grad_lhs + lhs_idx, g * rhs[rhs_idx]);
            atomicAdd(grad_rhs + rhs_idx, g * lhs[lhs_idx]);
            break;
        default:
            atomicAdd(grad_lhs + lhs_idx, g);
            break;
    }
}

__global__ void sum_kernel(const float* input, float* out, size_t len) {
    __shared__ float shared[256];
    unsigned int tid = threadIdx.x;
    size_t idx = static_cast<size_t>(blockIdx.x) * blockDim.x + threadIdx.x;
    size_t stride = static_cast<size_t>(blockDim.x) * gridDim.x;

    float local = 0.0f;
    for (size_t i = idx; i < len; i += stride) {
        local += input[i];
    }

    shared[tid] = local;
    __syncthreads();

    for (unsigned int offset = blockDim.x / 2; offset > 0; offset >>= 1) {
        if (tid < offset) {
            shared[tid] += shared[tid + offset];
        }
        __syncthreads();
    }

    if (tid == 0) {
        atomicAdd(out, shared[0]);
    }
}

__global__ void fill_scalar_kernel(float* out, size_t len, float value) {
    size_t idx = blockIdx.x * blockDim.x + threadIdx.x;
    if (idx < len) {
        out[idx] = value;
    }
}

__global__ void mse_backward_kernel(
    const float* diff,
    float* grad_output,
    float* grad_target,
    size_t len,
    float factor) {
    size_t idx = blockIdx.x * blockDim.x + threadIdx.x;
    if (idx < len) {
        float grad = diff[idx] * factor;
        grad_output[idx] = grad;
        grad_target[idx] = -grad;
    }
}

__global__ void cross_entropy_backward_kernel(
    const float* softmax,
    const float* target,
    float* out,
    size_t len,
    float factor) {
    size_t idx = blockIdx.x * blockDim.x + threadIdx.x;
    if (idx < len) {
        out[idx] = (softmax[idx] - target[idx]) * factor;
    }
}

__global__ void cross_entropy_loss_kernel(
    const float* softmax,
    const float* target,
    float* out,
    size_t len,
    float factor) {
    __shared__ float shared[256];
    unsigned int tid = threadIdx.x;
    size_t idx = blockIdx.x * blockDim.x + tid;
    size_t stride = blockDim.x * gridDim.x;

    float local = 0.0f;
    constexpr float epsilon = 1.0e-9f;
    while (idx < len) {
        float t = target[idx];
        if (t > 0.0f) {
            local += -t * logf(softmax[idx] + epsilon);
        }
        idx += stride;
    }

    shared[tid] = local;
    __syncthreads();

    for (unsigned int offset = blockDim.x / 2; offset > 0; offset >>= 1) {
        if (tid < offset) {
            shared[tid] += shared[tid + offset];
        }
        __syncthreads();
    }

    if (tid == 0) {
        atomicAdd(out, shared[0] * factor);
    }
}

__global__ void sgd_update_kernel(float* param, const float* grad, size_t len, float lr) {
    size_t idx = blockIdx.x * blockDim.x + threadIdx.x;
    if (idx < len) {
        param[idx] -= lr * grad[idx];
    }
}

__global__ void sgd_momentum_update_kernel(
    float* param,
    const float* grad,
    float* velocity,
    size_t len,
    float lr,
    float momentum) {
    size_t idx = blockIdx.x * blockDim.x + threadIdx.x;
    if (idx < len) {
        float v = momentum * velocity[idx] + grad[idx];
        velocity[idx] = v;
        param[idx] -= lr * v;
    }
}

__global__ void adam_update_kernel(
    float* param,
    const float* grad,
    float* exp_avg,
    float* exp_avg_sq,
    size_t len,
    float lr,
    float beta1,
    float beta2,
    float bias_correction1,
    float bias_correction2,
    float eps) {
    size_t idx = blockIdx.x * blockDim.x + threadIdx.x;
    if (idx >= len) {
        return;
    }

    float g = grad[idx];
    float m = beta1 * exp_avg[idx] + (1.0f - beta1) * g;
    float v = beta2 * exp_avg_sq[idx] + (1.0f - beta2) * g * g;
    exp_avg[idx] = m;
    exp_avg_sq[idx] = v;

    float m_hat = m / bias_correction1;
    float v_hat = v / bias_correction2;
    param[idx] -= lr * (m_hat / (sqrtf(v_hat) + eps));
}

__global__ void softmax_lastdim_kernel(const float* input, float* out, size_t outer, size_t last_dim) {
    size_t row = blockIdx.x * blockDim.x + threadIdx.x;
    if (row >= outer) {
        return;
    }

    const float* row_in = input + row * last_dim;
    float* row_out = out + row * last_dim;

    float max_val = -INFINITY;
    for (size_t j = 0; j < last_dim; ++j) {
        max_val = fmaxf(max_val, row_in[j]);
    }

    float sum_exp = 0.0f;
    for (size_t j = 0; j < last_dim; ++j) {
        float e = expf(row_in[j] - max_val);
        row_out[j] = e;
        sum_exp += e;
    }

    float inv_sum = 1.0f / sum_exp;
    for (size_t j = 0; j < last_dim; ++j) {
        row_out[j] *= inv_sum;
    }
}

__global__ void softmax_lastdim_backward_kernel(
    const float* output,
    const float* grad,
    float* out,
    size_t outer,
    size_t last_dim) {
    size_t row = blockIdx.x * blockDim.x + threadIdx.x;
    if (row >= outer) {
        return;
    }

    const float* row_y = output + row * last_dim;
    const float* row_grad = grad + row * last_dim;
    float* row_out = out + row * last_dim;

    float dot = 0.0f;
    for (size_t j = 0; j < last_dim; ++j) {
        dot += row_y[j] * row_grad[j];
    }

    for (size_t j = 0; j < last_dim; ++j) {
        row_out[j] = row_y[j] * (row_grad[j] - dot);
    }
}

__global__ void fused_softmax_kernel(
    const float* input,
    float* out,
    size_t rows,
    size_t q_len,
    size_t k_len,
    float scale,
    int is_causal) {
    size_t row = blockIdx.x * blockDim.x + threadIdx.x;
    if (row >= rows) {
        return;
    }

    size_t q_idx = row % q_len;
    const float* row_in = input + row * k_len;
    float* row_out = out + row * k_len;

    float max_val = -INFINITY;
    for (size_t j = 0; j < k_len; ++j) {
        bool masked = is_causal != 0 && q_len > 1 && j > q_idx;
        if (masked) {
            continue;
        }
        float value = row_in[j] * scale;
        max_val = fmaxf(max_val, value);
    }

    float sum_exp = 0.0f;
    for (size_t j = 0; j < k_len; ++j) {
        bool masked = is_causal != 0 && q_len > 1 && j > q_idx;
        if (masked) {
            row_out[j] = 0.0f;
            continue;
        }
        float value = expf(row_in[j] * scale - max_val);
        row_out[j] = value;
        sum_exp += value;
    }

    float inv_sum = 1.0f / (sum_exp + 1.0e-10f);
    for (size_t j = 0; j < k_len; ++j) {
        row_out[j] *= inv_sum;
    }
}

__global__ void fused_softmax_with_past_kernel(
    const float* input,
    float* out,
    size_t rows,
    size_t q_len,
    size_t k_len,
    float scale,
    int is_causal,
    size_t past_len) {
    size_t row = blockIdx.x * blockDim.x + threadIdx.x;
    if (row >= rows) {
        return;
    }

    size_t q_idx = row % q_len;
    size_t causal_limit = past_len + q_idx;
    const float* row_in = input + row * k_len;
    float* row_out = out + row * k_len;

    float max_val = -INFINITY;
    for (size_t j = 0; j < k_len; ++j) {
        bool masked = is_causal != 0 && q_len > 1 && j > causal_limit;
        if (masked) {
            continue;
        }
        float value = row_in[j] * scale;
        max_val = fmaxf(max_val, value);
    }

    float sum_exp = 0.0f;
    for (size_t j = 0; j < k_len; ++j) {
        bool masked = is_causal != 0 && q_len > 1 && j > causal_limit;
        if (masked) {
            row_out[j] = 0.0f;
            continue;
        }
        float value = expf(row_in[j] * scale - max_val);
        row_out[j] = value;
        sum_exp += value;
    }

    float inv_sum = 1.0f / (sum_exp + 1.0e-10f);
    for (size_t j = 0; j < k_len; ++j) {
        row_out[j] *= inv_sum;
    }
}

__global__ void fused_softmax_backward_kernel(
    const float* output,
    const float* grad,
    float* out,
    size_t rows,
    size_t k_len,
    float scale) {
    size_t row = blockIdx.x * blockDim.x + threadIdx.x;
    if (row >= rows) {
        return;
    }

    const float* row_y = output + row * k_len;
    const float* row_grad = grad + row * k_len;
    float* row_out = out + row * k_len;

    float dot = 0.0f;
    for (size_t j = 0; j < k_len; ++j) {
        dot += row_y[j] * row_grad[j];
    }

    for (size_t j = 0; j < k_len; ++j) {
        row_out[j] = scale * row_y[j] * (row_grad[j] - dot);
    }
}

__global__ void embedding_kernel(
    const float* indices,
    const float* weight,
    float* out,
    size_t num_indices,
    size_t vocab_size,
    size_t embed_dim,
    int* status) {
    size_t idx = blockIdx.x * blockDim.x + threadIdx.x;
    size_t total = num_indices * embed_dim;
    if (idx >= total) {
        return;
    }

    size_t token_idx = idx / embed_dim;
    size_t col = idx % embed_dim;
    float raw_index = indices[token_idx];
    if (!isfinite(raw_index) || raw_index < 0.0f || floorf(raw_index) != raw_index) {
        atomicCAS(status, 0, 1);
        out[idx] = 0.0f;
        return;
    }

    size_t row = static_cast<size_t>(raw_index);
    if (row >= vocab_size) {
        atomicCAS(status, 0, 2);
        out[idx] = 0.0f;
        return;
    }

    out[idx] = weight[row * embed_dim + col];
}

__global__ void embedding_backward_kernel(
    const float* indices,
    const float* grad,
    float* grad_weight,
    size_t num_indices,
    size_t vocab_size,
    size_t embed_dim,
    int* status) {
    size_t idx = blockIdx.x * blockDim.x + threadIdx.x;
    size_t total = num_indices * embed_dim;
    if (idx >= total) {
        return;
    }

    size_t token_idx = idx / embed_dim;
    size_t col = idx % embed_dim;
    float raw_index = indices[token_idx];
    if (!isfinite(raw_index) || raw_index < 0.0f || floorf(raw_index) != raw_index) {
        atomicCAS(status, 0, 1);
        return;
    }

    size_t row = static_cast<size_t>(raw_index);
    if (row >= vocab_size) {
        atomicCAS(status, 0, 2);
        return;
    }

    atomicAdd(grad_weight + row * embed_dim + col, grad[idx]);
}

__global__ void rms_norm_kernel(
    const float* input,
    const float* weight,
    float* out,
    size_t rows,
    size_t dim,
    float eps) {
    size_t row = blockIdx.x * blockDim.x + threadIdx.x;
    if (row >= rows) {
        return;
    }

    const float* row_in = input + row * dim;
    float* row_out = out + row * dim;

    float sum_sq = 0.0f;
    for (size_t j = 0; j < dim; ++j) {
        float value = row_in[j];
        sum_sq += value * value;
    }

    float inv_rms = rsqrtf(sum_sq / static_cast<float>(dim) + eps);
    for (size_t j = 0; j < dim; ++j) {
        row_out[j] = row_in[j] * inv_rms * weight[j];
    }
}

__global__ void rms_norm_backward_kernel(
    const float* input,
    const float* weight,
    const float* grad,
    float* grad_input,
    float* grad_weight,
    size_t rows,
    size_t dim,
    float eps) {
    size_t row = blockIdx.x * blockDim.x + threadIdx.x;
    if (row >= rows) {
        return;
    }

    const float* row_x = input + row * dim;
    const float* row_g = grad + row * dim;
    float* row_dx = grad_input + row * dim;

    float sum_sq = 0.0f;
    for (size_t j = 0; j < dim; ++j) {
        float x = row_x[j];
        sum_sq += x * x;
    }
    float inv_rms = rsqrtf(sum_sq / static_cast<float>(dim) + eps);

    float dot = 0.0f;
    for (size_t j = 0; j < dim; ++j) {
        dot += (row_g[j] * weight[j]) * (row_x[j] * inv_rms);
    }
    float mean_dot = dot / static_cast<float>(dim);

    for (size_t j = 0; j < dim; ++j) {
        float x_norm = row_x[j] * inv_rms;
        row_dx[j] = inv_rms * (row_g[j] * weight[j] - x_norm * mean_dot);
        atomicAdd(grad_weight + j, row_g[j] * row_x[j] * inv_rms);
    }
}

__global__ void permute_kernel(
    const float* input,
    float* out,
    size_t ndim,
    const size_t* out_shape,
    const size_t* out_strides,
    const size_t* mapped_input_strides,
    size_t len) {
    size_t out_idx = blockIdx.x * blockDim.x + threadIdx.x;
    if (out_idx >= len) {
        return;
    }

    size_t remaining = out_idx;
    size_t input_idx = 0;
    for (size_t i = 0; i < ndim; ++i) {
        size_t coord = 0;
        if (out_shape[i] > 0) {
            coord = remaining / out_strides[i];
            remaining %= out_strides[i];
        }
        input_idx += coord * mapped_input_strides[i];
    }
    out[out_idx] = input[input_idx];
}

__global__ void slice_lastdim_kernel(
    const float* input,
    float* out,
    size_t outer,
    size_t input_last_dim,
    size_t start,
    size_t slice_len) {
    size_t idx = blockIdx.x * blockDim.x + threadIdx.x;
    size_t total = outer * slice_len;
    if (idx >= total) {
        return;
    }

    size_t row = idx / slice_len;
    size_t col = idx % slice_len;
    out[idx] = input[row * input_last_dim + start + col];
}

__global__ void slice_lastdim_backward_kernel(
    const float* grad,
    float* out,
    size_t outer,
    size_t input_last_dim,
    size_t start,
    size_t slice_len) {
    size_t idx = blockIdx.x * blockDim.x + threadIdx.x;
    size_t total = outer * slice_len;
    if (idx >= total) {
        return;
    }

    size_t row = idx / slice_len;
    size_t col = idx % slice_len;
    out[row * input_last_dim + start + col] = grad[idx];
}

__global__ void append_kv_cache_kernel(
    float* dst,
    const float* src,
    size_t batch_size,
    size_t num_heads,
    size_t src_seq_len,
    size_t dst_seq_len,
    size_t dim,
    size_t dst_start) {
    size_t idx = blockIdx.x * blockDim.x + threadIdx.x;
    size_t total = batch_size * num_heads * src_seq_len * dim;
    if (idx >= total) {
        return;
    }

    size_t dd = idx % dim;
    size_t tmp = idx / dim;
    size_t ss = tmp % src_seq_len;
    tmp /= src_seq_len;
    size_t hh = tmp % num_heads;
    size_t bb = tmp / num_heads;

    size_t src_idx = (((bb * num_heads + hh) * src_seq_len + ss) * dim) + dd;
    size_t dst_idx = (((bb * num_heads + hh) * dst_seq_len + (dst_start + ss)) * dim) + dd;
    dst[dst_idx] = src[src_idx];
}

__global__ void append_kv_cache_pair_kernel(
    float* k_dst,
    float* v_dst,
    const float* k_src,
    const float* v_src,
    size_t batch_size,
    size_t num_heads,
    size_t src_seq_len,
    size_t dst_seq_len,
    size_t dim,
    size_t dst_start) {
    size_t idx = blockIdx.x * blockDim.x + threadIdx.x;
    size_t total = batch_size * num_heads * src_seq_len * dim;
    if (idx >= total) {
        return;
    }

    size_t dd = idx % dim;
    size_t tmp = idx / dim;
    size_t ss = tmp % src_seq_len;
    tmp /= src_seq_len;
    size_t hh = tmp % num_heads;
    size_t bb = tmp / num_heads;

    size_t src_idx = (((bb * num_heads + hh) * src_seq_len + ss) * dim) + dd;
    size_t dst_idx = (((bb * num_heads + hh) * dst_seq_len + (dst_start + ss)) * dim) + dd;
    k_dst[dst_idx] = k_src[src_idx];
    v_dst[dst_idx] = v_src[src_idx];
}

__global__ void decode_rope_q_append_kv_kernel(
    const float* q_src,
    const float* k_src,
    const float* v_src,
    const float* cos,
    const float* sin,
    float* q_out,
    float* k_cache,
    float* v_cache,
    size_t batch_size,
    size_t num_heads,
    size_t num_kv_heads,
    size_t dim,
    size_t dst_seq_len,
    size_t offset) {
    size_t idx = blockIdx.x * blockDim.x + threadIdx.x;
    size_t half = dim / 2;
    size_t q_len = batch_size * num_heads * dim;
    size_t kv_len = batch_size * num_kv_heads * dim;
    const float* cos_row = cos + offset * dim;
    const float* sin_row = sin + offset * dim;

    if (idx < q_len) {
        size_t dd = idx % dim;
        size_t pair = dd % half;
        size_t base = idx - dd;
        float x1 = q_src[base + pair];
        float x2 = q_src[base + pair + half];
        float c = cos_row[pair];
        float s = sin_row[pair];
        q_out[idx] = dd < half ? x1 * c - x2 * s : x1 * s + x2 * c;
    }

    if (idx < kv_len) {
        size_t dd = idx % dim;
        size_t pair = dd % half;
        size_t tmp = idx / dim;
        size_t hk = tmp % num_kv_heads;
        size_t bb = tmp / num_kv_heads;
        size_t base = idx - dd;
        float x1 = k_src[base + pair];
        float x2 = k_src[base + pair + half];
        float c = cos_row[pair];
        float s = sin_row[pair];
        size_t cache_idx = ((bb * num_kv_heads + hk) * dst_seq_len + offset) * dim + dd;
        k_cache[cache_idx] = dd < half ? x1 * c - x2 * s : x1 * s + x2 * c;
        v_cache[cache_idx] = v_src[idx];
    }
}

__global__ void kv_cache_prefix_kernel(
    const float* src,
    float* out,
    size_t batch_size,
    size_t num_heads,
    size_t active_seq_len,
    size_t src_seq_len,
    size_t dim) {
    size_t idx = blockIdx.x * blockDim.x + threadIdx.x;
    size_t total = batch_size * num_heads * active_seq_len * dim;
    if (idx >= total) {
        return;
    }

    size_t dd = idx % dim;
    size_t tmp = idx / dim;
    size_t ss = tmp % active_seq_len;
    tmp /= active_seq_len;
    size_t hh = tmp % num_heads;
    size_t bb = tmp / num_heads;

    size_t src_idx = (((bb * num_heads + hh) * src_seq_len + ss) * dim) + dd;
    out[idx] = src[src_idx];
}

__global__ void cat_kernel(
    const float* lhs,
    const float* rhs,
    float* out,
    const size_t* out_shape,
    const size_t* out_strides,
    const size_t* lhs_strides,
    const size_t* rhs_strides,
    size_t ndim,
    size_t axis,
    size_t lhs_axis_len,
    size_t len) {
    size_t out_idx = blockIdx.x * blockDim.x + threadIdx.x;
    if (out_idx >= len) {
        return;
    }

    size_t remaining = out_idx;
    size_t lhs_idx = 0;
    size_t rhs_idx = 0;
    bool use_rhs = false;

    for (size_t i = 0; i < ndim; ++i) {
        size_t coord = 0;
        if (out_shape[i] > 0) {
            coord = remaining / out_strides[i];
            remaining %= out_strides[i];
        }

        if (i == axis) {
            if (coord < lhs_axis_len) {
                lhs_idx += coord * lhs_strides[i];
            } else {
                use_rhs = true;
                rhs_idx += (coord - lhs_axis_len) * rhs_strides[i];
            }
        } else {
            lhs_idx += coord * lhs_strides[i];
            rhs_idx += coord * rhs_strides[i];
        }
    }

    out[out_idx] = use_rhs ? rhs[rhs_idx] : lhs[lhs_idx];
}

__global__ void cat_backward_slice_kernel(
    const float* grad,
    float* out,
    const size_t* input_shape,
    const size_t* input_strides,
    const size_t* out_strides,
    size_t ndim,
    size_t axis,
    size_t axis_start,
    size_t len) {
    size_t input_idx = blockIdx.x * blockDim.x + threadIdx.x;
    if (input_idx >= len) {
        return;
    }

    size_t remaining = input_idx;
    size_t grad_idx = 0;
    for (size_t i = 0; i < ndim; ++i) {
        size_t coord = 0;
        if (input_shape[i] > 0) {
            coord = remaining / input_strides[i];
            remaining %= input_strides[i];
        }
        if (i == axis) {
            coord += axis_start;
        }
        grad_idx += coord * out_strides[i];
    }

    out[input_idx] = grad[grad_idx];
}

__global__ void repeat_kv_kernel(
    const float* input,
    float* out,
    size_t num_heads,
    size_t seq_len,
    size_t dim,
    size_t n_rep) {
    size_t out_idx = blockIdx.x * blockDim.x + threadIdx.x;
    size_t total = gridDim.x * blockDim.x;
    (void)total;
    size_t head_stride = seq_len * dim;
    size_t out_head_idx = out_idx / head_stride;
    if (head_stride == 0 || out_head_idx >= num_heads) {
        return;
    }
    size_t within_head = out_idx % head_stride;
    size_t kv_head_idx = out_head_idx / n_rep;
    size_t input_idx = kv_head_idx * head_stride + within_head;
    out[out_idx] = input[input_idx];
}

__global__ void repeat_kv_backward_kernel(
    const float* grad,
    float* out,
    size_t batch_size,
    size_t num_kv_heads,
    size_t seq_len,
    size_t dim,
    size_t n_rep) {
    size_t idx = blockIdx.x * blockDim.x + threadIdx.x;
    size_t input_len = batch_size * num_kv_heads * seq_len * dim;
    if (idx >= input_len) {
        return;
    }

    size_t within_dim = idx % dim;
    size_t tmp = idx / dim;
    size_t seq_idx = tmp % seq_len;
    tmp /= seq_len;
    size_t kv_head = tmp % num_kv_heads;
    size_t batch = tmp / num_kv_heads;

    size_t out_heads = num_kv_heads * n_rep;
    float acc = 0.0f;
    for (size_t rep = 0; rep < n_rep; ++rep) {
        size_t out_head = kv_head * n_rep + rep;
        size_t grad_idx = ((batch * out_heads + out_head) * seq_len + seq_idx) * dim + within_dim;
        acc += grad[grad_idx];
    }
    out[idx] = acc;
}

__global__ void decode_attention_kernel(
    const float* q,
    const float* k,
    const float* v,
    float* out,
    size_t num_heads,
    size_t num_kv_heads,
    size_t active_seq_len,
    size_t cache_seq_len,
    size_t dim,
    size_t n_rep,
    float scale) {
    size_t row = blockIdx.x;
    size_t tid = threadIdx.x;
    size_t hh = row % num_heads;
    size_t hk = hh / n_rep;

    extern __shared__ float shared[];
    float* reduce = shared;
    float* q_shared = reduce + blockDim.x;
    float* ctx_shared = q_shared + dim;
    float* scalars = ctx_shared + dim;

    const float* q_row = q + row * dim;
    float* out_row = out + row * dim;
    for (size_t i = tid; i < dim; i += blockDim.x) {
        q_shared[i] = q_row[i];
        ctx_shared[i] = 0.0f;
    }
    if (tid == 0) {
        scalars[0] = -INFINITY;
        scalars[1] = 0.0f;
        scalars[2] = 0.0f;
        scalars[3] = 0.0f;
    }
    __syncthreads();

    size_t batch_idx = row / num_heads;
    const float* k_base = k + (batch_idx * num_kv_heads + hk) * cache_seq_len * dim;
    const float* v_base = v + (batch_idx * num_kv_heads + hk) * cache_seq_len * dim;

    for (size_t pos = 0; pos < active_seq_len; ++pos) {
        const float* k_row = k_base + pos * dim;
        const float* v_row = v_base + pos * dim;

        float partial = 0.0f;
        for (size_t i = tid; i < dim; i += blockDim.x) {
            partial += q_shared[i] * k_row[i];
        }
        reduce[tid] = partial;
        __syncthreads();

        for (unsigned int stride = blockDim.x / 2; stride > 0; stride >>= 1) {
            if (tid < stride) {
                reduce[tid] += reduce[tid + stride];
            }
            __syncthreads();
        }

        if (tid == 0) {
            float score = reduce[0] * scale;
            float m = scalars[0];
            float l = scalars[1];
            float prev_scale;
            float weight;
            if (score > m) {
                prev_scale = l > 0.0f ? expf(m - score) : 0.0f;
                weight = 1.0f;
                l = l * prev_scale + 1.0f;
                m = score;
            } else {
                prev_scale = 1.0f;
                weight = expf(score - m);
                l += weight;
            }
            scalars[0] = m;
            scalars[1] = l;
            scalars[2] = prev_scale;
            scalars[3] = weight;
        }
        __syncthreads();

        float prev_scale = scalars[2];
        float weight = scalars[3];
        for (size_t i = tid; i < dim; i += blockDim.x) {
            ctx_shared[i] = ctx_shared[i] * prev_scale + weight * v_row[i];
        }
        __syncthreads();
    }

    float inv_l = 1.0f / (scalars[1] + 1e-9f);
    for (size_t i = tid; i < dim; i += blockDim.x) {
        out_row[i] = ctx_shared[i] * inv_l;
    }
}

__global__ void prefill_attention_kernel(
    const float* q,
    const float* k,
    const float* v,
    float* out,
    size_t num_heads,
    size_t num_kv_heads,
    size_t q_seq_len,
    size_t active_seq_len,
    size_t cache_seq_len,
    size_t dim,
    size_t n_rep,
    size_t past_len,
    float scale,
    int is_causal) {
    size_t row = blockIdx.x;
    size_t tid = threadIdx.x;
    size_t sq = row % q_seq_len;
    size_t tmp = row / q_seq_len;
    size_t hh = tmp % num_heads;
    size_t batch_idx = tmp / num_heads;
    size_t hk = hh / n_rep;

    extern __shared__ float shared[];
    float* reduce = shared;
    float* q_shared = reduce + blockDim.x;
    float* ctx_shared = q_shared + dim;
    float* scalars = ctx_shared + dim;

    const float* q_row = q + ((batch_idx * num_heads + hh) * q_seq_len + sq) * dim;
    float* out_row = out + ((batch_idx * q_seq_len + sq) * num_heads + hh) * dim;
    for (size_t i = tid; i < dim; i += blockDim.x) {
        q_shared[i] = q_row[i];
        ctx_shared[i] = 0.0f;
    }
    if (tid == 0) {
        scalars[0] = -INFINITY;
        scalars[1] = 0.0f;
        scalars[2] = 0.0f;
        scalars[3] = 0.0f;
    }
    __syncthreads();

    const float* k_base = k + (batch_idx * num_kv_heads + hk) * cache_seq_len * dim;
    const float* v_base = v + (batch_idx * num_kv_heads + hk) * cache_seq_len * dim;
    size_t query_abs = past_len + sq;

    for (size_t pos = 0; pos < active_seq_len; ++pos) {
        if (is_causal != 0 && pos > query_abs) {
            break;
        }
        const float* k_row = k_base + pos * dim;
        const float* v_row = v_base + pos * dim;

        float partial = 0.0f;
        for (size_t i = tid; i < dim; i += blockDim.x) {
            partial += q_shared[i] * k_row[i];
        }
        reduce[tid] = partial;
        __syncthreads();

        for (unsigned int stride = blockDim.x / 2; stride > 0; stride >>= 1) {
            if (tid < stride) {
                reduce[tid] += reduce[tid + stride];
            }
            __syncthreads();
        }

        if (tid == 0) {
            float score = reduce[0] * scale;
            float m = scalars[0];
            float l = scalars[1];
            float prev_scale;
            float weight;
            if (score > m) {
                prev_scale = l > 0.0f ? expf(m - score) : 0.0f;
                weight = 1.0f;
                l = l * prev_scale + 1.0f;
                m = score;
            } else {
                prev_scale = 1.0f;
                weight = expf(score - m);
                l += weight;
            }
            scalars[0] = m;
            scalars[1] = l;
            scalars[2] = prev_scale;
            scalars[3] = weight;
        }
        __syncthreads();

        float prev_scale = scalars[2];
        float weight = scalars[3];
        for (size_t i = tid; i < dim; i += blockDim.x) {
            ctx_shared[i] = ctx_shared[i] * prev_scale + weight * v_row[i];
        }
        __syncthreads();
    }

    float inv_l = 1.0f / (scalars[1] + 1e-9f);
    for (size_t i = tid; i < dim; i += blockDim.x) {
        out_row[i] = ctx_shared[i] * inv_l;
    }
}

__global__ void silu_mul_kernel(const float* gate, const float* up, float* out, size_t len) {
    size_t idx = blockIdx.x * blockDim.x + threadIdx.x;
    if (idx >= len) {
        return;
    }
    float g = gate[idx];
    float sig = 1.0f / (1.0f + expf(-g));
    out[idx] = (g * sig) * up[idx];
}

__global__ void rope_kernel(
    const float* input,
    const float* cos,
    const float* sin,
    float* out,
    size_t seq_len,
    size_t dim,
    size_t offset) {
    size_t half = dim / 2;
    size_t elements = seq_len * half;
    size_t pair_idx = static_cast<size_t>(blockIdx.x) * blockDim.x + threadIdx.x;
    size_t batch_head = blockIdx.y;
    if (pair_idx >= elements) {
        return;
    }

    size_t seq_idx = pair_idx / half;
    size_t j = pair_idx % half;
    size_t base = (batch_head * seq_len + seq_idx) * dim;
    size_t cache_base = (offset + seq_idx) * dim;

    float x1 = input[base + j];
    float x2 = input[base + j + half];
    float c = cos[cache_base + j];
    float s_val = sin[cache_base + j];

    out[base + j] = x1 * c - x2 * s_val;
    out[base + j + half] = x2 * c + x1 * s_val;
}

__global__ void rope_backward_kernel(
    const float* grad,
    const float* cos,
    const float* sin,
    float* out,
    size_t seq_len,
    size_t dim,
    size_t offset) {
    size_t half = dim / 2;
    size_t elements = seq_len * half;
    size_t pair_idx = static_cast<size_t>(blockIdx.x) * blockDim.x + threadIdx.x;
    size_t batch_head = blockIdx.y;
    if (pair_idx >= elements) {
        return;
    }

    size_t seq_idx = pair_idx / half;
    size_t j = pair_idx % half;
    size_t base = (batch_head * seq_len + seq_idx) * dim;
    size_t cache_base = (offset + seq_idx) * dim;

    float g1 = grad[base + j];
    float g2 = grad[base + j + half];
    float c = cos[cache_base + j];
    float s_val = sin[cache_base + j];

    out[base + j] = g1 * c + g2 * s_val;
    out[base + j + half] = g2 * c - g1 * s_val;
}

bool validate_handle(uint64_t handle, const char* name) {
    if (handle == 0) {
        set_error(std::string(name) + " is null");
        return false;
    }
    return true;
}

}  // namespace

extern "C" int lumen_cuda_is_available() {
    int device_count = 0;
    cudaError_t status = cudaGetDeviceCount(&device_count);
    if (status != cudaSuccess) {
        set_cuda_error("failed to query CUDA devices", status);
        return 0;
    }
    return device_count > 0 ? 1 : 0;
}

extern "C" const char* lumen_cuda_last_error_message() {
    return g_last_error.c_str();
}

extern "C" int lumen_cuda_alloc_f32(size_t len, uint64_t* out_handle) {
    if (out_handle == nullptr) {
        set_error("CUDA alloc received a null output handle");
        return 1;
    }
    if (len > static_cast<size_t>(-1) / sizeof(float)) {
        set_error("CUDA alloc length overflow");
        return 1;
    }
    size_t bytes = len * sizeof(float);
    float* ptr = nullptr;
    if (try_take_pooled_cuda_buffer(bytes, reinterpret_cast<void**>(&ptr))) {
        *out_handle = reinterpret_cast<uint64_t>(ptr);
        return 0;
    }

    cudaError_t status = cudaMalloc(reinterpret_cast<void**>(&ptr), bytes);
    if (status != cudaSuccess) {
        clear_cuda_buffer_pool();
        status = cudaMalloc(reinterpret_cast<void**>(&ptr), bytes);
    }
    if (status != cudaSuccess) {
        set_cuda_error("failed to allocate CUDA buffer", status);
        return 1;
    }
    *out_handle = reinterpret_cast<uint64_t>(ptr);
    return 0;
}

extern "C" int lumen_cuda_upload_f32(uint64_t handle, const float* src, size_t len) {
    if (!validate_handle(handle, "CUDA upload handle")) {
        return 1;
    }
    if (src == nullptr) {
        set_error("CUDA upload source is null");
        return 1;
    }
    cudaError_t status =
        cudaMemcpy(handle_to_ptr(handle), src, len * sizeof(float), cudaMemcpyHostToDevice);
    if (status != cudaSuccess) {
        set_cuda_error("failed to upload CUDA buffer", status);
        return 1;
    }
    return 0;
}

extern "C" int lumen_cuda_upload_f32_offset(
    uint64_t handle,
    const float* src,
    size_t offset,
    size_t len) {
    if (!validate_handle(handle, "CUDA upload handle")) {
        return 1;
    }
    if (src == nullptr) {
        set_error("CUDA upload source is null");
        return 1;
    }
    cudaError_t status = cudaMemcpy(
        handle_to_ptr(handle) + offset,
        src,
        len * sizeof(float),
        cudaMemcpyHostToDevice);
    if (status != cudaSuccess) {
        set_cuda_error("failed to upload CUDA buffer slice", status);
        return 1;
    }
    return 0;
}

extern "C" int lumen_cuda_copy_f32_offset(
    uint64_t dst_handle,
    size_t dst_offset,
    uint64_t src_handle,
    size_t src_offset,
    size_t len) {
    if (!validate_handle(dst_handle, "CUDA copy destination handle") ||
        !validate_handle(src_handle, "CUDA copy source handle")) {
        return 1;
    }
    cudaError_t status = cudaMemcpy(
        handle_to_ptr(dst_handle) + dst_offset,
        handle_to_ptr(src_handle) + src_offset,
        len * sizeof(float),
        cudaMemcpyDeviceToDevice);
    if (status != cudaSuccess) {
        set_cuda_error("failed to copy CUDA tensor slice", status);
        return 1;
    }
    return 0;
}

extern "C" int lumen_cuda_append_kv_cache_f32_device(
    uint64_t dst_handle,
    uint64_t src_handle,
    size_t batch_size,
    size_t num_heads,
    size_t src_seq_len,
    size_t dst_seq_len,
    size_t dim,
    size_t dst_start) {
    if (!validate_handle(dst_handle, "CUDA KV cache destination handle") ||
        !validate_handle(src_handle, "CUDA KV cache source handle")) {
        return 1;
    }
    if (batch_size == 0 || num_heads == 0 || src_seq_len == 0 || dst_seq_len == 0 || dim == 0) {
        set_error("CUDA KV cache append dimensions must be greater than zero");
        return 1;
    }
    if (dst_start > dst_seq_len || src_seq_len > dst_seq_len - dst_start) {
        set_error("CUDA KV cache append range is out of bounds");
        return 1;
    }

    size_t total = batch_size * num_heads * src_seq_len * dim;
    constexpr int block_size = 256;
    size_t grid_size = (total + block_size - 1) / block_size;
    append_kv_cache_kernel<<<grid_size, block_size>>>(
        handle_to_ptr(dst_handle),
        handle_to_ptr(src_handle),
        batch_size,
        num_heads,
        src_seq_len,
        dst_seq_len,
        dim,
        dst_start);
    if (!sync_cuda("CUDA KV cache append kernel failed")) {
        return 1;
    }
    return 0;
}

extern "C" int lumen_cuda_kv_cache_prefix_f32_device(
    uint64_t src_handle,
    uint64_t out_handle,
    size_t batch_size,
    size_t num_heads,
    size_t active_seq_len,
    size_t src_seq_len,
    size_t dim) {
    if (!validate_handle(src_handle, "CUDA KV cache source handle") ||
        !validate_handle(out_handle, "CUDA KV cache prefix output handle")) {
        return 1;
    }
    if (batch_size == 0 || num_heads == 0 || active_seq_len == 0 || src_seq_len == 0 ||
        dim == 0) {
        set_error("CUDA KV cache prefix dimensions must be greater than zero");
        return 1;
    }
    if (active_seq_len > src_seq_len) {
        set_error("CUDA KV cache prefix range is out of bounds");
        return 1;
    }

    size_t total = batch_size * num_heads * active_seq_len * dim;
    constexpr int block_size = 256;
    size_t grid_size = (total + block_size - 1) / block_size;
    kv_cache_prefix_kernel<<<grid_size, block_size>>>(
        handle_to_ptr(src_handle),
        handle_to_ptr(out_handle),
        batch_size,
        num_heads,
        active_seq_len,
        src_seq_len,
        dim);
    if (!sync_cuda("CUDA KV cache prefix kernel failed")) {
        return 1;
    }
    return 0;
}

extern "C" int lumen_cuda_download_f32(uint64_t handle, float* dst, size_t len) {
    if (!validate_handle(handle, "CUDA download handle")) {
        return 1;
    }
    if (dst == nullptr) {
        set_error("CUDA download destination is null");
        return 1;
    }
    cudaError_t status =
        cudaMemcpy(dst, handle_to_ptr(handle), len * sizeof(float), cudaMemcpyDeviceToHost);
    if (status != cudaSuccess) {
        set_cuda_error("failed to download CUDA buffer", status);
        return 1;
    }
    return 0;
}

extern "C" int lumen_cuda_download_f32_offset(
    uint64_t handle,
    float* dst,
    size_t offset,
    size_t len) {
    if (!validate_handle(handle, "CUDA download handle")) {
        return 1;
    }
    if (dst == nullptr) {
        set_error("CUDA download destination is null");
        return 1;
    }
    cudaError_t status = cudaMemcpy(
        dst,
        handle_to_ptr(handle) + offset,
        len * sizeof(float),
        cudaMemcpyDeviceToHost);
    if (status != cudaSuccess) {
        set_cuda_error("failed to download CUDA buffer slice", status);
        return 1;
    }
    return 0;
}

extern "C" void lumen_cuda_free_f32(uint64_t handle, size_t len) {
    release_cuda_buffer(handle, len);
}

extern "C" int lumen_cuda_synchronize() {
    return sync_cuda("CUDA synchronize failed") ? 0 : 1;
}

extern "C" int lumen_cuda_matvec_argmax_f32_device(
    uint64_t input_handle,
    uint64_t weight_handle,
    size_t* out_indices,
    size_t batch_size,
    size_t vocab_size,
    size_t hidden_size) {
    if (!validate_handle(input_handle, "CUDA matvec argmax input handle") ||
        !validate_handle(weight_handle, "CUDA matvec argmax weight handle")) {
        return 1;
    }
    if (out_indices == nullptr) {
        set_error("CUDA matvec argmax output is null");
        return 1;
    }
    if (batch_size == 0 || vocab_size == 0 || hidden_size == 0) {
        set_error("CUDA matvec argmax dimensions must be greater than zero");
        return 1;
    }

    CublasHandle cublas;
    if (!init_cublas(cublas)) {
        return 1;
    }

    const size_t max_size = static_cast<size_t>(-1);
    if (batch_size > max_size / vocab_size ||
        batch_size * vocab_size > max_size / sizeof(float)) {
        set_error("CUDA matvec argmax logits length overflow");
        return 1;
    }
    if (batch_size > max_size / sizeof(size_t)) {
        set_error("CUDA matvec argmax output length overflow");
        return 1;
    }

    thread_local ReusableCudaWorkspace logits_tmp;
    if (!logits_tmp.ensure(
            batch_size * vocab_size * sizeof(float),
            "failed to allocate CUDA matvec argmax logits")) {
        return 1;
    }
    thread_local ReusableCudaWorkspace device_out_tmp;
    if (!device_out_tmp.ensure(
            batch_size * sizeof(size_t),
            "failed to allocate CUDA matvec argmax output")) {
        return 1;
    }

    const float alpha = 1.0f;
    const float beta = 0.0f;
    cublasStatus_t cublas_status = CUBLAS_STATUS_SUCCESS;
    if (batch_size == 1) {
        cublas_status = cublasSgemv(
            cublas.handle,
            CUBLAS_OP_T,
            static_cast<int>(hidden_size),
            static_cast<int>(vocab_size),
            &alpha,
            handle_to_ptr(weight_handle),
            static_cast<int>(hidden_size),
            handle_to_ptr(input_handle),
            1,
            &beta,
            static_cast<float*>(logits_tmp.ptr),
            1);
    } else {
        cublas_status = cublasSgemm(
            cublas.handle,
            CUBLAS_OP_T,
            CUBLAS_OP_N,
            static_cast<int>(vocab_size),
            static_cast<int>(batch_size),
            static_cast<int>(hidden_size),
            &alpha,
            handle_to_ptr(weight_handle),
            static_cast<int>(hidden_size),
            handle_to_ptr(input_handle),
            static_cast<int>(hidden_size),
            &beta,
            static_cast<float*>(logits_tmp.ptr),
            static_cast<int>(vocab_size));
    }
    if (cublas_status != CUBLAS_STATUS_SUCCESS) {
        set_cublas_error("cuBLAS failed for matvec argmax logits", cublas_status);
        return 1;
    }

    constexpr int block_size = 256;
    argmax_rows_kernel<<<static_cast<unsigned int>(batch_size), block_size>>>(
        static_cast<float*>(logits_tmp.ptr),
        static_cast<size_t*>(device_out_tmp.ptr),
        batch_size,
        vocab_size);
    if (!sync_cuda("CUDA matvec argmax kernel failed")) {
        return 1;
    }

    cudaError_t status = cudaMemcpy(
        out_indices,
        static_cast<size_t*>(device_out_tmp.ptr),
        batch_size * sizeof(size_t),
        cudaMemcpyDeviceToHost);
    if (status != cudaSuccess) {
        set_cuda_error("failed to download CUDA matvec argmax output", status);
        return 1;
    }
    return 0;
}

extern "C" int lumen_cuda_matmul_f32_device(
    uint64_t a_handle,
    uint64_t b_handle,
    uint64_t out_handle,
    size_t m,
    size_t n,
    size_t k) {
    if (!validate_handle(a_handle, "CUDA matmul A handle") ||
        !validate_handle(b_handle, "CUDA matmul B handle") ||
        !validate_handle(out_handle, "CUDA matmul output handle")) {
        return 1;
    }
    if (!validate_dims(m, n, k)) {
        return 1;
    }

    CublasHandle handle;
    if (!init_cublas(handle)) {
        return 1;
    }

    const float alpha = 1.0f;
    const float beta = 0.0f;
    cublasStatus_t cublas_status = cublasSgemm(
        handle.handle,
        CUBLAS_OP_T,
        CUBLAS_OP_N,
        static_cast<int>(n),
        static_cast<int>(m),
        static_cast<int>(k),
        &alpha,
        handle_to_ptr(b_handle),
        static_cast<int>(k),
        handle_to_ptr(a_handle),
        static_cast<int>(k),
        &beta,
        handle_to_ptr(out_handle),
        static_cast<int>(n));
    if (cublas_status != CUBLAS_STATUS_SUCCESS) {
        set_cublas_error("cuBLAS SGEMM failed for matmul", cublas_status);
        return 1;
    }
    return 0;
}

extern "C" int lumen_cuda_batch_matmul_f32_device(
    uint64_t lhs_handle,
    uint64_t rhs_handle,
    uint64_t out_handle,
    size_t batch_count,
    size_t m,
    size_t n,
    size_t k) {
    if (!validate_handle(lhs_handle, "CUDA batch_matmul lhs handle") ||
        !validate_handle(rhs_handle, "CUDA batch_matmul rhs handle") ||
        !validate_handle(out_handle, "CUDA batch_matmul output handle")) {
        return 1;
    }
    if (batch_count == 0) {
        set_error("CUDA batch_matmul batch_count must be greater than zero");
        return 1;
    }
    if (!validate_dims(m, n, k)) {
        return 1;
    }

    CublasHandle handle;
    if (!init_cublas(handle)) {
        return 1;
    }

    const float alpha = 1.0f;
    const float beta = 0.0f;
    cublasStatus_t cublas_status = cublasSgemmStridedBatched(
        handle.handle,
        CUBLAS_OP_N,
        CUBLAS_OP_N,
        static_cast<int>(n),
        static_cast<int>(m),
        static_cast<int>(k),
        &alpha,
        handle_to_ptr(rhs_handle),
        static_cast<int>(n),
        static_cast<long long>(n * k),
        handle_to_ptr(lhs_handle),
        static_cast<int>(k),
        static_cast<long long>(m * k),
        &beta,
        handle_to_ptr(out_handle),
        static_cast<int>(n),
        static_cast<long long>(m * n),
        static_cast<int>(batch_count));
    if (cublas_status != CUBLAS_STATUS_SUCCESS) {
        set_cublas_error("cuBLAS strided batched SGEMM failed", cublas_status);
        return 1;
    }
    return 0;
}

extern "C" int lumen_cuda_unary_f32_device(
    uint64_t input_handle,
    uint64_t out_handle,
    size_t len,
    int op) {
    if (!validate_handle(input_handle, "CUDA unary input handle") ||
        !validate_handle(out_handle, "CUDA unary output handle")) {
        return 1;
    }

#if LUMEN_HAS_CUDNN
    cudnnActivationMode_t mode;
    if (cudnn_activation_mode_for_op(op, mode)) {
        CudnnHandle handle;
        CudnnTensorDescriptor input_desc;
        CudnnActivationDescriptor activation_desc;
        if (!init_cudnn(handle) ||
            !init_tensor_descriptor_4d(input_desc, 1, static_cast<int>(len), 1, 1) ||
            !init_activation_descriptor(activation_desc, mode)) {
            return 1;
        }

        const float alpha = 1.0f;
        const float beta = 0.0f;
        cudnnStatus_t status = cudnnActivationForward(
            handle.handle,
            activation_desc.desc,
            &alpha,
            input_desc.desc,
            handle_to_ptr(input_handle),
            &beta,
            input_desc.desc,
            handle_to_ptr(out_handle));
        if (status != CUDNN_STATUS_SUCCESS) {
            set_cudnn_error("cuDNN activation forward failed", status);
            return 1;
        }
        return 0;
    }
#endif

    constexpr int block_size = 256;
    int grid_size = static_cast<int>((len + block_size - 1) / block_size);
    unary_kernel<<<grid_size, block_size>>>(
        handle_to_ptr(input_handle),
        handle_to_ptr(out_handle),
        len,
        op);
    if (!sync_cuda("CUDA unary kernel failed")) {
        return 1;
    }
    return 0;
}

extern "C" int lumen_cuda_unary_backward_f32_device(
    uint64_t input_handle,
    uint64_t output_handle,
    uint64_t grad_handle,
    uint64_t out_handle,
    size_t len,
    int op) {
    if (!validate_handle(input_handle, "CUDA unary backward input handle") ||
        !validate_handle(output_handle, "CUDA unary backward output handle") ||
        !validate_handle(grad_handle, "CUDA unary backward grad handle") ||
        !validate_handle(out_handle, "CUDA unary backward result handle")) {
        return 1;
    }

#if LUMEN_HAS_CUDNN
    cudnnActivationMode_t mode;
    if (cudnn_activation_mode_for_op(op, mode)) {
        CudnnHandle handle;
        CudnnTensorDescriptor tensor_desc;
        CudnnActivationDescriptor activation_desc;
        if (!init_cudnn(handle) ||
            !init_tensor_descriptor_4d(tensor_desc, 1, static_cast<int>(len), 1, 1) ||
            !init_activation_descriptor(activation_desc, mode)) {
            return 1;
        }

        const float alpha = 1.0f;
        const float beta = 0.0f;
        cudnnStatus_t status = cudnnActivationBackward(
            handle.handle,
            activation_desc.desc,
            &alpha,
            tensor_desc.desc,
            handle_to_ptr(output_handle),
            tensor_desc.desc,
            handle_to_ptr(grad_handle),
            tensor_desc.desc,
            handle_to_ptr(input_handle),
            &beta,
            tensor_desc.desc,
            handle_to_ptr(out_handle));
        if (status != CUDNN_STATUS_SUCCESS) {
            set_cudnn_error("cuDNN activation backward failed", status);
            return 1;
        }
        return 0;
    }
#endif

    constexpr int block_size = 256;
    int grid_size = static_cast<int>((len + block_size - 1) / block_size);
    unary_backward_kernel<<<grid_size, block_size>>>(
        handle_to_ptr(input_handle),
        handle_to_ptr(output_handle),
        handle_to_ptr(grad_handle),
        handle_to_ptr(out_handle),
        len,
        op);
    if (!sync_cuda("CUDA unary backward kernel failed")) {
        return 1;
    }
    return 0;
}

extern "C" int lumen_cuda_binary_f32_device(
    uint64_t lhs_handle,
    uint64_t rhs_handle,
    uint64_t out_handle,
    size_t len,
    int op) {
    if (!validate_handle(lhs_handle, "CUDA binary lhs handle") ||
        !validate_handle(rhs_handle, "CUDA binary rhs handle") ||
        !validate_handle(out_handle, "CUDA binary output handle")) {
        return 1;
    }

    constexpr int block_size = 256;
    int grid_size = static_cast<int>((len + block_size - 1) / block_size);
    binary_kernel<<<grid_size, block_size>>>(
        handle_to_ptr(lhs_handle),
        handle_to_ptr(rhs_handle),
        handle_to_ptr(out_handle),
        len,
        op);
    if (!sync_cuda("CUDA binary kernel failed")) {
        return 1;
    }
    return 0;
}

extern "C" int lumen_cuda_binary_backward_f32_device(
    uint64_t lhs_handle,
    uint64_t rhs_handle,
    uint64_t grad_handle,
    uint64_t grad_lhs_handle,
    uint64_t grad_rhs_handle,
    size_t len,
    int op) {
    if (!validate_handle(lhs_handle, "CUDA binary backward lhs handle") ||
        !validate_handle(rhs_handle, "CUDA binary backward rhs handle") ||
        !validate_handle(grad_handle, "CUDA binary backward grad handle") ||
        !validate_handle(grad_lhs_handle, "CUDA binary backward lhs grad handle") ||
        !validate_handle(grad_rhs_handle, "CUDA binary backward rhs grad handle")) {
        return 1;
    }

    constexpr int block_size = 256;
    int grid_size = static_cast<int>((len + block_size - 1) / block_size);
    binary_backward_kernel<<<grid_size, block_size>>>(
        handle_to_ptr(lhs_handle),
        handle_to_ptr(rhs_handle),
        handle_to_ptr(grad_handle),
        handle_to_ptr(grad_lhs_handle),
        handle_to_ptr(grad_rhs_handle),
        len,
        op);
    if (!sync_cuda("CUDA binary backward kernel failed")) {
        return 1;
    }
    return 0;
}

extern "C" int lumen_cuda_binary_broadcast_f32_device(
    uint64_t lhs_handle,
    uint64_t rhs_handle,
    uint64_t out_handle,
    size_t ndim,
    const size_t* out_shape,
    const size_t* out_strides,
    const size_t* lhs_shape,
    const size_t* lhs_strides,
    const size_t* rhs_shape,
    const size_t* rhs_strides,
    size_t len,
    int op) {
    if (!validate_handle(lhs_handle, "CUDA binary broadcast lhs handle") ||
        !validate_handle(rhs_handle, "CUDA binary broadcast rhs handle") ||
        !validate_handle(out_handle, "CUDA binary broadcast output handle")) {
        return 1;
    }
    if (ndim == 0 || len == 0 || out_shape == nullptr || out_strides == nullptr ||
        lhs_shape == nullptr || lhs_strides == nullptr || rhs_shape == nullptr || rhs_strides == nullptr) {
        set_error("CUDA binary broadcast received invalid metadata");
        return 1;
    }

    size_t* d_out_shape = nullptr;
    size_t* d_out_strides = nullptr;
    size_t* d_lhs_shape = nullptr;
    size_t* d_lhs_strides = nullptr;
    size_t* d_rhs_shape = nullptr;
    size_t* d_rhs_strides = nullptr;
    auto cleanup = [&]() {
        if (d_out_shape != nullptr) cudaFree(d_out_shape);
        if (d_out_strides != nullptr) cudaFree(d_out_strides);
        if (d_lhs_shape != nullptr) cudaFree(d_lhs_shape);
        if (d_lhs_strides != nullptr) cudaFree(d_lhs_strides);
        if (d_rhs_shape != nullptr) cudaFree(d_rhs_shape);
        if (d_rhs_strides != nullptr) cudaFree(d_rhs_strides);
    };

    if (!upload_size_metadata("binary broadcast output shape", out_shape, ndim, &d_out_shape) ||
        !upload_size_metadata("binary broadcast output strides", out_strides, ndim, &d_out_strides) ||
        !upload_size_metadata("binary broadcast lhs shape", lhs_shape, ndim, &d_lhs_shape) ||
        !upload_size_metadata("binary broadcast lhs strides", lhs_strides, ndim, &d_lhs_strides) ||
        !upload_size_metadata("binary broadcast rhs shape", rhs_shape, ndim, &d_rhs_shape) ||
        !upload_size_metadata("binary broadcast rhs strides", rhs_strides, ndim, &d_rhs_strides)) {
        cleanup();
        return 1;
    }

    constexpr int block_size = 256;
    int grid_size = static_cast<int>((len + block_size - 1) / block_size);
    binary_broadcast_kernel<<<grid_size, block_size>>>(
        handle_to_ptr(lhs_handle),
        handle_to_ptr(rhs_handle),
        handle_to_ptr(out_handle),
        d_out_shape,
        d_out_strides,
        d_lhs_shape,
        d_lhs_strides,
        d_rhs_shape,
        d_rhs_strides,
        ndim,
        len,
        op);
    bool ok = sync_cuda("CUDA binary broadcast kernel failed");
    cleanup();
    return ok ? 0 : 1;
}

extern "C" int lumen_cuda_binary_broadcast_backward_f32_device(
    uint64_t lhs_handle,
    uint64_t rhs_handle,
    uint64_t grad_handle,
    uint64_t grad_lhs_handle,
    uint64_t grad_rhs_handle,
    size_t ndim,
    const size_t* out_shape,
    const size_t* out_strides,
    const size_t* lhs_shape,
    const size_t* lhs_strides,
    const size_t* rhs_shape,
    const size_t* rhs_strides,
    size_t out_len,
    size_t lhs_len,
    size_t rhs_len,
    int op) {
    if (!validate_handle(lhs_handle, "CUDA binary broadcast backward lhs handle") ||
        !validate_handle(rhs_handle, "CUDA binary broadcast backward rhs handle") ||
        !validate_handle(grad_handle, "CUDA binary broadcast backward grad handle") ||
        !validate_handle(grad_lhs_handle, "CUDA binary broadcast backward lhs grad handle") ||
        !validate_handle(grad_rhs_handle, "CUDA binary broadcast backward rhs grad handle")) {
        return 1;
    }
    if (ndim == 0 || out_len == 0 || out_shape == nullptr || out_strides == nullptr ||
        lhs_shape == nullptr || lhs_strides == nullptr || rhs_shape == nullptr || rhs_strides == nullptr) {
        set_error("CUDA binary broadcast backward received invalid metadata");
        return 1;
    }

    cudaError_t status = cudaMemset(handle_to_ptr(grad_lhs_handle), 0, lhs_len * sizeof(float));
    if (status != cudaSuccess) {
        set_cuda_error("CUDA binary broadcast lhs grad initialization failed", status);
        return 1;
    }
    status = cudaMemset(handle_to_ptr(grad_rhs_handle), 0, rhs_len * sizeof(float));
    if (status != cudaSuccess) {
        set_cuda_error("CUDA binary broadcast rhs grad initialization failed", status);
        return 1;
    }

    size_t* d_out_shape = nullptr;
    size_t* d_out_strides = nullptr;
    size_t* d_lhs_shape = nullptr;
    size_t* d_lhs_strides = nullptr;
    size_t* d_rhs_shape = nullptr;
    size_t* d_rhs_strides = nullptr;
    auto cleanup = [&]() {
        if (d_out_shape != nullptr) cudaFree(d_out_shape);
        if (d_out_strides != nullptr) cudaFree(d_out_strides);
        if (d_lhs_shape != nullptr) cudaFree(d_lhs_shape);
        if (d_lhs_strides != nullptr) cudaFree(d_lhs_strides);
        if (d_rhs_shape != nullptr) cudaFree(d_rhs_shape);
        if (d_rhs_strides != nullptr) cudaFree(d_rhs_strides);
    };

    if (!upload_size_metadata("binary broadcast backward output shape", out_shape, ndim, &d_out_shape) ||
        !upload_size_metadata("binary broadcast backward output strides", out_strides, ndim, &d_out_strides) ||
        !upload_size_metadata("binary broadcast backward lhs shape", lhs_shape, ndim, &d_lhs_shape) ||
        !upload_size_metadata("binary broadcast backward lhs strides", lhs_strides, ndim, &d_lhs_strides) ||
        !upload_size_metadata("binary broadcast backward rhs shape", rhs_shape, ndim, &d_rhs_shape) ||
        !upload_size_metadata("binary broadcast backward rhs strides", rhs_strides, ndim, &d_rhs_strides)) {
        cleanup();
        return 1;
    }

    constexpr int block_size = 256;
    int grid_size = static_cast<int>((out_len + block_size - 1) / block_size);
    binary_broadcast_backward_kernel<<<grid_size, block_size>>>(
        handle_to_ptr(lhs_handle),
        handle_to_ptr(rhs_handle),
        handle_to_ptr(grad_handle),
        handle_to_ptr(grad_lhs_handle),
        handle_to_ptr(grad_rhs_handle),
        d_out_shape,
        d_out_strides,
        d_lhs_shape,
        d_lhs_strides,
        d_rhs_shape,
        d_rhs_strides,
        ndim,
        out_len,
        op);
    bool ok = sync_cuda("CUDA binary broadcast backward kernel failed");
    cleanup();
    return ok ? 0 : 1;
}

extern "C" int lumen_cuda_sum_f32_device(
    uint64_t input_handle,
    uint64_t out_handle,
    size_t len) {
    if (!validate_handle(input_handle, "CUDA sum input handle") ||
        !validate_handle(out_handle, "CUDA sum output handle")) {
        return 1;
    }
    if (len == 0) {
        set_error("CUDA sum length must be greater than zero");
        return 1;
    }

    cudaError_t memset_status = cudaMemset(handle_to_ptr(out_handle), 0, sizeof(float));
    if (memset_status != cudaSuccess) {
        set_cuda_error("CUDA sum output initialization failed", memset_status);
        return 1;
    }

    constexpr int block_size = 256;
    int grid_size = static_cast<int>((len + block_size - 1) / block_size);
    if (grid_size > 1024) {
        grid_size = 1024;
    }
    sum_kernel<<<grid_size, block_size>>>(handle_to_ptr(input_handle), handle_to_ptr(out_handle), len);
    if (!sync_cuda("CUDA sum kernel failed")) {
        return 1;
    }
    return 0;
}

extern "C" int lumen_cuda_fill_scalar_f32_device(
    uint64_t out_handle,
    size_t len,
    float value) {
    if (!validate_handle(out_handle, "CUDA fill output handle")) {
        return 1;
    }

    constexpr int block_size = 256;
    int grid_size = static_cast<int>((len + block_size - 1) / block_size);
    fill_scalar_kernel<<<grid_size, block_size>>>(handle_to_ptr(out_handle), len, value);
    if (!sync_cuda("CUDA fill kernel failed")) {
        return 1;
    }
    return 0;
}

extern "C" int lumen_cuda_mse_backward_f32_device(
    uint64_t diff_handle,
    uint64_t grad_output_handle,
    uint64_t grad_target_handle,
    size_t len,
    float factor) {
    if (!validate_handle(diff_handle, "CUDA MSE backward diff handle") ||
        !validate_handle(grad_output_handle, "CUDA MSE backward output grad handle") ||
        !validate_handle(grad_target_handle, "CUDA MSE backward target grad handle")) {
        return 1;
    }

    constexpr int block_size = 256;
    int grid_size = static_cast<int>((len + block_size - 1) / block_size);
    mse_backward_kernel<<<grid_size, block_size>>>(
        handle_to_ptr(diff_handle),
        handle_to_ptr(grad_output_handle),
        handle_to_ptr(grad_target_handle),
        len,
        factor);
    if (!sync_cuda("CUDA MSE backward kernel failed")) {
        return 1;
    }
    return 0;
}

extern "C" int lumen_cuda_cross_entropy_backward_f32_device(
    uint64_t softmax_handle,
    uint64_t target_handle,
    uint64_t out_handle,
    size_t len,
    float factor) {
    if (!validate_handle(softmax_handle, "CUDA cross_entropy backward softmax handle") ||
        !validate_handle(target_handle, "CUDA cross_entropy backward target handle") ||
        !validate_handle(out_handle, "CUDA cross_entropy backward output handle")) {
        return 1;
    }

    constexpr int block_size = 256;
    int grid_size = static_cast<int>((len + block_size - 1) / block_size);
    cross_entropy_backward_kernel<<<grid_size, block_size>>>(
        handle_to_ptr(softmax_handle),
        handle_to_ptr(target_handle),
        handle_to_ptr(out_handle),
        len,
        factor);
    if (!sync_cuda("CUDA cross_entropy backward kernel failed")) {
        return 1;
    }
    return 0;
}

extern "C" int lumen_cuda_cross_entropy_loss_f32_device(
    uint64_t softmax_handle,
    uint64_t target_handle,
    uint64_t out_handle,
    size_t len,
    float factor) {
    if (!validate_handle(softmax_handle, "CUDA cross_entropy loss softmax handle") ||
        !validate_handle(target_handle, "CUDA cross_entropy loss target handle") ||
        !validate_handle(out_handle, "CUDA cross_entropy loss output handle")) {
        return 1;
    }
    if (len == 0) {
        set_error("CUDA cross_entropy loss length must be greater than zero");
        return 1;
    }

    cudaError_t memset_status = cudaMemset(handle_to_ptr(out_handle), 0, sizeof(float));
    if (memset_status != cudaSuccess) {
        set_cuda_error("CUDA cross_entropy loss output initialization failed", memset_status);
        return 1;
    }

    constexpr int block_size = 256;
    int grid_size = static_cast<int>((len + block_size - 1) / block_size);
    if (grid_size > 1024) {
        grid_size = 1024;
    }
    cross_entropy_loss_kernel<<<grid_size, block_size>>>(
        handle_to_ptr(softmax_handle),
        handle_to_ptr(target_handle),
        handle_to_ptr(out_handle),
        len,
        factor);
    if (!sync_cuda("CUDA cross_entropy loss kernel failed")) {
        return 1;
    }
    return 0;
}

extern "C" int lumen_cuda_sgd_update_f32_device(
    uint64_t param_handle,
    uint64_t grad_handle,
    size_t len,
    float lr) {
    if (!validate_handle(param_handle, "CUDA SGD param handle") ||
        !validate_handle(grad_handle, "CUDA SGD grad handle")) {
        return 1;
    }

    constexpr int block_size = 256;
    int grid_size = static_cast<int>((len + block_size - 1) / block_size);
    sgd_update_kernel<<<grid_size, block_size>>>(
        handle_to_ptr(param_handle),
        handle_to_ptr(grad_handle),
        len,
        lr);
    if (!sync_cuda("CUDA SGD update kernel failed")) {
        return 1;
    }
    return 0;
}

extern "C" int lumen_cuda_sgd_momentum_update_f32_device(
    uint64_t param_handle,
    uint64_t grad_handle,
    uint64_t velocity_handle,
    size_t len,
    float lr,
    float momentum) {
    if (!validate_handle(param_handle, "CUDA SGD momentum param handle") ||
        !validate_handle(grad_handle, "CUDA SGD momentum grad handle") ||
        !validate_handle(velocity_handle, "CUDA SGD momentum velocity handle")) {
        return 1;
    }

    constexpr int block_size = 256;
    int grid_size = static_cast<int>((len + block_size - 1) / block_size);
    sgd_momentum_update_kernel<<<grid_size, block_size>>>(
        handle_to_ptr(param_handle),
        handle_to_ptr(grad_handle),
        handle_to_ptr(velocity_handle),
        len,
        lr,
        momentum);
    if (!sync_cuda("CUDA SGD momentum update kernel failed")) {
        return 1;
    }
    return 0;
}

extern "C" int lumen_cuda_adam_update_f32_device(
    uint64_t param_handle,
    uint64_t grad_handle,
    uint64_t exp_avg_handle,
    uint64_t exp_avg_sq_handle,
    size_t len,
    float lr,
    float beta1,
    float beta2,
    float bias_correction1,
    float bias_correction2,
    float eps) {
    if (!validate_handle(param_handle, "CUDA Adam param handle") ||
        !validate_handle(grad_handle, "CUDA Adam grad handle") ||
        !validate_handle(exp_avg_handle, "CUDA Adam exp_avg handle") ||
        !validate_handle(exp_avg_sq_handle, "CUDA Adam exp_avg_sq handle")) {
        return 1;
    }

    constexpr int block_size = 256;
    int grid_size = static_cast<int>((len + block_size - 1) / block_size);
    adam_update_kernel<<<grid_size, block_size>>>(
        handle_to_ptr(param_handle),
        handle_to_ptr(grad_handle),
        handle_to_ptr(exp_avg_handle),
        handle_to_ptr(exp_avg_sq_handle),
        len,
        lr,
        beta1,
        beta2,
        bias_correction1,
        bias_correction2,
        eps);
    if (!sync_cuda("CUDA Adam update kernel failed")) {
        return 1;
    }
    return 0;
}

extern "C" int lumen_cuda_softmax_lastdim_f32_device(
    uint64_t input_handle,
    uint64_t out_handle,
    size_t outer,
    size_t last_dim) {
    if (!validate_handle(input_handle, "CUDA softmax input handle") ||
        !validate_handle(out_handle, "CUDA softmax output handle")) {
        return 1;
    }
    if (outer == 0 || last_dim == 0) {
        set_error("CUDA softmax dimensions must be greater than zero");
        return 1;
    }

#if LUMEN_HAS_CUDNN
    CudnnHandle handle;
    CudnnTensorDescriptor tensor_desc;
    if (!init_cudnn(handle) ||
        !init_tensor_descriptor_4d(
            tensor_desc,
            static_cast<int>(outer),
            static_cast<int>(last_dim),
            1,
            1)) {
        return 1;
    }

    const float alpha = 1.0f;
    const float beta = 0.0f;
    cudnnStatus_t status = cudnnSoftmaxForward(
        handle.handle,
        CUDNN_SOFTMAX_ACCURATE,
        CUDNN_SOFTMAX_MODE_CHANNEL,
        &alpha,
        tensor_desc.desc,
        handle_to_ptr(input_handle),
        &beta,
        tensor_desc.desc,
        handle_to_ptr(out_handle));
    if (status != CUDNN_STATUS_SUCCESS) {
        set_cudnn_error("cuDNN softmax forward failed", status);
        return 1;
    }
    return 0;
#endif

    constexpr int block_size = 128;
    int grid_size = static_cast<int>((outer + block_size - 1) / block_size);
    softmax_lastdim_kernel<<<grid_size, block_size>>>(
        handle_to_ptr(input_handle),
        handle_to_ptr(out_handle),
        outer,
        last_dim);
    if (!sync_cuda("CUDA softmax kernel failed")) {
        return 1;
    }
    return 0;
}

extern "C" int lumen_cuda_softmax_lastdim_backward_f32_device(
    uint64_t output_handle,
    uint64_t grad_handle,
    uint64_t out_handle,
    size_t outer,
    size_t last_dim) {
    if (!validate_handle(output_handle, "CUDA softmax backward output handle") ||
        !validate_handle(grad_handle, "CUDA softmax backward grad handle") ||
        !validate_handle(out_handle, "CUDA softmax backward result handle")) {
        return 1;
    }
    if (outer == 0 || last_dim == 0) {
        set_error("CUDA softmax backward dimensions must be greater than zero");
        return 1;
    }

#if LUMEN_HAS_CUDNN
    CudnnHandle handle;
    CudnnTensorDescriptor tensor_desc;
    if (!init_cudnn(handle) ||
        !init_tensor_descriptor_4d(
            tensor_desc,
            static_cast<int>(outer),
            static_cast<int>(last_dim),
            1,
            1)) {
        return 1;
    }

    const float alpha = 1.0f;
    const float beta = 0.0f;
    cudnnStatus_t status = cudnnSoftmaxBackward(
        handle.handle,
        CUDNN_SOFTMAX_ACCURATE,
        CUDNN_SOFTMAX_MODE_CHANNEL,
        &alpha,
        tensor_desc.desc,
        handle_to_ptr(output_handle),
        tensor_desc.desc,
        handle_to_ptr(grad_handle),
        &beta,
        tensor_desc.desc,
        handle_to_ptr(out_handle));
    if (status != CUDNN_STATUS_SUCCESS) {
        set_cudnn_error("cuDNN softmax backward failed", status);
        return 1;
    }
    return 0;
#endif

    constexpr int block_size = 128;
    int grid_size = static_cast<int>((outer + block_size - 1) / block_size);
    softmax_lastdim_backward_kernel<<<grid_size, block_size>>>(
        handle_to_ptr(output_handle),
        handle_to_ptr(grad_handle),
        handle_to_ptr(out_handle),
        outer,
        last_dim);
    if (!sync_cuda("CUDA softmax backward kernel failed")) {
        return 1;
    }
    return 0;
}

extern "C" int lumen_cuda_fused_softmax_f32_device(
    uint64_t input_handle,
    uint64_t out_handle,
    size_t batch_heads,
    size_t q_len,
    size_t k_len,
    float scale,
    int is_causal) {
    if (!validate_handle(input_handle, "CUDA fused_softmax input handle") ||
        !validate_handle(out_handle, "CUDA fused_softmax output handle")) {
        return 1;
    }
    if (batch_heads == 0 || q_len == 0 || k_len == 0) {
        set_error("CUDA fused_softmax dimensions must be greater than zero");
        return 1;
    }

    size_t rows = batch_heads * q_len;
    constexpr int block_size = 128;
    int grid_size = static_cast<int>((rows + block_size - 1) / block_size);
    fused_softmax_kernel<<<grid_size, block_size>>>(
        handle_to_ptr(input_handle),
        handle_to_ptr(out_handle),
        rows,
        q_len,
        k_len,
        scale,
        is_causal);
    if (!sync_cuda("CUDA fused_softmax kernel failed")) {
        return 1;
    }
    return 0;
}

extern "C" int lumen_cuda_fused_softmax_backward_f32_device(
    uint64_t output_handle,
    uint64_t grad_handle,
    uint64_t out_handle,
    size_t batch_heads,
    size_t q_len,
    size_t k_len,
    float scale) {
    if (!validate_handle(output_handle, "CUDA fused_softmax backward output handle") ||
        !validate_handle(grad_handle, "CUDA fused_softmax backward grad handle") ||
        !validate_handle(out_handle, "CUDA fused_softmax backward result handle")) {
        return 1;
    }
    if (batch_heads == 0 || q_len == 0 || k_len == 0) {
        set_error("CUDA fused_softmax backward dimensions must be greater than zero");
        return 1;
    }

    size_t rows = batch_heads * q_len;
    constexpr int block_size = 128;
    int grid_size = static_cast<int>((rows + block_size - 1) / block_size);
    fused_softmax_backward_kernel<<<grid_size, block_size>>>(
        handle_to_ptr(output_handle),
        handle_to_ptr(grad_handle),
        handle_to_ptr(out_handle),
        rows,
        k_len,
        scale);
    if (!sync_cuda("CUDA fused_softmax backward kernel failed")) {
        return 1;
    }
    return 0;
}

extern "C" int lumen_cuda_fused_softmax_f32_with_past_device(
    uint64_t input_handle,
    uint64_t out_handle,
    size_t batch_heads,
    size_t q_len,
    size_t k_len,
    float scale,
    int is_causal,
    size_t past_len) {
    if (!validate_handle(input_handle, "CUDA fused_softmax_with_past input handle") ||
        !validate_handle(out_handle, "CUDA fused_softmax_with_past output handle")) {
        return 1;
    }
    if (batch_heads == 0 || q_len == 0 || k_len == 0) {
        set_error("CUDA fused_softmax_with_past dimensions must be greater than zero");
        return 1;
    }
    if (past_len + q_len > k_len) {
        set_error("CUDA fused_softmax_with_past causal window exceeds key length");
        return 1;
    }

    size_t rows = batch_heads * q_len;
    constexpr int block_size = 128;
    int grid_size = static_cast<int>((rows + block_size - 1) / block_size);
    fused_softmax_with_past_kernel<<<grid_size, block_size>>>(
        handle_to_ptr(input_handle),
        handle_to_ptr(out_handle),
        rows,
        q_len,
        k_len,
        scale,
        is_causal,
        past_len);
    if (!sync_cuda("CUDA fused_softmax_with_past kernel failed")) {
        return 1;
    }
    return 0;
}

extern "C" int lumen_cuda_embedding_f32_device(
    uint64_t indices_handle,
    uint64_t weight_handle,
    uint64_t out_handle,
    size_t num_indices,
    size_t vocab_size,
    size_t embed_dim) {
    if (!validate_handle(indices_handle, "CUDA embedding indices handle") ||
        !validate_handle(weight_handle, "CUDA embedding weight handle") ||
        !validate_handle(out_handle, "CUDA embedding output handle")) {
        return 1;
    }
    if (num_indices == 0 || vocab_size == 0 || embed_dim == 0) {
        set_error("CUDA embedding dimensions must be greater than zero");
        return 1;
    }

    int* d_status = nullptr;
    cudaError_t alloc_status = cudaMalloc(reinterpret_cast<void**>(&d_status), sizeof(int));
    if (alloc_status != cudaSuccess) {
        set_cuda_error("failed to allocate CUDA embedding status buffer", alloc_status);
        return 1;
    }

    int zero = 0;
    cudaError_t copy_status =
        cudaMemcpy(d_status, &zero, sizeof(int), cudaMemcpyHostToDevice);
    if (copy_status != cudaSuccess) {
        cudaFree(d_status);
        set_cuda_error("failed to initialize CUDA embedding status buffer", copy_status);
        return 1;
    }

    constexpr int block_size = 256;
    size_t total = num_indices * embed_dim;
    int grid_size = static_cast<int>((total + block_size - 1) / block_size);
    embedding_kernel<<<grid_size, block_size>>>(
        handle_to_ptr(indices_handle),
        handle_to_ptr(weight_handle),
        handle_to_ptr(out_handle),
        num_indices,
        vocab_size,
        embed_dim,
        d_status);
    if (!sync_cuda("CUDA embedding kernel failed")) {
        cudaFree(d_status);
        return 1;
    }

    int host_status = 0;
    copy_status = cudaMemcpy(&host_status, d_status, sizeof(int), cudaMemcpyDeviceToHost);
    cudaFree(d_status);
    if (copy_status != cudaSuccess) {
        set_cuda_error("failed to read CUDA embedding status buffer", copy_status);
        return 1;
    }
    if (host_status == 1) {
        set_error("CUDA embedding encountered a non-finite, negative, or fractional index");
        return 1;
    }
    if (host_status == 2) {
        set_error("CUDA embedding encountered an out-of-bounds index");
        return 1;
    }
    return 0;
}

extern "C" int lumen_cuda_embedding_backward_f32_device(
    uint64_t indices_handle,
    uint64_t grad_handle,
    uint64_t grad_weight_handle,
    size_t num_indices,
    size_t vocab_size,
    size_t embed_dim) {
    if (!validate_handle(indices_handle, "CUDA embedding backward indices handle") ||
        !validate_handle(grad_handle, "CUDA embedding backward grad handle") ||
        !validate_handle(grad_weight_handle, "CUDA embedding backward weight grad handle")) {
        return 1;
    }
    if (num_indices == 0 || vocab_size == 0 || embed_dim == 0) {
        set_error("CUDA embedding backward dimensions must be greater than zero");
        return 1;
    }

    cudaError_t memset_status =
        cudaMemset(handle_to_ptr(grad_weight_handle), 0, vocab_size * embed_dim * sizeof(float));
    if (memset_status != cudaSuccess) {
        set_cuda_error("CUDA embedding backward weight grad initialization failed", memset_status);
        return 1;
    }

    int* d_status = nullptr;
    cudaError_t alloc_status = cudaMalloc(reinterpret_cast<void**>(&d_status), sizeof(int));
    if (alloc_status != cudaSuccess) {
        set_cuda_error("failed to allocate CUDA embedding backward status buffer", alloc_status);
        return 1;
    }

    int zero = 0;
    cudaError_t copy_status =
        cudaMemcpy(d_status, &zero, sizeof(int), cudaMemcpyHostToDevice);
    if (copy_status != cudaSuccess) {
        cudaFree(d_status);
        set_cuda_error("failed to initialize CUDA embedding backward status buffer", copy_status);
        return 1;
    }

    constexpr int block_size = 256;
    size_t total = num_indices * embed_dim;
    int grid_size = static_cast<int>((total + block_size - 1) / block_size);
    embedding_backward_kernel<<<grid_size, block_size>>>(
        handle_to_ptr(indices_handle),
        handle_to_ptr(grad_handle),
        handle_to_ptr(grad_weight_handle),
        num_indices,
        vocab_size,
        embed_dim,
        d_status);
    if (!sync_cuda("CUDA embedding backward kernel failed")) {
        cudaFree(d_status);
        return 1;
    }

    int host_status = 0;
    copy_status = cudaMemcpy(&host_status, d_status, sizeof(int), cudaMemcpyDeviceToHost);
    cudaFree(d_status);
    if (copy_status != cudaSuccess) {
        set_cuda_error("failed to read CUDA embedding backward status buffer", copy_status);
        return 1;
    }
    if (host_status == 1) {
        set_error("CUDA embedding backward encountered a non-finite, negative, or fractional index");
        return 1;
    }
    if (host_status == 2) {
        set_error("CUDA embedding backward encountered an out-of-bounds index");
        return 1;
    }
    return 0;
}

extern "C" int lumen_cuda_rms_norm_f32_device(
    uint64_t input_handle,
    uint64_t weight_handle,
    uint64_t out_handle,
    size_t rows,
    size_t dim,
    float eps) {
    if (!validate_handle(input_handle, "CUDA RMSNorm input handle") ||
        !validate_handle(weight_handle, "CUDA RMSNorm weight handle") ||
        !validate_handle(out_handle, "CUDA RMSNorm output handle")) {
        return 1;
    }
    if (rows == 0 || dim == 0) {
        set_error("CUDA RMSNorm dimensions must be greater than zero");
        return 1;
    }

    constexpr int block_size = 128;
    int grid_size = static_cast<int>((rows + block_size - 1) / block_size);
    rms_norm_kernel<<<grid_size, block_size>>>(
        handle_to_ptr(input_handle),
        handle_to_ptr(weight_handle),
        handle_to_ptr(out_handle),
        rows,
        dim,
        eps);
    if (!sync_cuda("CUDA RMSNorm kernel failed")) {
        return 1;
    }
    return 0;
}

extern "C" int lumen_cuda_rms_norm_backward_f32_device(
    uint64_t input_handle,
    uint64_t weight_handle,
    uint64_t grad_handle,
    uint64_t grad_input_handle,
    uint64_t grad_weight_handle,
    size_t rows,
    size_t dim,
    float eps) {
    if (!validate_handle(input_handle, "CUDA RMSNorm backward input handle") ||
        !validate_handle(weight_handle, "CUDA RMSNorm backward weight handle") ||
        !validate_handle(grad_handle, "CUDA RMSNorm backward grad handle") ||
        !validate_handle(grad_input_handle, "CUDA RMSNorm backward input grad handle") ||
        !validate_handle(grad_weight_handle, "CUDA RMSNorm backward weight grad handle")) {
        return 1;
    }
    if (rows == 0 || dim == 0) {
        set_error("CUDA RMSNorm backward dimensions must be greater than zero");
        return 1;
    }

    cudaError_t status = cudaMemset(handle_to_ptr(grad_weight_handle), 0, dim * sizeof(float));
    if (status != cudaSuccess) {
        set_cuda_error("CUDA RMSNorm backward weight grad initialization failed", status);
        return 1;
    }

    constexpr int block_size = 128;
    int grid_size = static_cast<int>((rows + block_size - 1) / block_size);
    rms_norm_backward_kernel<<<grid_size, block_size>>>(
        handle_to_ptr(input_handle),
        handle_to_ptr(weight_handle),
        handle_to_ptr(grad_handle),
        handle_to_ptr(grad_input_handle),
        handle_to_ptr(grad_weight_handle),
        rows,
        dim,
        eps);
    if (!sync_cuda("CUDA RMSNorm backward kernel failed")) {
        return 1;
    }
    return 0;
}

extern "C" int lumen_cuda_permute_f32_device(
    uint64_t input_handle,
    uint64_t out_handle,
    size_t ndim,
    const size_t* out_shape,
    const size_t* out_strides,
    const size_t* mapped_input_strides,
    size_t len) {
    if (!validate_handle(input_handle, "CUDA permute input handle") ||
        !validate_handle(out_handle, "CUDA permute output handle")) {
        return 1;
    }
    if (ndim == 0 || out_shape == nullptr || out_strides == nullptr || mapped_input_strides == nullptr) {
        set_error("CUDA permute received invalid metadata");
        return 1;
    }

    size_t* d_out_shape = nullptr;
    size_t* d_out_strides = nullptr;
    size_t* d_mapped_input_strides = nullptr;

    auto cleanup = [&]() {
        if (d_out_shape != nullptr) cudaFree(d_out_shape);
        if (d_out_strides != nullptr) cudaFree(d_out_strides);
        if (d_mapped_input_strides != nullptr) cudaFree(d_mapped_input_strides);
    };

    cudaError_t status = cudaMalloc(reinterpret_cast<void**>(&d_out_shape), ndim * sizeof(size_t));
    if (status != cudaSuccess) {
        set_cuda_error("failed to allocate CUDA permute shape buffer", status);
        cleanup();
        return 1;
    }
    status = cudaMalloc(reinterpret_cast<void**>(&d_out_strides), ndim * sizeof(size_t));
    if (status != cudaSuccess) {
        set_cuda_error("failed to allocate CUDA permute stride buffer", status);
        cleanup();
        return 1;
    }
    status = cudaMalloc(reinterpret_cast<void**>(&d_mapped_input_strides), ndim * sizeof(size_t));
    if (status != cudaSuccess) {
        set_cuda_error("failed to allocate CUDA permute input stride buffer", status);
        cleanup();
        return 1;
    }

    status = cudaMemcpy(d_out_shape, out_shape, ndim * sizeof(size_t), cudaMemcpyHostToDevice);
    if (status != cudaSuccess) {
        set_cuda_error("failed to upload CUDA permute shape buffer", status);
        cleanup();
        return 1;
    }
    status = cudaMemcpy(d_out_strides, out_strides, ndim * sizeof(size_t), cudaMemcpyHostToDevice);
    if (status != cudaSuccess) {
        set_cuda_error("failed to upload CUDA permute stride buffer", status);
        cleanup();
        return 1;
    }
    status = cudaMemcpy(
        d_mapped_input_strides,
        mapped_input_strides,
        ndim * sizeof(size_t),
        cudaMemcpyHostToDevice);
    if (status != cudaSuccess) {
        set_cuda_error("failed to upload CUDA permute input stride buffer", status);
        cleanup();
        return 1;
    }

    constexpr int block_size = 256;
    int grid_size = static_cast<int>((len + block_size - 1) / block_size);
    permute_kernel<<<grid_size, block_size>>>(
        handle_to_ptr(input_handle),
        handle_to_ptr(out_handle),
        ndim,
        d_out_shape,
        d_out_strides,
        d_mapped_input_strides,
        len);
    bool ok = sync_cuda("CUDA permute kernel failed");
    cleanup();
    return ok ? 0 : 1;
}

extern "C" int lumen_cuda_slice_lastdim_f32_device(
    uint64_t input_handle,
    uint64_t out_handle,
    size_t outer,
    size_t input_last_dim,
    size_t start,
    size_t slice_len) {
    if (!validate_handle(input_handle, "CUDA slice input handle") ||
        !validate_handle(out_handle, "CUDA slice output handle")) {
        return 1;
    }
    if (outer == 0 || input_last_dim == 0 || slice_len == 0) {
        set_error("CUDA slice dimensions must be greater than zero");
        return 1;
    }
    if (start + slice_len > input_last_dim) {
        set_error("CUDA slice range is out of bounds");
        return 1;
    }

    constexpr int block_size = 256;
    size_t total = outer * slice_len;
    int grid_size = static_cast<int>((total + block_size - 1) / block_size);
    slice_lastdim_kernel<<<grid_size, block_size>>>(
        handle_to_ptr(input_handle),
        handle_to_ptr(out_handle),
        outer,
        input_last_dim,
        start,
        slice_len);
    if (!sync_cuda("CUDA slice kernel failed")) {
        return 1;
    }
    return 0;
}

extern "C" int lumen_cuda_slice_lastdim_backward_f32_device(
    uint64_t grad_handle,
    uint64_t out_handle,
    size_t outer,
    size_t input_last_dim,
    size_t start,
    size_t slice_len) {
    if (!validate_handle(grad_handle, "CUDA slice_lastdim backward grad handle") ||
        !validate_handle(out_handle, "CUDA slice_lastdim backward output handle")) {
        return 1;
    }
    if (outer == 0 || input_last_dim == 0 || slice_len == 0 || start + slice_len > input_last_dim) {
        set_error("CUDA slice_lastdim backward received invalid dimensions");
        return 1;
    }

    cudaError_t memset_status =
        cudaMemset(handle_to_ptr(out_handle), 0, outer * input_last_dim * sizeof(float));
    if (memset_status != cudaSuccess) {
        set_cuda_error("CUDA slice_lastdim backward output initialization failed", memset_status);
        return 1;
    }

    constexpr int block_size = 256;
    size_t total = outer * slice_len;
    int grid_size = static_cast<int>((total + block_size - 1) / block_size);
    slice_lastdim_backward_kernel<<<grid_size, block_size>>>(
        handle_to_ptr(grad_handle),
        handle_to_ptr(out_handle),
        outer,
        input_last_dim,
        start,
        slice_len);
    if (!sync_cuda("CUDA slice_lastdim backward kernel failed")) {
        return 1;
    }
    return 0;
}

extern "C" int lumen_cuda_cat_f32_device(
    uint64_t lhs_handle,
    uint64_t rhs_handle,
    uint64_t out_handle,
    size_t ndim,
    const size_t* out_shape,
    const size_t* out_strides,
    const size_t* lhs_strides,
    const size_t* rhs_strides,
    size_t axis,
    size_t lhs_axis_len,
    size_t len) {
    if (!validate_handle(lhs_handle, "CUDA cat lhs handle") ||
        !validate_handle(rhs_handle, "CUDA cat rhs handle") ||
        !validate_handle(out_handle, "CUDA cat output handle")) {
        return 1;
    }
    if (ndim == 0 || len == 0) {
        set_error("CUDA cat received invalid metadata");
        return 1;
    }
    if (axis >= ndim) {
        set_error("CUDA cat axis is out of bounds");
        return 1;
    }
    if (out_shape == nullptr || out_strides == nullptr || lhs_strides == nullptr || rhs_strides == nullptr) {
        set_error("CUDA cat metadata pointers must not be null");
        return 1;
    }

    size_t* d_out_shape = nullptr;
    size_t* d_out_strides = nullptr;
    size_t* d_lhs_strides = nullptr;
    size_t* d_rhs_strides = nullptr;

    cudaError_t status = cudaMalloc(reinterpret_cast<void**>(&d_out_shape), ndim * sizeof(size_t));
    if (status != cudaSuccess) {
        set_cuda_error("failed to allocate CUDA cat shape buffer", status);
        return 1;
    }
    status = cudaMalloc(reinterpret_cast<void**>(&d_out_strides), ndim * sizeof(size_t));
    if (status != cudaSuccess) {
        cudaFree(d_out_shape);
        set_cuda_error("failed to allocate CUDA cat output stride buffer", status);
        return 1;
    }
    status = cudaMalloc(reinterpret_cast<void**>(&d_lhs_strides), ndim * sizeof(size_t));
    if (status != cudaSuccess) {
        cudaFree(d_out_shape);
        cudaFree(d_out_strides);
        set_cuda_error("failed to allocate CUDA cat lhs stride buffer", status);
        return 1;
    }
    status = cudaMalloc(reinterpret_cast<void**>(&d_rhs_strides), ndim * sizeof(size_t));
    if (status != cudaSuccess) {
        cudaFree(d_out_shape);
        cudaFree(d_out_strides);
        cudaFree(d_lhs_strides);
        set_cuda_error("failed to allocate CUDA cat rhs stride buffer", status);
        return 1;
    }

    status = cudaMemcpy(d_out_shape, out_shape, ndim * sizeof(size_t), cudaMemcpyHostToDevice);
    if (status != cudaSuccess) {
        cudaFree(d_out_shape);
        cudaFree(d_out_strides);
        cudaFree(d_lhs_strides);
        cudaFree(d_rhs_strides);
        set_cuda_error("failed to upload CUDA cat shape buffer", status);
        return 1;
    }
    status = cudaMemcpy(d_out_strides, out_strides, ndim * sizeof(size_t), cudaMemcpyHostToDevice);
    if (status != cudaSuccess) {
        cudaFree(d_out_shape);
        cudaFree(d_out_strides);
        cudaFree(d_lhs_strides);
        cudaFree(d_rhs_strides);
        set_cuda_error("failed to upload CUDA cat output stride buffer", status);
        return 1;
    }
    status = cudaMemcpy(d_lhs_strides, lhs_strides, ndim * sizeof(size_t), cudaMemcpyHostToDevice);
    if (status != cudaSuccess) {
        cudaFree(d_out_shape);
        cudaFree(d_out_strides);
        cudaFree(d_lhs_strides);
        cudaFree(d_rhs_strides);
        set_cuda_error("failed to upload CUDA cat lhs stride buffer", status);
        return 1;
    }
    status = cudaMemcpy(d_rhs_strides, rhs_strides, ndim * sizeof(size_t), cudaMemcpyHostToDevice);
    if (status != cudaSuccess) {
        cudaFree(d_out_shape);
        cudaFree(d_out_strides);
        cudaFree(d_lhs_strides);
        cudaFree(d_rhs_strides);
        set_cuda_error("failed to upload CUDA cat rhs stride buffer", status);
        return 1;
    }

    constexpr int block_size = 256;
    size_t grid_size = (len + block_size - 1) / block_size;
    cat_kernel<<<grid_size, block_size>>>(
        handle_to_ptr(lhs_handle),
        handle_to_ptr(rhs_handle),
        handle_to_ptr(out_handle),
        d_out_shape,
        d_out_strides,
        d_lhs_strides,
        d_rhs_strides,
        ndim,
        axis,
        lhs_axis_len,
        len);

    bool ok = sync_cuda("CUDA cat kernel failed");
    cudaFree(d_out_shape);
    cudaFree(d_out_strides);
    cudaFree(d_lhs_strides);
    cudaFree(d_rhs_strides);
    return ok ? 0 : 1;
}

extern "C" int lumen_cuda_cat_backward_slice_f32_device(
    uint64_t grad_handle,
    uint64_t out_handle,
    size_t ndim,
    const size_t* input_shape,
    const size_t* input_strides,
    const size_t* out_strides,
    size_t axis,
    size_t axis_start,
    size_t len) {
    if (!validate_handle(grad_handle, "CUDA cat backward grad handle") ||
        !validate_handle(out_handle, "CUDA cat backward output handle")) {
        return 1;
    }
    if (ndim == 0 || len == 0) {
        set_error("CUDA cat backward received invalid metadata");
        return 1;
    }
    if (axis >= ndim) {
        set_error("CUDA cat backward axis is out of bounds");
        return 1;
    }
    if (input_shape == nullptr || input_strides == nullptr || out_strides == nullptr) {
        set_error("CUDA cat backward metadata pointers must not be null");
        return 1;
    }

    size_t* d_input_shape = nullptr;
    size_t* d_input_strides = nullptr;
    size_t* d_out_strides = nullptr;

    cudaError_t status = cudaMalloc(reinterpret_cast<void**>(&d_input_shape), ndim * sizeof(size_t));
    if (status != cudaSuccess) {
        set_cuda_error("failed to allocate CUDA cat backward shape buffer", status);
        return 1;
    }
    status = cudaMalloc(reinterpret_cast<void**>(&d_input_strides), ndim * sizeof(size_t));
    if (status != cudaSuccess) {
        cudaFree(d_input_shape);
        set_cuda_error("failed to allocate CUDA cat backward input stride buffer", status);
        return 1;
    }
    status = cudaMalloc(reinterpret_cast<void**>(&d_out_strides), ndim * sizeof(size_t));
    if (status != cudaSuccess) {
        cudaFree(d_input_shape);
        cudaFree(d_input_strides);
        set_cuda_error("failed to allocate CUDA cat backward output stride buffer", status);
        return 1;
    }

    status = cudaMemcpy(d_input_shape, input_shape, ndim * sizeof(size_t), cudaMemcpyHostToDevice);
    if (status != cudaSuccess) {
        cudaFree(d_input_shape);
        cudaFree(d_input_strides);
        cudaFree(d_out_strides);
        set_cuda_error("failed to upload CUDA cat backward shape buffer", status);
        return 1;
    }
    status = cudaMemcpy(d_input_strides, input_strides, ndim * sizeof(size_t), cudaMemcpyHostToDevice);
    if (status != cudaSuccess) {
        cudaFree(d_input_shape);
        cudaFree(d_input_strides);
        cudaFree(d_out_strides);
        set_cuda_error("failed to upload CUDA cat backward input stride buffer", status);
        return 1;
    }
    status = cudaMemcpy(d_out_strides, out_strides, ndim * sizeof(size_t), cudaMemcpyHostToDevice);
    if (status != cudaSuccess) {
        cudaFree(d_input_shape);
        cudaFree(d_input_strides);
        cudaFree(d_out_strides);
        set_cuda_error("failed to upload CUDA cat backward output stride buffer", status);
        return 1;
    }

    constexpr int block_size = 256;
    size_t grid_size = (len + block_size - 1) / block_size;
    cat_backward_slice_kernel<<<grid_size, block_size>>>(
        handle_to_ptr(grad_handle),
        handle_to_ptr(out_handle),
        d_input_shape,
        d_input_strides,
        d_out_strides,
        ndim,
        axis,
        axis_start,
        len);

    bool ok = sync_cuda("CUDA cat backward kernel failed");
    cudaFree(d_input_shape);
    cudaFree(d_input_strides);
    cudaFree(d_out_strides);
    return ok ? 0 : 1;
}

extern "C" int lumen_cuda_repeat_kv_f32_device(
    uint64_t input_handle,
    uint64_t out_handle,
    size_t batch_size,
    size_t num_kv_heads,
    size_t seq_len,
    size_t dim,
    size_t n_rep) {
    if (!validate_handle(input_handle, "CUDA repeat_kv input handle") ||
        !validate_handle(out_handle, "CUDA repeat_kv output handle")) {
        return 1;
    }
    if (batch_size == 0 || num_kv_heads == 0 || seq_len == 0 || dim == 0 || n_rep == 0) {
        set_error("CUDA repeat_kv dimensions must be greater than zero");
        return 1;
    }

    size_t num_heads = batch_size * num_kv_heads * n_rep;
    size_t len = num_heads * seq_len * dim;
    constexpr int block_size = 256;
    size_t grid_size = (len + block_size - 1) / block_size;
    repeat_kv_kernel<<<grid_size, block_size>>>(
        handle_to_ptr(input_handle),
        handle_to_ptr(out_handle),
        num_heads,
        seq_len,
        dim,
        n_rep);
    if (!sync_cuda("CUDA repeat_kv kernel failed")) {
        return 1;
    }
    return 0;
}

extern "C" int lumen_cuda_append_kv_cache_pair_f32_device(
    uint64_t k_dst_handle,
    uint64_t v_dst_handle,
    uint64_t k_src_handle,
    uint64_t v_src_handle,
    size_t batch_size,
    size_t num_heads,
    size_t src_seq_len,
    size_t dst_seq_len,
    size_t dim,
    size_t dst_start) {
    if (!validate_handle(k_dst_handle, "CUDA KV cache K destination handle") ||
        !validate_handle(v_dst_handle, "CUDA KV cache V destination handle") ||
        !validate_handle(k_src_handle, "CUDA KV cache K source handle") ||
        !validate_handle(v_src_handle, "CUDA KV cache V source handle")) {
        return 1;
    }
    if (batch_size == 0 || num_heads == 0 || src_seq_len == 0 || dst_seq_len == 0 || dim == 0) {
        set_error("CUDA KV cache pair append dimensions must be greater than zero");
        return 1;
    }
    if (dst_start > dst_seq_len || src_seq_len > dst_seq_len - dst_start) {
        set_error("CUDA KV cache pair append range is out of bounds");
        return 1;
    }

    constexpr int block_size = 256;
    size_t total = batch_size * num_heads * src_seq_len * dim;
    size_t grid_size = (total + block_size - 1) / block_size;
    append_kv_cache_pair_kernel<<<grid_size, block_size>>>(
        handle_to_ptr(k_dst_handle),
        handle_to_ptr(v_dst_handle),
        handle_to_ptr(k_src_handle),
        handle_to_ptr(v_src_handle),
        batch_size,
        num_heads,
        src_seq_len,
        dst_seq_len,
        dim,
        dst_start);
    if (!sync_cuda("CUDA KV cache pair append kernel failed")) {
        return 1;
    }
    return 0;
}

extern "C" int lumen_cuda_decode_rope_q_append_kv_f32_device(
    uint64_t q_src_handle,
    uint64_t k_src_handle,
    uint64_t v_src_handle,
    uint64_t cos_handle,
    uint64_t sin_handle,
    uint64_t q_out_handle,
    uint64_t k_cache_handle,
    uint64_t v_cache_handle,
    size_t batch_size,
    size_t num_heads,
    size_t num_kv_heads,
    size_t dim,
    size_t dst_seq_len,
    size_t offset,
    size_t cache_seq_len) {
    if (!validate_handle(q_src_handle, "CUDA decode RoPE Q source handle") ||
        !validate_handle(k_src_handle, "CUDA decode RoPE K source handle") ||
        !validate_handle(v_src_handle, "CUDA decode RoPE V source handle") ||
        !validate_handle(cos_handle, "CUDA decode RoPE cos handle") ||
        !validate_handle(sin_handle, "CUDA decode RoPE sin handle") ||
        !validate_handle(q_out_handle, "CUDA decode RoPE Q output handle") ||
        !validate_handle(k_cache_handle, "CUDA decode RoPE K cache handle") ||
        !validate_handle(v_cache_handle, "CUDA decode RoPE V cache handle")) {
        return 1;
    }
    if (batch_size == 0 || num_heads == 0 || num_kv_heads == 0 || dim == 0 || dst_seq_len == 0 ||
        cache_seq_len == 0) {
        set_error("CUDA decode RoPE append dimensions must be greater than zero");
        return 1;
    }
    if ((dim % 2) != 0) {
        set_error("CUDA decode RoPE append expects an even hidden dimension");
        return 1;
    }
    if (offset >= dst_seq_len || offset >= cache_seq_len) {
        set_error("CUDA decode RoPE append offset is out of bounds");
        return 1;
    }

    constexpr int block_size = 256;
    size_t q_len = batch_size * num_heads * dim;
    size_t kv_len = batch_size * num_kv_heads * dim;
    size_t total = q_len > kv_len ? q_len : kv_len;
    size_t grid_size = (total + block_size - 1) / block_size;
    decode_rope_q_append_kv_kernel<<<grid_size, block_size>>>(
        handle_to_ptr(q_src_handle),
        handle_to_ptr(k_src_handle),
        handle_to_ptr(v_src_handle),
        handle_to_ptr(cos_handle),
        handle_to_ptr(sin_handle),
        handle_to_ptr(q_out_handle),
        handle_to_ptr(k_cache_handle),
        handle_to_ptr(v_cache_handle),
        batch_size,
        num_heads,
        num_kv_heads,
        dim,
        dst_seq_len,
        offset);
    if (!sync_cuda("CUDA decode RoPE append kernel failed")) {
        return 1;
    }
    return 0;
}

extern "C" int lumen_cuda_repeat_kv_backward_f32_device(
    uint64_t grad_handle,
    uint64_t out_handle,
    size_t batch_size,
    size_t num_kv_heads,
    size_t seq_len,
    size_t dim,
    size_t n_rep) {
    if (!validate_handle(grad_handle, "CUDA repeat_kv backward grad handle") ||
        !validate_handle(out_handle, "CUDA repeat_kv backward output handle")) {
        return 1;
    }
    if (batch_size == 0 || num_kv_heads == 0 || seq_len == 0 || dim == 0 || n_rep == 0) {
        set_error("CUDA repeat_kv backward dimensions must be greater than zero");
        return 1;
    }

    size_t input_len = batch_size * num_kv_heads * seq_len * dim;
    constexpr int block_size = 256;
    size_t grid_size = (input_len + block_size - 1) / block_size;
    repeat_kv_backward_kernel<<<grid_size, block_size>>>(
        handle_to_ptr(grad_handle),
        handle_to_ptr(out_handle),
        batch_size,
        num_kv_heads,
        seq_len,
        dim,
        n_rep);
    if (!sync_cuda("CUDA repeat_kv backward kernel failed")) {
        return 1;
    }
    return 0;
}

extern "C" int lumen_cuda_decode_attention_f32_device(
    uint64_t q_handle,
    uint64_t k_handle,
    uint64_t v_handle,
    uint64_t out_handle,
    size_t batch_size,
    size_t num_heads,
    size_t num_kv_heads,
    size_t active_seq_len,
    size_t cache_seq_len,
    size_t dim,
    size_t n_rep,
    float scale) {
    if (!validate_handle(q_handle, "CUDA decode attention q handle") ||
        !validate_handle(k_handle, "CUDA decode attention k handle") ||
        !validate_handle(v_handle, "CUDA decode attention v handle") ||
        !validate_handle(out_handle, "CUDA decode attention out handle")) {
        return 1;
    }
    if (batch_size == 0 || num_heads == 0 || num_kv_heads == 0 || active_seq_len == 0 ||
        cache_seq_len == 0 || dim == 0 || n_rep == 0) {
        set_error("CUDA decode attention dimensions must be greater than zero");
        return 1;
    }
    if (active_seq_len > cache_seq_len) {
        set_error("CUDA decode attention active sequence length exceeds cache length");
        return 1;
    }

    constexpr int block_size = 256;
    size_t rows = batch_size * num_heads;
    size_t shared_bytes = (block_size + 2 * dim + 4) * sizeof(float);
    decode_attention_kernel<<<rows, block_size, shared_bytes>>>(
        handle_to_ptr(q_handle),
        handle_to_ptr(k_handle),
        handle_to_ptr(v_handle),
        handle_to_ptr(out_handle),
        num_heads,
        num_kv_heads,
        active_seq_len,
        cache_seq_len,
        dim,
        n_rep,
        scale);
    if (!sync_cuda("CUDA decode attention kernel failed")) {
        return 1;
    }
    return 0;
}

extern "C" int lumen_cuda_prefill_attention_f32_device(
    uint64_t q_handle,
    uint64_t k_handle,
    uint64_t v_handle,
    uint64_t out_handle,
    size_t batch_size,
    size_t num_heads,
    size_t num_kv_heads,
    size_t q_seq_len,
    size_t active_seq_len,
    size_t cache_seq_len,
    size_t dim,
    size_t n_rep,
    size_t past_len,
    float scale,
    int is_causal) {
    if (!validate_handle(q_handle, "CUDA prefill attention q handle") ||
        !validate_handle(k_handle, "CUDA prefill attention k handle") ||
        !validate_handle(v_handle, "CUDA prefill attention v handle") ||
        !validate_handle(out_handle, "CUDA prefill attention out handle")) {
        return 1;
    }
    if (batch_size == 0 || num_heads == 0 || num_kv_heads == 0 || q_seq_len == 0 ||
        active_seq_len == 0 || cache_seq_len == 0 || dim == 0 || n_rep == 0) {
        set_error("CUDA prefill attention dimensions must be greater than zero");
        return 1;
    }
    if (active_seq_len > cache_seq_len || past_len + q_seq_len > active_seq_len) {
        set_error("CUDA prefill attention sequence range is out of bounds");
        return 1;
    }

    constexpr int block_size = 256;
    size_t rows = batch_size * num_heads * q_seq_len;
    size_t shared_bytes = (block_size + 2 * dim + 4) * sizeof(float);
    prefill_attention_kernel<<<rows, block_size, shared_bytes>>>(
        handle_to_ptr(q_handle),
        handle_to_ptr(k_handle),
        handle_to_ptr(v_handle),
        handle_to_ptr(out_handle),
        num_heads,
        num_kv_heads,
        q_seq_len,
        active_seq_len,
        cache_seq_len,
        dim,
        n_rep,
        past_len,
        scale,
        is_causal);
    if (!sync_cuda("CUDA prefill attention kernel failed")) {
        return 1;
    }
    return 0;
}

extern "C" int lumen_cuda_fused_gate_up_silu_f32_device(
    uint64_t input_handle,
    uint64_t gate_handle,
    uint64_t up_handle,
    uint64_t out_handle,
    size_t rows,
    size_t n_dim,
    size_t k_dim) {
    if (!validate_handle(input_handle, "CUDA fused gate/up input handle") ||
        !validate_handle(gate_handle, "CUDA fused gate/up gate handle") ||
        !validate_handle(up_handle, "CUDA fused gate/up up handle") ||
        !validate_handle(out_handle, "CUDA fused gate/up output handle")) {
        return 1;
    }
    if (rows == 0 || n_dim == 0 || k_dim == 0) {
        set_error("CUDA fused gate/up dimensions must be greater than zero");
        return 1;
    }

    CublasHandle cublas;
    if (!init_cublas(cublas)) {
        return 1;
    }

    const size_t max_size = static_cast<size_t>(-1);
    if (rows > max_size / n_dim || rows * n_dim > max_size / sizeof(float)) {
        set_error("CUDA fused gate/up temporary length overflow");
        return 1;
    }
    size_t len = rows * n_dim;

    thread_local ReusableCudaWorkspace gate_tmp_workspace;
    if (!gate_tmp_workspace.ensure(
            len * sizeof(float),
            "failed to allocate CUDA fused gate temporary buffer")) {
        return 1;
    }
    thread_local ReusableCudaWorkspace up_tmp_workspace;
    if (!up_tmp_workspace.ensure(
            len * sizeof(float),
            "failed to allocate CUDA fused up temporary buffer")) {
        return 1;
    }
    float* gate_tmp = static_cast<float*>(gate_tmp_workspace.ptr);
    float* up_tmp = static_cast<float*>(up_tmp_workspace.ptr);

    const float alpha = 1.0f;
    const float beta = 0.0f;
    const float* input = handle_to_ptr(input_handle);
    const float* gate = handle_to_ptr(gate_handle);
    const float* up = handle_to_ptr(up_handle);
    cublasStatus_t cublas_status = cublasSgemm(
        cublas.handle,
        CUBLAS_OP_T,
        CUBLAS_OP_N,
        static_cast<int>(n_dim),
        static_cast<int>(rows),
        static_cast<int>(k_dim),
        &alpha,
        gate,
        static_cast<int>(k_dim),
        input,
        static_cast<int>(k_dim),
        &beta,
        gate_tmp,
        static_cast<int>(n_dim));
    if (cublas_status != CUBLAS_STATUS_SUCCESS) {
        set_cublas_error("cuBLAS SGEMM failed for fused gate projection", cublas_status);
        return 1;
    }

    cublas_status = cublasSgemm(
        cublas.handle,
        CUBLAS_OP_T,
        CUBLAS_OP_N,
        static_cast<int>(n_dim),
        static_cast<int>(rows),
        static_cast<int>(k_dim),
        &alpha,
        up,
        static_cast<int>(k_dim),
        input,
        static_cast<int>(k_dim),
        &beta,
        up_tmp,
        static_cast<int>(n_dim));
    if (cublas_status != CUBLAS_STATUS_SUCCESS) {
        set_cublas_error("cuBLAS SGEMM failed for fused up projection", cublas_status);
        return 1;
    }

    constexpr int block_size = 256;
    size_t grid_size = (len + block_size - 1) / block_size;
    silu_mul_kernel<<<grid_size, block_size>>>(gate_tmp, up_tmp, handle_to_ptr(out_handle), len);
    bool ok = sync_cuda("CUDA fused gate/up kernel failed");
    return ok ? 0 : 1;
}

extern "C" int lumen_cuda_fused_qkv_f32_device(
    uint64_t input_handle,
    uint64_t q_handle,
    uint64_t k_handle,
    uint64_t v_handle,
    uint64_t q_out_handle,
    uint64_t k_out_handle,
    uint64_t v_out_handle,
    size_t rows,
    size_t q_n,
    size_t k_n,
    size_t k_dim) {
    if (!validate_handle(input_handle, "CUDA fused qkv input handle") ||
        !validate_handle(q_handle, "CUDA fused qkv q handle") ||
        !validate_handle(k_handle, "CUDA fused qkv k handle") ||
        !validate_handle(v_handle, "CUDA fused qkv v handle") ||
        !validate_handle(q_out_handle, "CUDA fused qkv q output handle") ||
        !validate_handle(k_out_handle, "CUDA fused qkv k output handle") ||
        !validate_handle(v_out_handle, "CUDA fused qkv v output handle")) {
        return 1;
    }
    if (rows == 0 || q_n == 0 || k_n == 0 || k_dim == 0) {
        set_error("CUDA fused qkv dimensions must be greater than zero");
        return 1;
    }

    CublasHandle cublas;
    if (!init_cublas(cublas)) {
        return 1;
    }

    const float alpha = 1.0f;
    const float beta = 0.0f;
    const float* input = handle_to_ptr(input_handle);
    cublasStatus_t cublas_status = cublasSgemm(
        cublas.handle,
        CUBLAS_OP_T,
        CUBLAS_OP_N,
        static_cast<int>(q_n),
        static_cast<int>(rows),
        static_cast<int>(k_dim),
        &alpha,
        handle_to_ptr(q_handle),
        static_cast<int>(k_dim),
        input,
        static_cast<int>(k_dim),
        &beta,
        handle_to_ptr(q_out_handle),
        static_cast<int>(q_n));
    if (cublas_status != CUBLAS_STATUS_SUCCESS) {
        set_cublas_error("cuBLAS SGEMM failed for fused q projection", cublas_status);
        return 1;
    }

    cublas_status = cublasSgemm(
        cublas.handle,
        CUBLAS_OP_T,
        CUBLAS_OP_N,
        static_cast<int>(k_n),
        static_cast<int>(rows),
        static_cast<int>(k_dim),
        &alpha,
        handle_to_ptr(k_handle),
        static_cast<int>(k_dim),
        input,
        static_cast<int>(k_dim),
        &beta,
        handle_to_ptr(k_out_handle),
        static_cast<int>(k_n));
    if (cublas_status != CUBLAS_STATUS_SUCCESS) {
        set_cublas_error("cuBLAS SGEMM failed for fused k projection", cublas_status);
        return 1;
    }

    cublas_status = cublasSgemm(
        cublas.handle,
        CUBLAS_OP_T,
        CUBLAS_OP_N,
        static_cast<int>(k_n),
        static_cast<int>(rows),
        static_cast<int>(k_dim),
        &alpha,
        handle_to_ptr(v_handle),
        static_cast<int>(k_dim),
        input,
        static_cast<int>(k_dim),
        &beta,
        handle_to_ptr(v_out_handle),
        static_cast<int>(k_n));
    if (cublas_status != CUBLAS_STATUS_SUCCESS) {
        set_cublas_error("cuBLAS SGEMM failed for fused v projection", cublas_status);
        return 1;
    }

    if (!sync_cuda("CUDA fused qkv failed")) {
        return 1;
    }
    return 0;
}

extern "C" int lumen_cuda_rope_f32_device(
    uint64_t input_handle,
    uint64_t cos_handle,
    uint64_t sin_handle,
    uint64_t out_handle,
    size_t batch_size,
    size_t num_heads,
    size_t seq_len,
    size_t dim,
    size_t offset,
    size_t cache_seq_len) {
    if (!validate_handle(input_handle, "CUDA RoPE input handle") ||
        !validate_handle(cos_handle, "CUDA RoPE cos handle") ||
        !validate_handle(sin_handle, "CUDA RoPE sin handle") ||
        !validate_handle(out_handle, "CUDA RoPE output handle")) {
        return 1;
    }
    if (batch_size == 0 || num_heads == 0 || seq_len == 0 || dim == 0) {
        set_error("CUDA RoPE dimensions must be greater than zero");
        return 1;
    }
    if ((dim % 2) != 0) {
        set_error("CUDA RoPE expects an even hidden dimension");
        return 1;
    }
    if (offset + seq_len > cache_seq_len) {
        set_error("CUDA RoPE offset exceeds cache sequence length");
        return 1;
    }

    size_t batch_heads = batch_size * num_heads;
    size_t half = dim / 2;
    size_t pairs_per_head = seq_len * half;
    constexpr int block_size = 256;
    size_t grid_x = (pairs_per_head + block_size - 1) / block_size;
    if (grid_x == 0) {
        grid_x = 1;
    }

    dim3 grid(static_cast<unsigned int>(grid_x), static_cast<unsigned int>(batch_heads), 1);
    rope_kernel<<<grid, block_size>>>(
        handle_to_ptr(input_handle),
        handle_to_ptr(cos_handle),
        handle_to_ptr(sin_handle),
        handle_to_ptr(out_handle),
        seq_len,
        dim,
        offset);
    if (!sync_cuda("CUDA RoPE kernel failed")) {
        return 1;
    }
    return 0;
}

extern "C" int lumen_cuda_rope_backward_f32_device(
    uint64_t grad_handle,
    uint64_t cos_handle,
    uint64_t sin_handle,
    uint64_t out_handle,
    size_t batch_size,
    size_t num_heads,
    size_t seq_len,
    size_t dim,
    size_t offset,
    size_t cache_seq_len) {
    if (!validate_handle(grad_handle, "CUDA RoPE backward grad handle") ||
        !validate_handle(cos_handle, "CUDA RoPE backward cos handle") ||
        !validate_handle(sin_handle, "CUDA RoPE backward sin handle") ||
        !validate_handle(out_handle, "CUDA RoPE backward output handle")) {
        return 1;
    }
    if (batch_size == 0 || num_heads == 0 || seq_len == 0 || dim == 0) {
        set_error("CUDA RoPE backward dimensions must be greater than zero");
        return 1;
    }
    if (dim % 2 != 0) {
        set_error("CUDA RoPE backward expects an even hidden dimension");
        return 1;
    }
    if (offset + seq_len > cache_seq_len) {
        set_error("CUDA RoPE backward offset exceeds cache sequence length");
        return 1;
    }

    size_t batch_heads = batch_size * num_heads;
    size_t half = dim / 2;
    size_t pairs_per_head = seq_len * half;
    constexpr int block_size = 256;
    size_t grid_x = (pairs_per_head + block_size - 1) / block_size;
    if (grid_x == 0) {
        grid_x = 1;
    }

    dim3 grid(static_cast<unsigned int>(grid_x), static_cast<unsigned int>(batch_heads), 1);
    rope_backward_kernel<<<grid, block_size>>>(
        handle_to_ptr(grad_handle),
        handle_to_ptr(cos_handle),
        handle_to_ptr(sin_handle),
        handle_to_ptr(out_handle),
        seq_len,
        dim,
        offset);
    if (!sync_cuda("CUDA RoPE backward kernel failed")) {
        return 1;
    }
    return 0;
}

extern "C" int lumen_cuda_conv2d_f32_device(
    uint64_t input_handle,
    uint64_t weight_handle,
    uint64_t bias_handle,
    uint64_t out_handle,
    size_t batch_size,
    size_t in_channels,
    size_t in_h,
    size_t in_w,
    size_t out_channels,
    size_t k_h,
    size_t k_w,
    size_t pad_h,
    size_t pad_w,
    size_t stride_h,
    size_t stride_w,
    size_t out_h,
    size_t out_w) {
    if (!validate_handle(input_handle, "CUDA conv2d input handle") ||
        !validate_handle(weight_handle, "CUDA conv2d weight handle") ||
        !validate_handle(out_handle, "CUDA conv2d output handle")) {
        return 1;
    }
    if (batch_size == 0 || in_channels == 0 || in_h == 0 || in_w == 0 || out_channels == 0 ||
        k_h == 0 || k_w == 0 || stride_h == 0 || stride_w == 0 || out_h == 0 || out_w == 0) {
        set_error("CUDA conv2d dimensions must be greater than zero");
        return 1;
    }

#if !LUMEN_HAS_CUDNN
    set_error("CUDA conv2d requires cuDNN support");
    return 1;
#else
    CudnnHandle handle;
    CudnnTensorDescriptor input_desc;
    CudnnTensorDescriptor output_desc;
    CudnnTensorDescriptor bias_desc;
    CudnnFilterDescriptor filter_desc;
    CudnnConvolutionDescriptor conv_desc;
    if (!init_cudnn(handle) ||
        !init_tensor_descriptor_4d(
            input_desc,
            static_cast<int>(batch_size),
            static_cast<int>(in_channels),
            static_cast<int>(in_h),
            static_cast<int>(in_w)) ||
        !init_tensor_descriptor_4d(
            output_desc,
            static_cast<int>(batch_size),
            static_cast<int>(out_channels),
            static_cast<int>(out_h),
            static_cast<int>(out_w)) ||
        !init_filter_descriptor_4d(
            filter_desc,
            static_cast<int>(out_channels),
            static_cast<int>(in_channels),
            static_cast<int>(k_h),
            static_cast<int>(k_w)) ||
        !init_convolution_descriptor_2d(
            conv_desc,
            static_cast<int>(pad_h),
            static_cast<int>(pad_w),
            static_cast<int>(stride_h),
            static_cast<int>(stride_w))) {
        return 1;
    }

    int cudnn_n = 0;
    int cudnn_c = 0;
    int cudnn_h = 0;
    int cudnn_w = 0;
    cudnnStatus_t status = cudnnGetConvolution2dForwardOutputDim(
        conv_desc.desc,
        input_desc.desc,
        filter_desc.desc,
        &cudnn_n,
        &cudnn_c,
        &cudnn_h,
        &cudnn_w);
    if (status != CUDNN_STATUS_SUCCESS) {
        set_cudnn_error("failed to query cuDNN conv2d output shape", status);
        return 1;
    }
    if (cudnn_n != static_cast<int>(batch_size) || cudnn_c != static_cast<int>(out_channels) ||
        cudnn_h != static_cast<int>(out_h) || cudnn_w != static_cast<int>(out_w)) {
        set_error("cuDNN conv2d output shape does not match the expected dimensions");
        return 1;
    }

    const float alpha = 1.0f;
    const float beta = 0.0f;
    cudnnConvolutionFwdAlgo_t fwd_algo;
    size_t fwd_workspace_bytes = 0;
    if (!select_cudnn_fwd_algo(
            handle.handle,
            input_desc.desc,
            filter_desc.desc,
            conv_desc.desc,
            output_desc.desc,
            fwd_algo,
            fwd_workspace_bytes)) {
        return 1;
    }
    CudaWorkspace workspace;
    if (!workspace.allocate(fwd_workspace_bytes, "failed to allocate cuDNN conv2d forward workspace")) {
        fwd_algo = CUDNN_CONVOLUTION_FWD_ALGO_IMPLICIT_GEMM;
        fwd_workspace_bytes = 0;
    }
    status = cudnnConvolutionForward(
        handle.handle,
        &alpha,
        input_desc.desc,
        handle_to_ptr(input_handle),
        filter_desc.desc,
        handle_to_ptr(weight_handle),
        conv_desc.desc,
        fwd_algo,
        workspace.ptr,
        fwd_workspace_bytes,
        &beta,
        output_desc.desc,
        handle_to_ptr(out_handle));
    if (status != CUDNN_STATUS_SUCCESS && fwd_algo != CUDNN_CONVOLUTION_FWD_ALGO_IMPLICIT_GEMM) {
        fwd_workspace_bytes = 0;
        status = cudnnConvolutionForward(
            handle.handle,
            &alpha,
            input_desc.desc,
            handle_to_ptr(input_handle),
            filter_desc.desc,
            handle_to_ptr(weight_handle),
            conv_desc.desc,
            CUDNN_CONVOLUTION_FWD_ALGO_IMPLICIT_GEMM,
            nullptr,
            0,
            &beta,
            output_desc.desc,
            handle_to_ptr(out_handle));
    }
    if (status != CUDNN_STATUS_SUCCESS) {
        set_cudnn_error("cuDNN conv2d forward failed", status);
        return 1;
    }

    if (bias_handle != 0) {
        if (!init_tensor_descriptor_4d(
                bias_desc, 1, static_cast<int>(out_channels), 1, 1)) {
            return 1;
        }
        const float add_alpha = 1.0f;
        const float add_beta = 1.0f;
        status = cudnnAddTensor(
            handle.handle,
            &add_alpha,
            bias_desc.desc,
            handle_to_ptr(bias_handle),
            &add_beta,
            output_desc.desc,
            handle_to_ptr(out_handle));
        if (status != CUDNN_STATUS_SUCCESS) {
            set_cudnn_error("cuDNN conv2d bias add failed", status);
            return 1;
        }
    }

    if (!sync_cuda("CUDA conv2d failed")) {
        return 1;
    }
    return 0;
#endif
}

extern "C" int lumen_cuda_conv2d_backward_f32_device(
    uint64_t input_handle,
    uint64_t weight_handle,
    uint64_t grad_output_handle,
    uint64_t grad_input_handle,
    uint64_t grad_weight_handle,
    uint64_t grad_bias_handle,
    size_t batch_size,
    size_t in_channels,
    size_t in_h,
    size_t in_w,
    size_t out_channels,
    size_t k_h,
    size_t k_w,
    size_t pad_h,
    size_t pad_w,
    size_t stride_h,
    size_t stride_w,
    size_t out_h,
    size_t out_w) {
    if (!validate_handle(input_handle, "CUDA conv2d backward input handle") ||
        !validate_handle(weight_handle, "CUDA conv2d backward weight handle") ||
        !validate_handle(grad_output_handle, "CUDA conv2d backward grad output handle") ||
        !validate_handle(grad_input_handle, "CUDA conv2d backward grad input handle") ||
        !validate_handle(grad_weight_handle, "CUDA conv2d backward grad weight handle")) {
        return 1;
    }
    if (batch_size == 0 || in_channels == 0 || in_h == 0 || in_w == 0 || out_channels == 0 ||
        k_h == 0 || k_w == 0 || stride_h == 0 || stride_w == 0 || out_h == 0 || out_w == 0) {
        set_error("CUDA conv2d backward dimensions must be greater than zero");
        return 1;
    }

#if !LUMEN_HAS_CUDNN
    set_error("CUDA conv2d backward requires cuDNN support");
    return 1;
#else
    CudnnHandle handle;
    CudnnTensorDescriptor input_desc;
    CudnnTensorDescriptor grad_output_desc;
    CudnnTensorDescriptor bias_desc;
    CudnnFilterDescriptor filter_desc;
    CudnnConvolutionDescriptor conv_desc;
    if (!init_cudnn(handle) ||
        !init_tensor_descriptor_4d(
            input_desc,
            static_cast<int>(batch_size),
            static_cast<int>(in_channels),
            static_cast<int>(in_h),
            static_cast<int>(in_w)) ||
        !init_tensor_descriptor_4d(
            grad_output_desc,
            static_cast<int>(batch_size),
            static_cast<int>(out_channels),
            static_cast<int>(out_h),
            static_cast<int>(out_w)) ||
        !init_filter_descriptor_4d(
            filter_desc,
            static_cast<int>(out_channels),
            static_cast<int>(in_channels),
            static_cast<int>(k_h),
            static_cast<int>(k_w)) ||
        !init_convolution_descriptor_2d(
            conv_desc,
            static_cast<int>(pad_h),
            static_cast<int>(pad_w),
            static_cast<int>(stride_h),
            static_cast<int>(stride_w))) {
        return 1;
    }

    const float alpha = 1.0f;
    const float beta = 0.0f;
    cudnnConvolutionBwdDataAlgo_t bwd_data_algo;
    cudnnConvolutionBwdFilterAlgo_t bwd_filter_algo;
    size_t bwd_data_workspace_bytes = 0;
    size_t bwd_filter_workspace_bytes = 0;
    if (!select_cudnn_bwd_data_algo(
            handle.handle,
            filter_desc.desc,
            grad_output_desc.desc,
            conv_desc.desc,
            input_desc.desc,
            bwd_data_algo,
            bwd_data_workspace_bytes) ||
        !select_cudnn_bwd_filter_algo(
            handle.handle,
            input_desc.desc,
            grad_output_desc.desc,
            conv_desc.desc,
            filter_desc.desc,
            bwd_filter_algo,
            bwd_filter_workspace_bytes)) {
        return 1;
    }
    size_t workspace_bytes =
        bwd_data_workspace_bytes > bwd_filter_workspace_bytes
            ? bwd_data_workspace_bytes
            : bwd_filter_workspace_bytes;
    CudaWorkspace workspace;
    if (!workspace.allocate(workspace_bytes, "failed to allocate cuDNN conv2d backward workspace")) {
        bwd_data_algo = CUDNN_CONVOLUTION_BWD_DATA_ALGO_0;
        bwd_filter_algo = CUDNN_CONVOLUTION_BWD_FILTER_ALGO_0;
        bwd_data_workspace_bytes = 0;
        bwd_filter_workspace_bytes = 0;
    }
    cudnnStatus_t status = cudnnConvolutionBackwardData(
        handle.handle,
        &alpha,
        filter_desc.desc,
        handle_to_ptr(weight_handle),
        grad_output_desc.desc,
        handle_to_ptr(grad_output_handle),
        conv_desc.desc,
        bwd_data_algo,
        workspace.ptr,
        bwd_data_workspace_bytes,
        &beta,
        input_desc.desc,
        handle_to_ptr(grad_input_handle));
    if (status != CUDNN_STATUS_SUCCESS && bwd_data_algo != CUDNN_CONVOLUTION_BWD_DATA_ALGO_0) {
        bwd_data_workspace_bytes = 0;
        status = cudnnConvolutionBackwardData(
            handle.handle,
            &alpha,
            filter_desc.desc,
            handle_to_ptr(weight_handle),
            grad_output_desc.desc,
            handle_to_ptr(grad_output_handle),
            conv_desc.desc,
            CUDNN_CONVOLUTION_BWD_DATA_ALGO_0,
            nullptr,
            0,
            &beta,
            input_desc.desc,
            handle_to_ptr(grad_input_handle));
    }
    if (status != CUDNN_STATUS_SUCCESS) {
        set_cudnn_error("cuDNN conv2d backward data failed", status);
        return 1;
    }

    status = cudnnConvolutionBackwardFilter(
        handle.handle,
        &alpha,
        input_desc.desc,
        handle_to_ptr(input_handle),
        grad_output_desc.desc,
        handle_to_ptr(grad_output_handle),
        conv_desc.desc,
        bwd_filter_algo,
        workspace.ptr,
        bwd_filter_workspace_bytes,
        &beta,
        filter_desc.desc,
        handle_to_ptr(grad_weight_handle));
    if (status != CUDNN_STATUS_SUCCESS && bwd_filter_algo != CUDNN_CONVOLUTION_BWD_FILTER_ALGO_0) {
        bwd_filter_workspace_bytes = 0;
        status = cudnnConvolutionBackwardFilter(
            handle.handle,
            &alpha,
            input_desc.desc,
            handle_to_ptr(input_handle),
            grad_output_desc.desc,
            handle_to_ptr(grad_output_handle),
            conv_desc.desc,
            CUDNN_CONVOLUTION_BWD_FILTER_ALGO_0,
            nullptr,
            0,
            &beta,
            filter_desc.desc,
            handle_to_ptr(grad_weight_handle));
    }
    if (status != CUDNN_STATUS_SUCCESS) {
        set_cudnn_error("cuDNN conv2d backward filter failed", status);
        return 1;
    }

    if (grad_bias_handle != 0) {
        if (!init_tensor_descriptor_4d(
                bias_desc, 1, static_cast<int>(out_channels), 1, 1)) {
            return 1;
        }
        status = cudnnConvolutionBackwardBias(
            handle.handle,
            &alpha,
            grad_output_desc.desc,
            handle_to_ptr(grad_output_handle),
            &beta,
            bias_desc.desc,
            handle_to_ptr(grad_bias_handle));
        if (status != CUDNN_STATUS_SUCCESS) {
            set_cudnn_error("cuDNN conv2d backward bias failed", status);
            return 1;
        }
    }

    if (!sync_cuda("CUDA conv2d backward failed")) {
        return 1;
    }
    return 0;
#endif
}

__global__ void max_pool2d_forward_kernel(
    const float* input,
    float* out,
    size_t total,
    size_t channels,
    size_t in_h,
    size_t in_w,
    size_t kernel_h,
    size_t kernel_w,
    size_t stride_h,
    size_t stride_w,
    size_t out_h,
    size_t out_w) {
    size_t idx = blockIdx.x * blockDim.x + threadIdx.x;
    if (idx >= total) {
        return;
    }

    size_t ow = idx % out_w;
    size_t oh = (idx / out_w) % out_h;
    size_t channel = (idx / (out_w * out_h)) % channels;
    size_t batch = idx / (channels * out_h * out_w);

    size_t h_start = oh * stride_h;
    size_t w_start = ow * stride_w;
    float max_val = -3.4028234663852886e+38F;
    for (size_t ky = 0; ky < kernel_h; ++ky) {
        for (size_t kx = 0; kx < kernel_w; ++kx) {
            size_t input_idx = ((batch * channels + channel) * in_h + h_start + ky) * in_w + w_start + kx;
            float value = input[input_idx];
            if (value > max_val) {
                max_val = value;
            }
        }
    }
    out[idx] = max_val;
}

__global__ void max_pool2d_backward_kernel(
    const float* input,
    const float* grad_output,
    float* grad_input,
    size_t total,
    size_t channels,
    size_t in_h,
    size_t in_w,
    size_t kernel_h,
    size_t kernel_w,
    size_t stride_h,
    size_t stride_w,
    size_t out_h,
    size_t out_w) {
    size_t idx = blockIdx.x * blockDim.x + threadIdx.x;
    if (idx >= total) {
        return;
    }

    size_t ow = idx % out_w;
    size_t oh = (idx / out_w) % out_h;
    size_t channel = (idx / (out_w * out_h)) % channels;
    size_t batch = idx / (channels * out_h * out_w);

    size_t h_start = oh * stride_h;
    size_t w_start = ow * stride_w;
    float max_val = -3.4028234663852886e+38F;
    size_t max_idx = 0;
    for (size_t ky = 0; ky < kernel_h; ++ky) {
        for (size_t kx = 0; kx < kernel_w; ++kx) {
            size_t input_idx = ((batch * channels + channel) * in_h + h_start + ky) * in_w + w_start + kx;
            float value = input[input_idx];
            if (value > max_val) {
                max_val = value;
                max_idx = input_idx;
            }
        }
    }
    atomicAdd(grad_input + max_idx, grad_output[idx]);
}

extern "C" int lumen_cuda_max_pool2d_f32_device(
    uint64_t input_handle,
    uint64_t out_handle,
    size_t batch_size,
    size_t channels,
    size_t in_h,
    size_t in_w,
    size_t kernel_h,
    size_t kernel_w,
    size_t stride_h,
    size_t stride_w,
    size_t out_h,
    size_t out_w) {
    if (!validate_handle(input_handle, "CUDA max_pool2d input handle") ||
        !validate_handle(out_handle, "CUDA max_pool2d output handle")) {
        return 1;
    }
    if (batch_size == 0 || channels == 0 || in_h == 0 || in_w == 0 || kernel_h == 0 ||
        kernel_w == 0 || stride_h == 0 || stride_w == 0 || out_h == 0 || out_w == 0) {
        set_error("CUDA max_pool2d dimensions must be greater than zero");
        return 1;
    }

    size_t total = batch_size * channels * out_h * out_w;
    constexpr int block_size = 256;
    int grid_size = static_cast<int>((total + block_size - 1) / block_size);
    max_pool2d_forward_kernel<<<grid_size, block_size>>>(
        handle_to_ptr(input_handle),
        handle_to_ptr(out_handle),
        total,
        channels,
        in_h,
        in_w,
        kernel_h,
        kernel_w,
        stride_h,
        stride_w,
        out_h,
        out_w);
    if (!sync_cuda("CUDA max_pool2d forward failed")) {
        return 1;
    }
    return 0;
}

extern "C" int lumen_cuda_max_pool2d_backward_f32_device(
    uint64_t input_handle,
    uint64_t grad_output_handle,
    uint64_t grad_input_handle,
    size_t batch_size,
    size_t channels,
    size_t in_h,
    size_t in_w,
    size_t kernel_h,
    size_t kernel_w,
    size_t stride_h,
    size_t stride_w,
    size_t out_h,
    size_t out_w) {
    if (!validate_handle(input_handle, "CUDA max_pool2d backward input handle") ||
        !validate_handle(grad_output_handle, "CUDA max_pool2d backward grad output handle") ||
        !validate_handle(grad_input_handle, "CUDA max_pool2d backward grad input handle")) {
        return 1;
    }
    if (batch_size == 0 || channels == 0 || in_h == 0 || in_w == 0 || kernel_h == 0 ||
        kernel_w == 0 || stride_h == 0 || stride_w == 0 || out_h == 0 || out_w == 0) {
        set_error("CUDA max_pool2d backward dimensions must be greater than zero");
        return 1;
    }

    size_t input_len = batch_size * channels * in_h * in_w;
    cudaError_t memset_status = cudaMemset(handle_to_ptr(grad_input_handle), 0, input_len * sizeof(float));
    if (memset_status != cudaSuccess) {
        set_cuda_error("CUDA max_pool2d backward grad input initialization failed", memset_status);
        return 1;
    }

    size_t total = batch_size * channels * out_h * out_w;
    constexpr int block_size = 256;
    int grid_size = static_cast<int>((total + block_size - 1) / block_size);
    max_pool2d_backward_kernel<<<grid_size, block_size>>>(
        handle_to_ptr(input_handle),
        handle_to_ptr(grad_output_handle),
        handle_to_ptr(grad_input_handle),
        total,
        channels,
        in_h,
        in_w,
        kernel_h,
        kernel_w,
        stride_h,
        stride_w,
        out_h,
        out_w);
    if (!sync_cuda("CUDA max_pool2d backward failed")) {
        return 1;
    }
    return 0;
}
